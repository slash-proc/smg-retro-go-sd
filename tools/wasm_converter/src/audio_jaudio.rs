//! JAudio archive and wave metadata parsing for recipe-based PCM generation.

use std::collections::BTreeMap;

#[derive(Clone, Debug)]
pub struct Wave {
    pub archive: String,
    pub id: u16,
    pub format: u8,
    pub base_key: u8,
    pub rate: usize,
    pub offset: usize,
    pub size: usize,
    pub looped: bool,
    pub loop_start: usize,
    pub loop_end: usize,
    pub sample_count: usize,
}

#[derive(Debug)]
pub struct AudioArchive {
    pub waves: BTreeMap<u32, BTreeMap<u16, Wave>>,
    pub banks: BTreeMap<u32, Bank>,
    pub bsc: Vec<u8>,
    pub bst: Vec<u8>,
    pub sequence_offsets: Vec<Vec<usize>>,
}

/// A sequence note resolved to the sampled instrument it plays.
///
/// This is the bridge between the sequence interpreter and PCM synthesis:
/// `wave` identifies the encoded sample, while `ratio`, `volume`, and the
/// envelope state describe how the sequence note uses it.
#[derive(Clone, Debug)]
pub struct ResolvedNote {
    pub wave: Wave,
    pub ratio: f64,
    pub volume: f64,
    pub oscillator: Option<Oscillator>,
    pub direct_release: u16,
    pub note: crate::audio_seq::AudioNote,
}

#[derive(Clone, Debug)]
pub struct RenderedPart {
    pub offset: usize,
    pub samples: Vec<f64>,
}

const ENVELOPE_RATE: f64 = 400.0;
const ENVELOPE_INCREMENT: f64 = 1.5;
const ENVELOPE_CURVES: [[f64; 17]; 4] = [
    [
        1.0, 0.9375, 0.875, 0.8125, 0.75, 0.6875, 0.625, 0.5625, 0.5, 0.4375, 0.375, 0.3125, 0.25,
        0.1875, 0.125, 0.0625, 0.0,
    ],
    [
        1.0, 0.968246, 0.935414, 0.901388, 0.866025, 0.829156, 0.790569, 0.75, 0.707107, 0.661438,
        0.612372, 0.559017, 0.5, 0.433013, 0.353553, 0.25, 0.0,
    ],
    [
        1.0, 0.878906, 0.765625, 0.660156, 0.5625, 0.472656, 0.390625, 0.316406, 0.25, 0.191406,
        0.140625, 0.097656, 0.0625, 0.035156, 0.015625, 0.003906, 0.0,
    ],
    [
        1.0, 0.970489, 0.781274, 0.546281, 0.399792, 0.289315, 0.212104, 0.157476, 0.112613,
        0.08179, 0.057985, 0.043642, 0.030824, 0.023713, 0.015259, 0.009156, 0.0,
    ],
];

#[derive(Clone, Debug)]
struct EnvelopeState<'a> {
    oscillator: Option<&'a Oscillator>,
    elapsed: f64,
    current: f64,
    start: f64,
    target: f64,
    index: usize,
    curve: u8,
    direct_release: u16,
    mode: u8,
}

impl<'a> EnvelopeState<'a> {
    fn new(oscillator: Option<&'a Oscillator>) -> Self {
        let mut state = Self {
            oscillator,
            elapsed: 0.0,
            current: 0.0,
            start: 0.0,
            target: 0.0,
            index: 0,
            curve: 0,
            direct_release: 0,
            mode: 0,
        };
        if let Some(oscillator) = oscillator.filter(|oscillator| !oscillator.table.is_empty()) {
            state.target = oscillator.table[0].2 as f64 / 32768.0;
            state.curve = oscillator.table[0].0 & 3;
            state.mode = 1;
        } else {
            state.current = 1.0;
            state.mode = 2;
        }
        state
    }

    fn value(&self) -> f64 {
        let oscillator = self.oscillator;
        self.current * oscillator.map_or(1.0, |oscillator| oscillator.scale as f64)
            + oscillator.map_or(0.0, |oscillator| oscillator.offset as f64)
    }

    fn curve_to_target(&mut self, duration: f64) {
        let fraction = if duration != 0.0 {
            16.0 * self.elapsed / duration
        } else {
            16.0
        };
        let index = (fraction as usize).min(15);
        let part = fraction - index as f64;
        let curve = &ENVELOPE_CURVES[(self.curve & 3) as usize];
        let weight = (1.0 - part) * curve[index] + part * curve[index + 1];
        self.current = self.start * weight + self.target * (1.0 - weight);
    }

    fn update(&mut self) {
        if self.mode == 4 {
            let duration = (self.direct_release & 0x3fff) as f64;
            if self.elapsed < duration {
                self.curve_to_target(duration);
            } else {
                self.current = self.target;
                self.mode = 0;
            }
            return;
        }
        if self.mode == 0 || self.mode == 2 {
            return;
        }
        let Some(oscillator) = self.oscillator else {
            self.mode = 0;
            return;
        };
        let table = if self.mode == 1 {
            &oscillator.table
        } else {
            &oscillator.release_table
        };
        let mut guard = 0;
        while self.index < table.len() && self.elapsed >= table[self.index].1 as f64 && guard < 64 {
            guard += 1;
            self.elapsed -= table[self.index].1 as f64;
            self.current = self.target;
            self.index += 1;
            if self.index >= table.len() {
                self.mode = 0;
                return;
            }
            self.start = self.current;
            let (mode, _, value) = table[self.index];
            if mode == 0xf {
                self.mode = 0;
                return;
            }
            if mode == 0xe {
                self.mode = 2;
                return;
            }
            if mode == 0xd {
                self.index = value as usize;
                continue;
            }
            self.curve = mode & 3;
            self.target = value as f64 / 32768.0;
        }
        if self.index < table.len() {
            self.curve_to_target(table[self.index].1 as f64);
        }
    }

    fn increment(&mut self) {
        if self.mode == 0 || self.mode == 2 {
            return;
        }
        let rate = self
            .oscillator
            .map_or(1.0, |oscillator| oscillator.rate as f64);
        self.elapsed += ENVELOPE_INCREMENT * rate;
        self.update();
    }

    fn release(&mut self, direct_release: u16) {
        if direct_release != 0 {
            self.direct_release = direct_release;
            self.elapsed = 0.0;
            self.start = self.current;
            self.target = 0.0;
            self.index = 0;
            self.curve = ((direct_release >> 14) & 3) as u8;
            self.mode = 4;
            self.update();
            return;
        }
        let Some(oscillator) = self.oscillator else {
            self.begin_direct_release(10);
            return;
        };
        if oscillator.release_table.is_empty() {
            self.begin_direct_release(10);
            return;
        }
        if !oscillator.same_release {
            self.elapsed = 0.0;
            self.start = self.current;
            self.target = oscillator.release_table[0].2 as f64 / 32768.0;
            self.index = 0;
            self.curve = oscillator.release_table[0].0 & 3;
        }
        self.mode = 3;
        self.update();
    }

    fn begin_direct_release(&mut self, duration: u16) {
        self.direct_release = duration;
        self.elapsed = 0.0;
        self.start = self.current;
        self.target = 0.0;
        self.curve = 0;
        self.mode = 4;
        self.update();
    }
}

pub fn note_envelope(
    oscillator: Option<&Oscillator>,
    gate: Option<f64>,
    max_seconds: f64,
    direct_release: u16,
) -> Vec<f64> {
    let mut state = EnvelopeState::new(oscillator);
    let release_at = gate
        .filter(|seconds| *seconds != 0.0)
        .map(|seconds| (seconds * ENVELOPE_RATE).round_ties_even() as usize);
    let mut output = Vec::new();
    let count = (max_seconds * ENVELOPE_RATE) as usize;
    for tick in 0..count {
        if release_at == Some(tick) {
            state.release(direct_release);
        }
        output.push(state.value().max(0.0));
        if state.mode == 0 && tick > 0 {
            break;
        }
        state.increment();
    }
    output
}

/// Render one resolved note into the shared f64 mix domain used by the asset
/// builder. The caller adds `samples` to the mix beginning at `offset`.
pub fn render_note(
    note: &ResolvedNote,
    source: &[i16],
    rate: usize,
    max_seconds: f64,
    flavor: u8,
    adsr: bool,
) -> Option<RenderedPart> {
    if source.is_empty() || note.wave.rate == 0 || rate == 0 || max_seconds <= 0.0 {
        return None;
    }
    let step = if flavor & 1 != 0 {
        let ratio = note.ratio as f32;
        (((ratio * note.wave.rate as f32) as f32) / rate as f32) as f64
    } else {
        note.ratio * note.wave.rate as f64 / rate as f64
    };
    if !step.is_finite() || step <= 0.0 {
        return None;
    }
    let adsr_oscillator = note.note.adsr.filter(|_| adsr).map(|adsr| Oscillator {
        target: 0,
        rate: 1.0,
        table: vec![
            (0, adsr[0] as i16, 32767),
            (0, adsr[1] as i16, 32767),
            (0, adsr[2] as i16, adsr[3] as i16),
            (0xe, 0, 0),
        ],
        release_table: Vec::new(),
        same_release: false,
        scale: 1.0,
        offset: 0.0,
    });
    let oscillator = adsr_oscillator.as_ref().or(note.oscillator.as_ref());
    let direct_release = if adsr_oscillator.is_some() {
        note.note.adsr.map_or(0, |adsr| adsr[4] as u16)
    } else {
        note.direct_release
    };
    let mut gain = note_envelope(oscillator, note.note.gate, max_seconds, direct_release);
    if flavor & 2 != 0 && gain.len() > 1 {
        gain.pop();
    }
    if flavor & 8 != 0 {
        gain.iter_mut()
            .for_each(|value| *value = (*value as f32) as f64);
    }
    if gain.is_empty() {
        return None;
    }
    let envelope_seconds = gain.len() as f64 / ENVELOPE_RATE;
    let looped = note.wave.looped && note.wave.loop_end > note.wave.loop_start;
    let out_len = if looped {
        (envelope_seconds.min(max_seconds) * rate as f64) as usize
    } else {
        ((source.len() as f64 / step)
            .min(envelope_seconds * rate as f64)
            .min(max_seconds * rate as f64)) as usize
    };
    if out_len == 0 {
        return None;
    }

    let mut samples = Vec::with_capacity(out_len);
    for index in 0..out_len {
        let mut source_x = index as f64 * step;
        if looped && source_x >= note.wave.loop_end as f64 {
            let loop_len = (note.wave.loop_end - note.wave.loop_start) as f64;
            source_x =
                note.wave.loop_start as f64 + (source_x - note.wave.loop_start as f64) % loop_len;
        }
        let source_index = source_x.floor() as usize;
        let source_sample =
            if source_x > (source.len() - 1) as f64 || source_index >= source.len() - 1 {
                source[source.len() - 1] as f64
            } else {
                let fraction = source_x - source_index as f64;
                (source[source_index + 1] as f64 - source[source_index] as f64)
                    .mul_add(fraction, source[source_index] as f64)
            };
        let time = if flavor & 8 != 0 {
            (index as f32 / rate as f32) as f64
        } else {
            index as f64 / rate as f64
        };
        let gain_index = (time * ENVELOPE_RATE).floor() as usize;
        let envelope = if gain.len() == 1 || time >= (gain.len() - 1) as f64 / ENVELOPE_RATE {
            *gain.last().unwrap()
        } else {
            let index = gain_index.min(gain.len() - 2);
            let start = index as f64 / ENVELOPE_RATE;
            let end = (index + 1) as f64 / ENVELOPE_RATE;
            if start == time {
                gain[index]
            } else {
                let slope = (gain[index + 1] - gain[index]) / (end - start);
                slope.mul_add(time - start, gain[index])
            }
        };
        samples.push(source_sample * envelope * note.volume);
    }
    Some(RenderedPart {
        offset: (note.note.time * rate as f64).round_ties_even().max(0.0) as usize,
        samples,
    })
}

/// Add rendered note parts in sequence order, apply the native peak limiter,
/// optionally trim quiet edges, and convert the result to signed 16-bit PCM.
pub fn mix_to_pcm(parts: &[RenderedPart], max_samples: usize, trim: bool) -> Vec<i16> {
    if parts.is_empty() || max_samples == 0 {
        return Vec::new();
    }
    let total = parts
        .iter()
        .filter_map(|part| part.offset.checked_add(part.samples.len()))
        .max()
        .unwrap_or(0)
        .min(max_samples);
    let mut mix = vec![0.0f64; total];
    for part in parts {
        if part.offset >= total {
            continue;
        }
        let count = part.samples.len().min(total - part.offset);
        for (sample, source) in mix[part.offset..part.offset + count]
            .iter_mut()
            .zip(&part.samples[..count])
        {
            *sample += source;
        }
    }

    let peak = mix
        .iter()
        .fold(0.0f64, |peak, sample| peak.max(sample.abs()));
    if peak > 32767.0 {
        let scale = 32767.0 / peak;
        mix.iter_mut().for_each(|sample| *sample *= scale);
    }
    let (start, end) = if trim {
        let first = mix.iter().position(|sample| sample.abs() > 40.0);
        let last = mix.iter().rposition(|sample| sample.abs() > 40.0);
        match (first, last) {
            (Some(first), Some(last)) => (first.saturating_sub(16), last + 1),
            _ => (0, mix.len()),
        }
    } else {
        (0, mix.len())
    };
    mix[start..end]
        .iter()
        .map(|sample| *sample as i16)
        .collect()
}

#[derive(Clone, Debug)]
pub struct VelocityRegion {
    pub high_velocity: u8,
    pub wave_id: u16,
    pub volume: f32,
    pub pitch: f32,
}

#[derive(Clone, Debug)]
pub struct Oscillator {
    pub target: u8,
    pub rate: f32,
    pub table: Vec<(u8, i16, i16)>,
    pub release_table: Vec<(u8, i16, i16)>,
    pub same_release: bool,
    pub scale: f32,
    pub offset: f32,
}

#[derive(Clone, Debug)]
pub struct Instrument {
    pub percussion: bool,
    pub volume: f32,
    pub pitch: f32,
    pub oscillators: Vec<Oscillator>,
    pub regions: Vec<(u8, Vec<VelocityRegion>)>,
    pub percussion_regions: BTreeMap<u32, (f32, f32, u16, Vec<VelocityRegion>)>,
}

impl Instrument {
    pub fn lookup(&self, key: u8, velocity: u8) -> Option<(&VelocityRegion, f32, f32)> {
        if self.percussion {
            let (volume, pitch, _, regions) = self.percussion_regions.get(&(key as u32))?;
            regions
                .iter()
                .find(|region| velocity <= region.high_velocity)
                .map(|region| (region, *volume, *pitch))
        } else {
            let (_, regions) = self.regions.iter().find(|(high_key, _)| key <= *high_key)?;
            regions
                .iter()
                .find(|region| velocity <= region.high_velocity)
                .map(|region| (region, self.volume, self.pitch))
        }
    }

    pub fn volume_oscillator(&self) -> Option<&Oscillator> {
        self.oscillators
            .iter()
            .find(|oscillator| oscillator.target == 0)
    }

    pub fn percussion_release(&self, key: u8) -> u16 {
        self.percussion_regions
            .get(&((key & 0x7f) as u32))
            .map_or(0, |region| region.2)
    }
}

#[derive(Clone, Debug)]
pub struct Bank {
    pub number: u32,
    pub wave_system: u32,
    pub instruments: BTreeMap<u32, Instrument>,
}

const AFC_COEFFICIENTS: [(i32, i32); 16] = [
    (0, 0),
    (2048, 0),
    (0, 2048),
    (1024, 1024),
    (4096, -2048),
    (3584, -1536),
    (3072, -1024),
    (4608, -2560),
    (4200, -2248),
    (4800, -2300),
    (5120, -3072),
    (2048, -2048),
    (1024, -1024),
    (-1024, 1024),
    (-1024, 0),
    (-2048, 0),
];

pub fn decode_wave(archive_data: &[u8], wave: &Wave) -> Result<Vec<i16>, String> {
    let end = wave
        .offset
        .checked_add(wave.size)
        .ok_or("wave data range overflow")?;
    let raw = archive_data
        .get(wave.offset..end)
        .ok_or_else(|| format!("wave {} exceeds {}", wave.id, wave.archive))?;
    let decoded = match wave.format {
        0 => decode_afc(raw, 4, wave.sample_count)?,
        1 => decode_afc(raw, 2, wave.sample_count)?,
        2 => raw.iter().map(|byte| ((*byte as i8 as i16) << 8)).collect(),
        3 => raw
            .chunks_exact(2)
            .map(|sample| i16::from_be_bytes([sample[0], sample[1]]))
            .collect(),
        format => return Err(format!("unknown JAudio wave format {format}")),
    };
    if wave.sample_count == 0 {
        Ok(decoded)
    } else {
        Ok(decoded.into_iter().take(wave.sample_count).collect())
    }
}

fn decode_afc(raw: &[u8], bits: u8, sample_count: usize) -> Result<Vec<i16>, String> {
    let frame_size = if bits == 4 { 9 } else { 5 };
    let shift = if bits == 4 { 11 } else { 13 };
    let mut output = Vec::new();
    let mut history_1 = 0i64;
    let mut history_2 = 0i64;
    for frame in raw.chunks_exact(frame_size) {
        let header = frame[0];
        let scale = 1i64 << (header >> 4);
        let (coefficient_1, coefficient_2) = AFC_COEFFICIENTS[(header & 0x0f) as usize];
        if bits == 4 {
            for byte in &frame[1..] {
                for nibble in [byte >> 4, byte & 0x0f] {
                    let sample = signed_nibble(nibble) as i64;
                    let value = ((sample * scale) << shift)
                        + coefficient_1 as i64 * history_1
                        + coefficient_2 as i64 * history_2;
                    let decoded = (value >> 11).clamp(i16::MIN as i64, i16::MAX as i64);
                    output.push(decoded as i16);
                    history_2 = history_1;
                    history_1 = decoded;
                }
            }
        } else {
            for byte in &frame[1..] {
                for shift_in_byte in [6, 4, 2, 0] {
                    let packed = (byte >> shift_in_byte) & 3;
                    let sample = (packed as i8 - if packed >= 2 { 4 } else { 0 }) as i64;
                    let value = ((sample * scale) << shift)
                        + coefficient_1 as i64 * history_1
                        + coefficient_2 as i64 * history_2;
                    let decoded = (value >> 11).clamp(i16::MIN as i64, i16::MAX as i64);
                    output.push(decoded as i16);
                    history_2 = history_1;
                    history_1 = decoded;
                }
            }
        }
    }
    if sample_count != 0 {
        output.truncate(sample_count);
    }
    Ok(output)
}

fn signed_nibble(value: u8) -> i8 {
    if value >= 8 {
        value as i8 - 16
    } else {
        value as i8
    }
}

impl AudioArchive {
    pub fn parse(data: &[u8]) -> Result<Self, String> {
        let data = crate::asset_formats::yaz0_decompress(data)
            .map_err(|error| format!("decompress SMR audio archive: {error:?}"))?;
        if data.get(..4) != Some(b"AA_<") {
            return Err("SMR audio archive has no BAA header".into());
        }
        let mut waves = BTreeMap::new();
        let mut banks = BTreeMap::new();
        let mut bsc = Vec::new();
        let mut bst = Vec::new();
        let mut cursor = 4usize;
        while cursor < data.len() {
            let command = data
                .get(cursor..cursor.saturating_add(4))
                .ok_or("truncated BAA command")?;
            if command == b">_AA" {
                break;
            }
            match command {
                b"ws  " => {
                    let number = be_u32(&data, cursor + 4)?;
                    let base = be_u32(&data, cursor + 8)? as usize;
                    waves.insert(number, parse_wave_system(&data, base)?);
                    cursor = cursor.checked_add(16).ok_or("BAA offset overflow")?;
                }
                b"bnk " => {
                    let wave_system = be_u32(&data, cursor + 4)?;
                    let base = be_u32(&data, cursor + 8)?;
                    let bank = parse_bank(&data, base as usize, wave_system).map_err(|error| {
                        format!("parse bank at {base:#x} for wave system {wave_system}: {error}")
                    })?;
                    banks.insert(bank.number, bank);
                    cursor = cursor.checked_add(12).ok_or("BAA offset overflow")?;
                }
                b"bst " | b"bstn" | b"bsc " => {
                    let start = be_u32(&data, cursor + 4)? as usize;
                    let end = be_u32(&data, cursor + 8)? as usize;
                    let bytes = data
                        .get(start..end)
                        .ok_or("BAA sequence table range is invalid")?
                        .to_vec();
                    match command {
                        b"bst " => bst = bytes,
                        b"bsc " => bsc = bytes,
                        _ => {}
                    }
                    cursor = cursor.checked_add(12).ok_or("BAA offset overflow")?;
                }
                b"bms " => cursor = cursor.checked_add(16).ok_or("BAA offset overflow")?,
                b"bl_<" | b"vbnk" => {
                    cursor = cursor.checked_add(12).ok_or("BAA offset overflow")?
                }
                b"bsft" | b"bmsa" | b"dsqb" | b">_bl" | b"sect" => {
                    cursor = cursor
                        .checked_add(if command == b">_bl" { 4 } else { 8 })
                        .ok_or("BAA offset overflow")?;
                }
                _ => return Err(format!("unknown BAA command at {cursor:#x}")),
            }
        }
        if bsc.len() < 4 {
            return Err("SMR audio archive has no BSC sequence table".into());
        }
        let group_count = be_u16(&bsc, 2)? as usize;
        let mut sequence_offsets = Vec::with_capacity(group_count);
        for group in 0..group_count {
            let table = be_u32(&bsc, 8 + 4 * group)? as usize;
            let count = be_u32(&bsc, table)? as usize;
            let mut offsets = Vec::with_capacity(count);
            for index in 0..count {
                offsets.push(be_u32(&bsc, table + 4 + 4 * index)? as usize);
            }
            sequence_offsets.push(offsets);
        }
        Ok(Self {
            waves,
            banks,
            bsc,
            bst,
            sequence_offsets,
        })
    }

    pub fn wave_count(&self) -> usize {
        self.waves.values().map(BTreeMap::len).sum()
    }

    /// Resolve a simulated sequence event to its bank instrument and sample.
    /// Missing banks, programs, key/velocity zones, or wave IDs are silent in
    /// JAudio and therefore return `None` rather than failing conversion.
    pub fn resolve_note(&self, note: &crate::audio_seq::AudioNote) -> Option<ResolvedNote> {
        self.resolve_note_using(note, &f64::exp2)
    }

    /// Resolve a note using the same precomputed pitch ratios as the native
    /// audio recipe builder. Keys are the IEEE-754 bits of the exponent.
    pub fn resolve_note_with_pow2(
        &self,
        note: &crate::audio_seq::AudioNote,
        pow2: &BTreeMap<u64, f64>,
    ) -> Option<ResolvedNote> {
        self.resolve_note_using(note, &|exponent| {
            if exponent == 0.0 {
                1.0
            } else {
                pow2.get(&exponent.to_bits())
                    .copied()
                    .unwrap_or_else(|| f64::exp2(exponent))
            }
        })
    }

    fn resolve_note_using(
        &self,
        note: &crate::audio_seq::AudioNote,
        pow2: &impl Fn(f64) -> f64,
    ) -> Option<ResolvedNote> {
        let bank = self.banks.get(&(note.bank as u32))?;
        let instrument = bank.instruments.get(&(note.program as u32))?;
        let (region, instrument_volume, instrument_pitch) =
            instrument.lookup(note.key, note.velocity)?;
        let wave = self.waves.get(&bank.wave_system)?.get(&region.wave_id)?;

        let mut ratio = region.pitch as f64 * instrument_pitch as f64;
        ratio *= pow2(note.pitch_octaves);
        let (oscillator, direct_release) = if instrument.percussion {
            (None, instrument.percussion_release(note.key))
        } else {
            ratio *= pow2((note.key as f64 - wave.base_key as f64) / 12.0);
            (instrument.volume_oscillator().cloned(), 0)
        };
        let velocity = note.velocity as f64 / 127.0;
        let volume =
            region.volume as f64 * instrument_volume as f64 * note.volume * note.volume * velocity;

        Some(ResolvedNote {
            wave: wave.clone(),
            ratio,
            volume,
            oscillator,
            direct_release,
            note: note.clone(),
        })
    }

    pub fn bgm_sequence(
        &self,
        sound_id: u32,
        jai_sequence_archive: &[u8],
    ) -> Result<Vec<u8>, String> {
        let system = (sound_id >> 24) as usize;
        let group = ((sound_id >> 16) & 0xff) as usize;
        let index = (sound_id & 0xffff) as usize;
        let root = be_u32(&self.bst, 12)? as usize;
        let system_table = be_u32(&self.bst, root + 4 + system * 4)? as usize;
        let group_table = be_u32(&self.bst, system_table + 4 + group * 4)? as usize;
        let item = be_u32(&self.bst, group_table + 8 + index * 4)? as usize & 0x00ff_ffff;
        let resource_id = be_u16(&self.bst, item + 2)?;
        crate::asset_formats::rarc_find_id(jai_sequence_archive, resource_id)
            .map_err(|error| format!("find JaiSeq resource {resource_id}: {error:?}"))
    }
}

fn parse_wave_system(data: &[u8], base: usize) -> Result<BTreeMap<u16, Wave>, String> {
    let winf = base
        .checked_add(be_u32(data, base + 0x10)? as usize)
        .ok_or("wave info offset overflow")?;
    let wbct = base
        .checked_add(be_u32(data, base + 0x14)? as usize)
        .ok_or("wave control offset overflow")?;
    let group_count = be_u32(data, winf + 4)? as usize;
    let control_group_count = be_u32(data, wbct + 8)? as usize;
    let mut waves = BTreeMap::new();
    for group in 0..group_count {
        let group_offset = base
            .checked_add(be_u32(data, winf + 8 + 4 * group)? as usize)
            .ok_or("wave group offset overflow")?;
        let archive = cstr(data, group_offset)?;
        let count = be_u32(data, group_offset + 0x70)? as usize;
        let control = if group < control_group_count {
            let scene = base
                .checked_add(be_u32(data, wbct + 0x0c + 4 * group)? as usize)
                .ok_or("wave scene offset overflow")?;
            Some(
                base.checked_add(be_u32(data, scene + 0x0c)? as usize)
                    .ok_or("wave control group offset overflow")?,
            )
        } else {
            None
        };
        for index in 0..count {
            let wave_offset = base
                .checked_add(be_u32(data, group_offset + 0x74 + 4 * index)? as usize)
                .ok_or("wave entry offset overflow")?;
            let format = *data.get(wave_offset + 1).ok_or("truncated wave format")?;
            let base_key = *data.get(wave_offset + 2).ok_or("truncated wave base key")?;
            let rate = (be_f32(data, wave_offset + 4)? as f64).round() as usize;
            let offset = be_u32(data, wave_offset + 8)? as usize;
            let size = be_u32(data, wave_offset + 12)? as usize;
            let loop_flag = be_u32(data, wave_offset + 16)?;
            let loop_start = be_u32(data, wave_offset + 20)? as usize;
            let loop_end = be_u32(data, wave_offset + 24)? as usize;
            let sample_count = be_u32(data, wave_offset + 28)? as usize;
            let id = match control {
                Some(control) if index < be_u32(data, control + 4)? as usize => {
                    let entry = base
                        .checked_add(be_u32(data, control + 8 + 4 * index)? as usize)
                        .ok_or("wave ID offset overflow")?;
                    be_u16(data, entry + 2)?
                }
                _ => u16::try_from(index).map_err(|_| "wave index exceeds u16")?,
            };
            waves.insert(
                id,
                Wave {
                    archive: archive.clone(),
                    id,
                    format,
                    base_key,
                    rate,
                    offset,
                    size,
                    looped: loop_flag != 0,
                    loop_start,
                    loop_end,
                    sample_count,
                },
            );
        }
    }
    Ok(waves)
}

fn parse_bank(data: &[u8], base: usize, wave_system: u32) -> Result<Bank, String> {
    let size = be_u32(data, base + 4)? as usize;
    let number = be_u32(data, base + 8)?;
    let mut bank = Bank {
        number,
        wave_system,
        instruments: BTreeMap::new(),
    };
    if be_u32(data, base + 0x0c)? != 1 {
        return Ok(bank);
    }
    let end = base.checked_add(size).ok_or("bank size overflow")?;
    let mut cursor = base + 0x20;
    let (mut list, mut envelope_table, mut oscillator_table) = (None, None, None);
    while cursor < end {
        let command = data
            .get(cursor..cursor + 4)
            .ok_or("truncated bank chunk header")?;
        let chunk_size = be_u32(data, cursor + 4)? as usize;
        match command {
            b"LIST" => list = Some(cursor),
            b"ENVT" => envelope_table = Some(cursor + 8),
            b"OSCT" => oscillator_table = Some(cursor + 8),
            _ => {}
        }
        if command == b"\0\0\0\0" && chunk_size == 0 {
            break;
        }
        cursor = cursor
            .checked_add(0x0b)
            .and_then(|offset| offset.checked_add(chunk_size))
            .ok_or("bank chunk size overflow")?
            & !3;
    }
    let Some(list) = list else {
        return Ok(bank);
    };
    let mut oscillators = Vec::new();
    if let (Some(envelope_table), Some(oscillator_table)) = (envelope_table, oscillator_table) {
        let count = be_u32(data, oscillator_table)? as usize;
        for index in 0..count {
            let record = oscillator_table
                .checked_add(4 + 0x1c * index)
                .ok_or("oscillator table offset overflow")?;
            let target = *data.get(record + 4).ok_or("truncated oscillator target")?;
            let rate = be_f32(data, record + 8)?;
            let table_offset = be_u32(data, record + 0x0c)? as usize;
            let release_offset = be_u32(data, record + 0x10)? as usize;
            let scale = be_f32(data, record + 0x14)?;
            let offset = be_f32(data, record + 0x18)?;
            let table = read_envelope(data, envelope_table + table_offset).map_err(|error| {
                format!(
                    "oscillator {index} envelope at {:#x}: {error}",
                    envelope_table + table_offset
                )
            })?;
            let same_release = release_offset == table_offset;
            let release_table = if same_release {
                table.clone()
            } else {
                read_envelope(data, envelope_table + release_offset).map_err(|error| {
                    format!(
                        "oscillator {index} release envelope at {:#x}: {error}",
                        envelope_table + release_offset
                    )
                })?
            };
            oscillators.push(Oscillator {
                target,
                rate,
                table,
                release_table,
                same_release,
                scale,
                offset,
            });
        }
    }
    let program_count = be_u32(data, list + 8)? as usize;
    for program in 0..program_count {
        let offset = be_u32(data, list + 0x0c + 4 * program)? as usize;
        if offset == 0 {
            continue;
        }
        let mut cursor = base
            .checked_add(offset)
            .ok_or("instrument offset overflow")?;
        let magic = data
            .get(cursor..cursor + 4)
            .ok_or("truncated instrument header")?;
        cursor += 4;
        if magic == b"Inst" {
            let oscillator_count = be_u32(data, cursor)? as usize;
            let mut instrument_oscillators = Vec::new();
            for index in 0..oscillator_count {
                let oscillator_index = be_u32(data, cursor + 4 + 4 * index)? as usize;
                if let Some(oscillator) = oscillators.get(oscillator_index) {
                    instrument_oscillators.push(oscillator.clone());
                }
            }
            cursor = cursor
                .checked_add(4 + 4 * oscillator_count)
                .ok_or("instrument oscillator list overflow")?;
            let extra_count = be_u32(data, cursor)? as usize;
            cursor = cursor
                .checked_add(4 + 4 * extra_count)
                .ok_or("instrument extra list overflow")?;
            let region_count = be_u32(data, cursor)? as usize;
            cursor += 4;
            let mut regions = Vec::with_capacity(region_count);
            for _ in 0..region_count {
                let high_key = *data.get(cursor).ok_or("truncated instrument key region")?;
                let velocity_count = be_u32(data, cursor + 4)? as usize;
                let velocities = read_velocities(data, cursor + 8, velocity_count)?;
                regions.push((high_key, velocities));
                cursor = cursor
                    .checked_add(8 + 16 * velocity_count)
                    .ok_or("instrument velocity region overflow")?;
            }
            bank.instruments.insert(
                program as u32,
                Instrument {
                    percussion: false,
                    volume: be_f32(data, cursor)?,
                    pitch: be_f32(data, cursor + 4)?,
                    oscillators: instrument_oscillators,
                    regions,
                    percussion_regions: BTreeMap::new(),
                },
            );
        } else if magic == b"Perc" {
            let count = be_u32(data, cursor)? as usize;
            let mut percussion_regions = BTreeMap::new();
            for key in 0..count {
                let relative = be_u32(data, cursor + 4 + 4 * key)? as usize;
                if relative == 0 {
                    continue;
                }
                let mut record = base
                    .checked_add(relative)
                    .ok_or("percussion entry offset overflow")?;
                let volume = be_f32(data, record + 4)?;
                let pitch = be_f32(data, record + 8)?;
                let direct_release = be_u16(data, record + 0x0e)?;
                record += 16;
                let extra_count = be_u32(data, record)? as usize;
                record = record
                    .checked_add(4 + 4 * extra_count)
                    .ok_or("percussion extra list overflow")?;
                let velocity_count = be_u32(data, record)? as usize;
                let velocities = read_velocities(data, record + 4, velocity_count)?;
                percussion_regions.insert(key as u32, (volume, pitch, direct_release, velocities));
            }
            bank.instruments.insert(
                program as u32,
                Instrument {
                    percussion: true,
                    volume: 1.0,
                    pitch: 1.0,
                    oscillators: Vec::new(),
                    regions: Vec::new(),
                    percussion_regions,
                },
            );
        }
    }
    Ok(bank)
}

fn read_velocities(
    data: &[u8],
    mut offset: usize,
    count: usize,
) -> Result<Vec<VelocityRegion>, String> {
    let mut regions = Vec::with_capacity(count);
    for _ in 0..count {
        regions.push(VelocityRegion {
            high_velocity: *data.get(offset).ok_or("truncated velocity region")?,
            wave_id: be_u32(data, offset + 4)? as u16,
            volume: be_f32(data, offset + 8)?,
            pitch: be_f32(data, offset + 12)?,
        });
        offset = offset.checked_add(16).ok_or("velocity offset overflow")?;
    }
    Ok(regions)
}

fn read_envelope(data: &[u8], mut offset: usize) -> Result<Vec<(u8, i16, i16)>, String> {
    let mut points = Vec::new();
    for _ in 0..64 {
        let mode = be_u16(data, offset)?;
        let time = be_u16(data, offset + 2)? as i16;
        let value = be_u16(data, offset + 4)? as i16;
        points.push((mode as u8, time, value));
        offset = offset.checked_add(6).ok_or("envelope offset overflow")?;
        if (0x0d..=0x0f).contains(&mode) {
            break;
        }
    }
    Ok(points)
}

fn cstr(data: &[u8], offset: usize) -> Result<String, String> {
    let bytes = data.get(offset..).ok_or("invalid BAA string offset")?;
    let length = bytes
        .iter()
        .position(|byte| *byte == 0)
        .ok_or("unterminated BAA string")?;
    std::str::from_utf8(&bytes[..length])
        .map(str::to_owned)
        .map_err(|_| "BAA wave archive name is not UTF-8".into())
}

fn be_u16(data: &[u8], offset: usize) -> Result<u16, String> {
    let bytes = data
        .get(offset..offset.checked_add(2).ok_or("BAA offset overflow")?)
        .ok_or_else(|| {
            format!(
                "truncated big-endian u16 at {offset:#x} ({} bytes)",
                data.len()
            )
        })?;
    Ok(u16::from_be_bytes([bytes[0], bytes[1]]))
}

fn be_u32(data: &[u8], offset: usize) -> Result<u32, String> {
    let bytes = data
        .get(offset..offset.checked_add(4).ok_or("BAA offset overflow")?)
        .ok_or_else(|| {
            format!(
                "truncated big-endian u32 at {offset:#x} ({} bytes)",
                data.len()
            )
        })?;
    Ok(u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
}

fn be_f32(data: &[u8], offset: usize) -> Result<f32, String> {
    Ok(f32::from_bits(be_u32(data, offset)?))
}

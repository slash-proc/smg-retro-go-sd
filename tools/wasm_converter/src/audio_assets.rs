//! ISO-derived stream and JAudio conversion used by the asset compiler.

use std::collections::BTreeMap;

const PHASES: usize = 16_384;
const OUTPUT_RATE: usize = 18_157;
const PI: f64 = 3.14159265358979323846;

#[derive(Clone)]
struct Operation {
    kind: String,
    args: Vec<serde_json::Value>,
}

#[derive(Clone)]
struct AstRecipe {
    name: String,
    pair: Option<usize>,
    track_ops: Vec<Operation>,
    post_ops: Vec<Operation>,
    final_ops: Vec<Operation>,
    residuals: Vec<(usize, i32)>,
    total: usize,
}

#[derive(Clone)]
struct JAudioRecipe {
    name: String,
    recipe: serde_json::Value,
    residuals: Vec<(usize, i32)>,
    total: usize,
}

pub struct AudioAstConverter {
    recipes_by_source: BTreeMap<String, Vec<AstRecipe>>,
    jaudio_recipes: Vec<JAudioRecipe>,
    jaudio_source_paths: Vec<String>,
    jaudio_sources: BTreeMap<String, Vec<u8>>,
    jaudio_generated: bool,
    pow2: BTreeMap<u64, f64>,
    resamplers: BTreeMap<usize, ResampleBank>,
    seed_state: u128,
    seed_increment: u128,
}

impl AudioAstConverter {
    pub fn new() -> Result<Self, String> {
        let recipes = crate::asset_packs::get("audio/audio.pak", "recipes.json")
            .ok_or("audio recipe pack is missing recipes.json")?;
        let residual_blob = crate::asset_packs::get("audio/audio.pak", "residuals.bin")
            .ok_or("audio recipe pack is missing residuals.bin")?;
        let recipe_text =
            std::str::from_utf8(recipes).map_err(|_| "audio recipes are not UTF-8")?;
        let root: serde_json::Value = serde_json::from_str(recipe_text)
            .map_err(|error| format!("parse audio recipes: {error}"))?;
        let (seed_state, seed_increment) = parse_pcg_seeds(recipe_text)?;
        let pow2 = root["pow2"]
            .as_object()
            .ok_or("audio recipe power table is missing")?
            .iter()
            .map(|(exponent, value)| {
                let exponent = parse_hex_float(&serde_json::Value::String(exponent.clone()))?;
                let value = parse_hex_float(value)?;
                Ok((exponent.to_bits(), value))
            })
            .collect::<Result<BTreeMap<_, _>, String>>()?;
        let order = root["order"]
            .as_array()
            .ok_or("audio recipe order is missing")?;
        let recipe_map = root["recipes"]
            .as_object()
            .ok_or("audio recipe map is missing")?;
        let mut residuals = BTreeMap::new();
        let mut cursor = VarintReader::new(residual_blob);
        for value in order {
            let name = value.as_str().ok_or("invalid audio recipe name")?;
            let count = cursor.read_usize()?;
            let total = cursor.read_usize()?;
            let mut position = 0usize;
            let mut corrections = Vec::with_capacity(count);
            for _ in 0..count {
                position = position
                    .checked_add(cursor.read_usize()?)
                    .ok_or("audio residual position overflow")?;
                let correction = cursor.read_signed()?;
                corrections.push((position, correction));
            }
            residuals.insert(name.to_owned(), (corrections, total));
        }

        let mut recipes_by_source = BTreeMap::<String, Vec<AstRecipe>>::new();
        let mut jaudio_recipes = Vec::new();
        for value in order {
            let name = value.as_str().ok_or("invalid audio recipe name")?;
            let recipe = recipe_map
                .get(name)
                .ok_or("audio recipe order references missing recipe")?;
            let tracks = recipe["tracks"]
                .as_array()
                .ok_or("audio recipe has no tracks")?;
            let has_jaudio = tracks
                .iter()
                .any(|track| matches!(track["src"]["k"].as_str(), Some("se" | "bgm")));
            if has_jaudio {
                let (corrections, total) = residuals
                    .remove(name)
                    .ok_or("JAudio recipe has no residual record")?;
                jaudio_recipes.push(JAudioRecipe {
                    name: name.to_owned(),
                    recipe: recipe.clone(),
                    residuals: corrections,
                    total,
                });
                continue;
            }
            if tracks.len() != 1 {
                continue;
            }
            let track = &tracks[0];
            let source = &track["src"];
            if source["k"].as_str() != Some("ast") {
                continue;
            }
            let source_path = source["file"]
                .as_str()
                .ok_or("AST audio recipe has no source file")?
                .to_owned();
            let (corrections, total) = residuals
                .remove(name)
                .ok_or("AST audio recipe has no residual record")?;
            let ast = AstRecipe {
                name: name.to_owned(),
                pair: source["pair"].as_u64().map(|value| value as usize),
                track_ops: parse_operations(&track["ops"])?,
                post_ops: parse_operations(&recipe["post"])?,
                final_ops: parse_operations(&recipe["final"])?,
                residuals: corrections,
                total,
            };
            recipes_by_source.entry(source_path).or_default().push(ast);
        }
        if recipes_by_source.is_empty() {
            return Err("audio recipe pack contains no AST stream recipes".into());
        }
        let mut jaudio_source_paths = include_str!("../../../asset_construction/audio/sources.txt")
            .lines()
            .filter_map(|line| {
                let mut fields = line.split_whitespace();
                let _hash = fields.next()?;
                let path = fields.next()?;
                (path == "AudioRes/SMR.szs"
                    || path == "AudioRes/Seqs/JaiSeq.arc"
                    || (path.starts_with("AudioRes/Waves/") && path.ends_with(".aw")))
                .then(|| path.to_owned())
            })
            .collect::<Vec<_>>();
        jaudio_source_paths.sort();
        jaudio_source_paths.dedup();
        if jaudio_source_paths.is_empty() || jaudio_recipes.is_empty() {
            return Err("audio recipe pack has no JAudio sources or recipes".into());
        }
        Ok(Self {
            recipes_by_source,
            jaudio_recipes,
            jaudio_source_paths,
            jaudio_sources: BTreeMap::new(),
            jaudio_generated: false,
            pow2,
            resamplers: BTreeMap::new(),
            seed_state,
            seed_increment,
        })
    }

    pub fn source_paths(&self) -> Vec<String> {
        let mut paths = self.recipes_by_source.keys().cloned().collect::<Vec<_>>();
        paths.extend(self.jaudio_source_paths.iter().cloned());
        paths.sort();
        paths.dedup();
        paths
    }

    pub fn process_archive(
        &mut self,
        path: &str,
        data: Vec<u8>,
    ) -> Result<Vec<(String, Vec<u8>)>, String> {
        let Some(recipes) = self.recipes_by_source.get(path).cloned() else {
            if !self.jaudio_source_paths.iter().any(|source| source == path) {
                return Err(format!("no audio recipes use {path}"));
            }
            self.jaudio_sources.insert(path.to_owned(), data);
            if !self.jaudio_generated
                && self
                    .jaudio_source_paths
                    .iter()
                    .all(|source| self.jaudio_sources.contains_key(source))
            {
                let outputs = self.generate_jaudio()?;
                self.jaudio_generated = true;
                return Ok(outputs);
            }
            return Ok(Vec::new());
        };
        let (channels, sample_rate) = decode_ast(&data)?;
        drop(data);
        let mut outputs = Vec::with_capacity(recipes.len());
        for recipe in recipes {
            let selected: Vec<&[i16]> = if let Some(pair) = recipe.pair {
                let first = pair.checked_mul(2).ok_or("AST channel pair overflow")?;
                if first + 2 > channels.len() {
                    return Err(format!(
                        "{} requests absent AST channel pair {pair}",
                        recipe.name
                    ));
                }
                vec![&channels[first], &channels[first + 1]]
            } else {
                channels.iter().map(Vec::as_slice).collect()
            };
            let mut samples = self.resample(&selected, sample_rate)?;
            apply_float_ops(&mut samples, &recipe.track_ops)?;
            apply_float_ops(&mut samples, &recipe.post_ops)?;
            let bytes = quantise(
                &samples,
                &recipe.final_ops,
                &recipe.residuals,
                recipe.total,
                self.seed_state,
                self.seed_increment,
            )?;
            outputs.push((format!("assets/audio/{}.pcm8", recipe.name), bytes));
        }
        Ok(outputs)
    }

    fn generate_jaudio(&mut self) -> Result<Vec<(String, Vec<u8>)>, String> {
        let smr = self
            .jaudio_sources
            .remove("AudioRes/SMR.szs")
            .ok_or("missing captured SMR audio archive")?;
        let jai_sequence = self
            .jaudio_sources
            .remove("AudioRes/Seqs/JaiSeq.arc")
            .ok_or("missing captured JaiSeq archive")?;
        let archive = crate::audio_jaudio::AudioArchive::parse(&smr)?;
        let wave_archives = core::mem::take(&mut self.jaudio_sources)
            .into_iter()
            .filter_map(|(path, data)| {
                path.strip_prefix("AudioRes/Waves/")
                    .map(|name| (name.to_owned(), data))
            })
            .collect::<BTreeMap<_, _>>();
        let mut wave_cache = BTreeMap::<(String, usize, usize), Vec<i16>>::new();
        let recipes = self.jaudio_recipes.clone();
        let mut outputs = Vec::with_capacity(recipes.len());
        for recipe in recipes {
            let samples = self
                .build_jaudio_recipe(
                    &recipe,
                    &archive,
                    &jai_sequence,
                    &wave_archives,
                    &mut wave_cache,
                )
                .map_err(|error| format!("{}: {error}", recipe.name))?;
            outputs.push((format!("assets/audio/{}.pcm8", recipe.name), samples));
        }
        Ok(outputs)
    }

    fn build_jaudio_recipe(
        &mut self,
        recipe: &JAudioRecipe,
        archive: &crate::audio_jaudio::AudioArchive,
        jai_sequence: &[u8],
        wave_archives: &BTreeMap<String, Vec<u8>>,
        wave_cache: &mut BTreeMap<(String, usize, usize), Vec<i16>>,
    ) -> Result<Vec<u8>, String> {
        let tracks = recipe.recipe["tracks"]
            .as_array()
            .ok_or("JAudio recipe has no tracks")?;
        let mut rendered_tracks = Vec::with_capacity(tracks.len());
        for track in tracks {
            let source = &track["src"];
            let samples =
                self.build_jaudio_source(source, archive, jai_sequence, wave_archives, wave_cache)?;
            let mut samples = samples;
            apply_float_ops(&mut samples, &parse_operations(&track["ops"])?)?;
            let offset = track["at"].as_u64().unwrap_or(0) as usize;
            rendered_tracks.push((offset, samples));
        }
        let mut mix = if rendered_tracks.len() == 1 && rendered_tracks[0].0 == 0 {
            rendered_tracks.remove(0).1
        } else {
            let total = rendered_tracks
                .iter()
                .filter_map(|(offset, samples)| offset.checked_add(samples.len()))
                .max()
                .unwrap_or(0);
            let mut mix = vec![0.0f64; total];
            for (offset, samples) in rendered_tracks {
                if offset >= total {
                    continue;
                }
                let count = samples.len().min(total - offset);
                for (destination, sample) in mix[offset..offset + count]
                    .iter_mut()
                    .zip(&samples[..count])
                {
                    *destination += sample;
                }
            }
            mix
        };
        apply_float_ops(&mut mix, &parse_operations(&recipe.recipe["post"])?)?;
        quantise(
            &mix,
            &parse_operations(&recipe.recipe["final"])?,
            &recipe.residuals,
            recipe.total,
            self.seed_state,
            self.seed_increment,
        )
    }

    fn build_jaudio_source(
        &mut self,
        source: &serde_json::Value,
        archive: &crate::audio_jaudio::AudioArchive,
        jai_sequence: &[u8],
        wave_archives: &BTreeMap<String, Vec<u8>>,
        wave_cache: &mut BTreeMap<(String, usize, usize), Vec<i16>>,
    ) -> Result<Vec<f64>, String> {
        match source["k"].as_str() {
            Some("se") => {
                self.build_se_source(source, archive, jai_sequence, wave_archives, wave_cache)
            }
            Some("bgm") => {
                self.build_bgm_source(source, archive, jai_sequence, wave_archives, wave_cache)
            }
            other => Err(format!("unsupported JAudio source kind {other:?}")),
        }
    }

    fn build_se_source(
        &mut self,
        source: &serde_json::Value,
        archive: &crate::audio_jaudio::AudioArchive,
        jai_sequence: &[u8],
        wave_archives: &BTreeMap<String, Vec<u8>>,
        wave_cache: &mut BTreeMap<(String, usize, usize), Vec<i16>>,
    ) -> Result<Vec<f64>, String> {
        let maximum = parse_hex_float(&source["max_len"])?;
        let flavor = source["fl"].as_u64().unwrap_or(0) as u8;
        let mut resolved = Vec::new();
        if let Some(parts) = source["notes"].as_array() {
            for part in parts {
                resolved.extend(self.resolve_se_part(part, archive, jai_sequence)?);
            }
        }
        let rate = source["rate"]
            .as_u64()
            .map(|rate| rate as usize)
            .unwrap_or_else(|| {
                resolved
                    .iter()
                    .map(|note| note.wave.rate)
                    .max()
                    .unwrap_or(32_000)
            });
        let mut rendered = Vec::with_capacity(resolved.len());
        for note in &resolved {
            let key = (note.wave.archive.clone(), note.wave.offset, note.wave.size);
            if !wave_cache.contains_key(&key) {
                let bytes = wave_archives
                    .get(&note.wave.archive)
                    .ok_or_else(|| format!("missing JAudio wave archive {}", note.wave.archive))?;
                wave_cache.insert(
                    key.clone(),
                    crate::audio_jaudio::decode_wave(bytes, &note.wave)?,
                );
            }
            let pcm = wave_cache.get(&key).expect("decoded wave is cached");
            if let Some(part) =
                crate::audio_jaudio::render_note(note, pcm, rate, maximum, flavor, flavor & 4 != 0)
            {
                rendered.push(part);
            }
        }
        let pcm = crate::audio_jaudio::mix_to_pcm(
            &rendered,
            (maximum * rate as f64) as usize,
            source["trim"].as_u64().unwrap_or(1) != 0,
        );
        let pcm = if let Some(norm) = source.get("norm") {
            let norm = parse_hex_float(norm)?;
            let peak = pcm
                .iter()
                .fold(0i32, |peak, sample| peak.max((*sample as i32).abs()))
                as f64;
            if peak > 0.0 {
                let scale = norm * 32767.0 / peak;
                pcm.into_iter()
                    .map(|sample| ((sample as f64 * scale).clamp(-32768.0, 32767.0)) as i16)
                    .collect()
            } else {
                pcm
            }
        } else {
            pcm
        };
        if source["direct"].as_u64().unwrap_or(0) != 0 {
            return Ok(pcm
                .into_iter()
                .map(|sample| sample as f64 / 32768.0)
                .collect());
        }
        self.resample(&[&pcm], rate)
    }

    fn build_bgm_source(
        &mut self,
        source: &serde_json::Value,
        archive: &crate::audio_jaudio::AudioArchive,
        jai_sequence: &[u8],
        wave_archives: &BTreeMap<String, Vec<u8>>,
        wave_cache: &mut BTreeMap<(String, usize, usize), Vec<i16>>,
    ) -> Result<Vec<f64>, String> {
        let seconds = parse_hex_float(&source["sec"])?;
        let sid = source["sid"].as_u64().ok_or("BGM source has no sound ID")? as u32;
        let sequence = archive.bgm_sequence(sid, jai_sequence)?;
        let notes = crate::audio_seq::simulate(&sequence, 0, None, None, seconds, 5_000_000);
        let resolved = notes
            .iter()
            .filter_map(|note| archive.resolve_note_with_pow2(note, &self.pow2))
            .collect::<Vec<_>>();
        let rate = source["rate"]
            .as_u64()
            .map(|rate| rate as usize)
            .unwrap_or_else(|| {
                resolved
                    .iter()
                    .map(|note| note.wave.rate)
                    .max()
                    .unwrap_or(32_000)
            });
        let mut rendered = Vec::with_capacity(resolved.len());
        for note in &resolved {
            let key = (note.wave.archive.clone(), note.wave.offset, note.wave.size);
            if !wave_cache.contains_key(&key) {
                let bytes = wave_archives
                    .get(&note.wave.archive)
                    .ok_or_else(|| format!("missing JAudio wave archive {}", note.wave.archive))?;
                wave_cache.insert(
                    key.clone(),
                    crate::audio_jaudio::decode_wave(bytes, &note.wave)?,
                );
            }
            let pcm = wave_cache.get(&key).expect("decoded wave is cached");
            if let Some(part) = crate::audio_jaudio::render_note(note, pcm, rate, seconds, 0, false)
            {
                rendered.push(part);
            }
        }
        let mut pcm =
            crate::audio_jaudio::mix_to_pcm(&rendered, (seconds * rate as f64) as usize, false);
        let output_samples = (seconds * rate as f64) as usize;
        pcm.resize(output_samples, 0);
        pcm.truncate(output_samples);
        let peak = pcm
            .iter()
            .fold(0i32, |peak, sample| peak.max((*sample as i32).abs())) as f64;
        let scale = 32767.0 / peak.max(1.0);
        for sample in &mut pcm {
            *sample = (*sample as f64 * scale).round_ties_even() as i16;
        }
        self.resample(&[&pcm], rate)
    }

    fn resolve_se_part(
        &self,
        part: &serde_json::Value,
        archive: &crate::audio_jaudio::AudioArchive,
        jai_sequence: &[u8],
    ) -> Result<Vec<crate::audio_jaudio::ResolvedNote>, String> {
        let sid = part["sid"].as_u64().ok_or("SFX part has no sound ID")? as u32;
        let system = (sid >> 24) as usize;
        let bgm_sequence;
        let (data, start, max_time) = if system == 1 {
            bgm_sequence = archive.bgm_sequence(sid, jai_sequence)?;
            (&bgm_sequence[..], 0, 20.0)
        } else {
            let group = ((sid >> 16) & 0xff) as usize;
            let index = (sid & 0xffff) as usize;
            let start = *archive
                .sequence_offsets
                .get(group)
                .and_then(|sequences| sequences.get(index))
                .ok_or_else(|| format!("SFX sequence {sid:#010x} is out of range"))?;
            (&archive.bsc[..], start, 6.0)
        };
        let ports = part["ports"].as_object().map(|ports| {
            ports
                .iter()
                .filter_map(|(key, value)| Some((key.parse::<u8>().ok()?, value.as_u64()? as u16)))
                .collect::<BTreeMap<u8, u16>>()
        });
        let want = part["v"].as_u64().unwrap_or(0) as usize;
        let mut variants: Vec<Vec<crate::audio_jaudio::ResolvedNote>> = Vec::new();
        let mut signatures = std::collections::BTreeSet::new();
        for seed in std::iter::once(None).chain((1..=48).map(Some)) {
            let notes =
                crate::audio_seq::simulate(data, start, ports.as_ref(), seed, max_time, 20_000);
            let resolved = notes
                .iter()
                .filter_map(|note| archive.resolve_note_with_pow2(note, &self.pow2))
                .collect::<Vec<_>>();
            let signature = resolved
                .iter()
                .map(|note| {
                    (
                        note.wave.archive.clone(),
                        note.wave.id,
                        (note.note.time * 1000.0).round_ties_even() as i64,
                    )
                })
                .collect::<Vec<_>>();
            if signatures.insert(signature) {
                variants.push(resolved);
                if variants.len() > want {
                    break;
                }
            }
        }
        let mut resolved = variants
            .get(want.min(variants.len().saturating_sub(1)))
            .cloned()
            .unwrap_or_default();
        if let Some(hold) = part.get("hold") {
            let hold = parse_hex_float(hold)?;
            resolved.retain_mut(|note| {
                if note.note.time >= hold {
                    return false;
                }
                let left = hold - note.note.time;
                if note.note.gate.is_none_or(|gate| gate > left) {
                    note.note.gate = Some(left);
                }
                true
            });
        }
        if part["vel"].as_u64().unwrap_or(0) != 0 {
            resolved.retain(|note| note.note.velocity > 1);
        }
        if part["vel0"].as_u64().unwrap_or(0) != 0 {
            resolved.retain(|note| note.note.velocity > 0);
        }
        if let Some(shift) = part.get("shift") {
            let shift = parse_hex_float(shift)?;
            for note in &mut resolved {
                note.note.time -= shift;
            }
        }
        Ok(resolved)
    }

    fn resample(&mut self, channels: &[&[i16]], rate: usize) -> Result<Vec<f64>, String> {
        if channels.is_empty()
            || channels
                .iter()
                .any(|channel| channel.len() != channels[0].len())
        {
            return Err("AST channels have inconsistent sample counts".into());
        }
        let bank = self
            .resamplers
            .entry(rate)
            .or_insert_with(|| ResampleBank::new(rate));
        let count = bank.output_count(channels[0].len());
        let mut converted = Vec::with_capacity(channels.len());
        for channel in channels {
            converted.push(bank.run_channel(channel, count)?);
        }
        let mut mono = vec![0.0f32; count];
        if channels.len() == 1 {
            mono.copy_from_slice(&converted[0]);
        } else if channels.len() == 2 {
            let h = 0.70710678118654752440f32;
            for i in 0..count {
                mono[i] = converted[0][i].mul_add(h, converted[1][i] * h);
            }
        } else {
            let h = 0.70710678118654752440f32;
            let coefficients = [h, h, 1.0f32, 0.5f32];
            for i in 0..count {
                let mut value = 0.0f32;
                for channel in 0..channels.len() {
                    let coefficient = coefficients.get(channel).copied().unwrap_or(0.0);
                    value = converted[channel][i].mul_add(coefficient, value);
                }
                mono[i] = value;
            }
        }
        Ok(mono.into_iter().map(f64::from).collect())
    }
}

fn parse_operations(value: &serde_json::Value) -> Result<Vec<Operation>, String> {
    let Some(operations) = value.as_array() else {
        return Ok(Vec::new());
    };
    operations
        .iter()
        .map(|operation| {
            let fields = operation.as_array().ok_or("invalid audio operation")?;
            let kind = fields
                .first()
                .and_then(serde_json::Value::as_str)
                .ok_or("audio operation has no kind")?
                .to_owned();
            Ok(Operation {
                kind,
                args: fields.iter().skip(1).cloned().collect(),
            })
        })
        .collect()
}

pub fn parse_hex_float(value: &serde_json::Value) -> Result<f64, String> {
    let text = value.as_str().ok_or("audio float is not a string")?;
    if let Ok(value) = text.parse::<f64>() {
        return Ok(value);
    }
    let (mantissa, exponent) = text
        .split_once('p')
        .ok_or_else(|| format!("invalid audio float {text}"))?;
    let exponent: i32 = exponent
        .parse()
        .map_err(|_| format!("invalid audio float exponent {text}"))?;
    let (sign, mantissa) = if let Some(mantissa) = mantissa.strip_prefix('-') {
        (-1.0, mantissa)
    } else if let Some(mantissa) = mantissa.strip_prefix('+') {
        (1.0, mantissa)
    } else {
        (1.0, mantissa)
    };
    let mantissa = mantissa
        .strip_prefix("0x")
        .ok_or_else(|| format!("invalid audio float mantissa {text}"))?;
    let (whole, fraction) = mantissa.split_once('.').unwrap_or((mantissa, ""));
    let mut value = 0.0f64;
    for byte in whole.bytes() {
        value = value * 16.0 + hex_digit(byte)? as f64;
    }
    let mut place = 1.0 / 16.0;
    for byte in fraction.bytes() {
        value += hex_digit(byte)? as f64 * place;
        place /= 16.0;
    }
    Ok(sign * value * 2.0f64.powi(exponent))
}

fn hex_digit(byte: u8) -> Result<u8, String> {
    match byte {
        b'0'..=b'9' => Ok(byte - b'0'),
        b'a'..=b'f' => Ok(byte - b'a' + 10),
        b'A'..=b'F' => Ok(byte - b'A' + 10),
        _ => Err("invalid hexadecimal float digit".into()),
    }
}

fn parse_pcg_seeds(text: &str) -> Result<(u128, u128), String> {
    let marker = text
        .find("\"pcg\"")
        .ok_or("audio recipes have no PCG seed")?;
    let after = &text[marker + 5..];
    let open = after.find('[').ok_or("invalid audio PCG seed array")?;
    let values = after[open + 1..]
        .split_once(']')
        .ok_or("unterminated audio PCG seed array")?
        .0;
    let mut numbers = values.split(',').map(str::trim);
    let first = numbers
        .next()
        .ok_or("missing audio PCG state")?
        .parse()
        .map_err(|_| "invalid audio PCG state")?;
    let second = numbers
        .next()
        .ok_or("missing audio PCG increment")?
        .parse()
        .map_err(|_| "invalid audio PCG increment")?;
    Ok((first, second))
}

struct VarintReader<'a> {
    data: &'a [u8],
    offset: usize,
}

impl<'a> VarintReader<'a> {
    fn new(data: &'a [u8]) -> Self {
        Self { data, offset: 0 }
    }

    fn read_u64(&mut self) -> Result<u64, String> {
        let mut value = 0u64;
        let mut shift = 0;
        loop {
            let byte = *self
                .data
                .get(self.offset)
                .ok_or("truncated audio residuals")?;
            self.offset += 1;
            if shift >= 64 && byte & 0x7f != 0 {
                return Err("audio residual varint overflow".into());
            }
            value |= ((byte & 0x7f) as u64).checked_shl(shift).unwrap_or(0);
            if byte < 0x80 {
                return Ok(value);
            }
            shift += 7;
            if shift > 70 {
                return Err("audio residual varint too long".into());
            }
        }
    }

    fn read_usize(&mut self) -> Result<usize, String> {
        usize::try_from(self.read_u64()?).map_err(|_| "audio residual exceeds address space".into())
    }

    fn read_signed(&mut self) -> Result<i32, String> {
        let value = self.read_u64()?;
        let decoded = ((value >> 1) as i64) ^ (-((value & 1) as i64));
        i32::try_from(decoded).map_err(|_| "audio residual correction exceeds i32".into())
    }
}

fn decode_ast(data: &[u8]) -> Result<(Vec<Vec<i16>>, usize), String> {
    if data.len() < 0x40 {
        return Err("AST header is truncated".into());
    }
    let format = be_u16(data, 8)?;
    let channels_count = be_u16(data, 12)? as usize;
    let rate = be_u32(data, 16)? as usize;
    let total = be_u32(data, 20)? as usize;
    if format != 1 || channels_count == 0 || channels_count > 8 || rate == 0 {
        return Err("unsupported AST format, channel count, or sample rate".into());
    }
    let mut channels = vec![Vec::<i16>::new(); channels_count];
    let mut cursor = 0x40usize;
    while data.get(cursor..cursor + 4) == Some(b"BLCK") {
        let block_size = be_u32(data, cursor + 4)? as usize;
        let block_start = cursor
            .checked_add(0x20)
            .ok_or("AST block offset overflow")?;
        let end = block_start
            .checked_add(
                block_size
                    .checked_mul(channels_count)
                    .ok_or("AST block size overflow")?,
            )
            .ok_or("AST block end overflow")?;
        if block_size & 1 != 0 {
            return Err("AST block payload is misaligned".into());
        }
        for (channel_index, channel) in channels.iter_mut().enumerate() {
            let start = block_start + block_size * channel_index;
            let available_end = start.saturating_add(block_size).min(data.len());
            let available = data.get(start..available_end).unwrap_or(&[]);
            for sample in available.chunks(2) {
                channel.push(i16::from_be_bytes([
                    sample.first().copied().unwrap_or(0),
                    sample.get(1).copied().unwrap_or(0),
                ]));
            }
            let expected_samples = block_size / 2;
            let missing_samples = expected_samples.saturating_sub(available.len().div_ceil(2));
            if missing_samples != 0 {
                channel.resize(channel.len() + missing_samples, 0);
            }
        }
        cursor = end;
    }
    if channels.iter().any(|channel| channel.len() < total) {
        return Err("AST blocks contain fewer samples than declared".into());
    }
    for channel in &mut channels {
        channel.truncate(total);
    }
    Ok((channels, rate))
}

fn be_u16(data: &[u8], offset: usize) -> Result<u16, String> {
    let bytes = data
        .get(offset..offset + 2)
        .ok_or("truncated AST integer")?;
    Ok(u16::from_be_bytes([bytes[0], bytes[1]]))
}

fn be_u32(data: &[u8], offset: usize) -> Result<u32, String> {
    let bytes = data
        .get(offset..offset + 4)
        .ok_or("truncated AST integer")?;
    Ok(u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
}

fn apply_float_ops(samples: &mut Vec<f64>, operations: &[Operation]) -> Result<(), String> {
    for operation in operations {
        match operation.kind.as_str() {
            "mul" => {
                let scale =
                    parse_hex_float(operation.args.first().ok_or("missing audio multiply")?)?;
                for sample in samples.iter_mut() {
                    *sample *= scale;
                }
            }
            "peak" => {
                let limit = parse_hex_float(operation.args.first().ok_or("missing audio peak")?)?;
                let peak = samples.iter().fold(0.0f64, |p, x| p.max(x.abs()));
                if peak > limit {
                    let scale = limit / peak;
                    for sample in samples.iter_mut() {
                        *sample *= scale;
                    }
                }
            }
            "fade" => {
                let length = operation
                    .args
                    .first()
                    .and_then(serde_json::Value::as_u64)
                    .ok_or("invalid audio fade length")? as usize;
                let length = length.min(samples.len());
                if length > 1 {
                    let start = samples.len() - length;
                    let step = -1.0 / (length - 1) as f64;
                    for (offset, sample) in samples[start..].iter_mut().enumerate() {
                        *sample *= offset as f64 * step + 1.0;
                    }
                    let last = samples.len() - 1;
                    samples[last] *= 0.0;
                } else if length == 1 {
                    let last = samples.len() - 1;
                    samples[last] = 0.0;
                }
            }
            "seg" => {
                let ranges = operation
                    .args
                    .first()
                    .and_then(serde_json::Value::as_array)
                    .ok_or("invalid audio segment ranges")?;
                let mut selected = Vec::new();
                for range in ranges {
                    let pair = range.as_array().ok_or("invalid audio segment range")?;
                    let start =
                        pair.first()
                            .and_then(serde_json::Value::as_u64)
                            .ok_or("invalid audio segment start")? as usize;
                    let end = pair
                        .get(1)
                        .and_then(serde_json::Value::as_u64)
                        .ok_or("invalid audio segment end")? as usize;
                    let slice = samples
                        .get(start..end)
                        .ok_or("audio segment is outside source")?;
                    selected.extend_from_slice(slice);
                }
                *samples = selected;
            }
            other => return Err(format!("unsupported AST audio operation {other}")),
        }
    }
    Ok(())
}

fn quantise(
    samples: &[f64],
    final_ops: &[Operation],
    residuals: &[(usize, i32)],
    total: usize,
    seed_state: u128,
    seed_increment: u128,
) -> Result<Vec<u8>, String> {
    let mut keep = samples.len();
    let mut guide = samples.len();
    for operation in final_ops {
        match operation.kind.as_str() {
            "t" if keep > 4 => keep &= !3,
            "trim8" => {
                let maximum = operation
                    .args
                    .first()
                    .and_then(serde_json::Value::as_u64)
                    .ok_or("invalid audio trim length")? as usize;
                let fade = operation
                    .args
                    .get(1)
                    .and_then(serde_json::Value::as_u64)
                    .ok_or("invalid audio trim fade")? as usize;
                if keep > maximum {
                    keep = maximum;
                    guide = guide.min(maximum.saturating_sub(fade));
                }
            }
            "p" | "pad" => {}
            other => return Err(format!("unsupported AST audio final operation {other}")),
        }
    }
    guide = guide.min(keep);
    let mut output = vec![0u8; samples.len()];
    let mut correction_index = 0usize;
    let mut rng = Pcg64::new(seed_state, seed_increment);
    let mut random_a = vec![0.0f64; samples.len()];
    let mut random_b = vec![0.0f64; samples.len()];
    for value in &mut random_a {
        *value = (rng.next() >> 11) as f64 * (1.0 / 9_007_199_254_740_992.0);
    }
    for value in &mut random_b {
        *value = (rng.next() >> 11) as f64 * (1.0 / 9_007_199_254_740_992.0);
    }
    let mut error1 = 0.0;
    let mut error2 = 0.0;
    for index in 0..samples.len() {
        let dither = (random_a[index] - random_b[index]) * 0.5;
        let want = samples[index] * 127.0 - (1.5 * error1 - 0.6 * error2);
        let rounded = (want + dither + 0.5).floor();
        let mut value = rounded.clamp(-128.0, 127.0) as i32;
        if index < guide {
            while correction_index < residuals.len() && residuals[correction_index].0 < index {
                correction_index += 1;
            }
            if correction_index < residuals.len() && residuals[correction_index].0 == index {
                value += residuals[correction_index].1;
            }
        }
        error2 = error1;
        error1 = value as f64 - want;
        output[index] = value as i8 as u8;
    }
    for operation in final_ops {
        match operation.kind.as_str() {
            "t" => {
                if output.len() > 4 {
                    output.truncate(output.len() & !3);
                }
            }
            "p" => output.resize((output.len() + 3) & !3, 0),
            "pad" => {
                let count = operation
                    .args
                    .first()
                    .and_then(serde_json::Value::as_u64)
                    .ok_or("invalid audio pad size")? as usize;
                output.resize(
                    output
                        .len()
                        .checked_add(count)
                        .ok_or("audio output size overflow")?,
                    0,
                );
            }
            "trim8" => {
                let maximum = operation
                    .args
                    .first()
                    .and_then(serde_json::Value::as_u64)
                    .ok_or("invalid audio trim length")? as usize;
                let fade = operation
                    .args
                    .get(1)
                    .and_then(serde_json::Value::as_u64)
                    .ok_or("invalid audio trim fade")? as usize;
                if output.len() > maximum {
                    output.truncate(maximum);
                    if fade > 1 {
                        let start = output.len().saturating_sub(fade);
                        let step = -1.0 / (fade - 1) as f64;
                        for (offset, sample) in output[start..].iter_mut().enumerate() {
                            *sample = ((*sample as i8 as f64) * (offset as f64 * step + 1.0))
                                .round_ties_even() as i8
                                as u8;
                        }
                    }
                }
            }
            _ => unreachable!(),
        }
    }
    output.resize(total, 0);
    if output.len() > total {
        output.truncate(total);
    }
    for &(position, correction) in residuals {
        if position >= guide && position < output.len() {
            output[position] = (output[position] as i32 + correction) as u8;
        }
    }
    Ok(output)
}

struct Pcg64 {
    state: u128,
    increment: u128,
}

impl Pcg64 {
    fn new(state: u128, increment: u128) -> Self {
        Self { state, increment }
    }
    fn next(&mut self) -> u64 {
        let multiplier = ((2_549_297_995_355_413_924u128) << 64) | 4_865_540_595_714_422_341u128;
        self.state = self
            .state
            .wrapping_mul(multiplier)
            .wrapping_add(self.increment);
        let high = (self.state >> 64) as u64;
        let low = self.state as u64;
        let rotation = (self.state >> 122) as u32;
        let value = high ^ low;
        value.rotate_right(rotation)
    }
}

struct ResampleBank {
    rate: usize,
    length: usize,
    allocation: usize,
    coefficients: Vec<f32>,
}

impl ResampleBank {
    fn new(rate: usize) -> Self {
        let mut factor = OUTPUT_RATE as f64 * 0.94 / rate as f64;
        if factor > 1.0 {
            factor = 1.0;
        }
        let mut length = (64.0 / factor).ceil() as usize;
        length = length.max(1);
        if length > 1 {
            length = (length + 1) & !1;
        }
        let allocation = (length + 7) & !7;
        let mut coefficients = vec![0.0f32; (PHASES + 1) * allocation];
        let center = (length - 1) / 2;
        let mut table = vec![0.0f64; length];
        let mut norm = 0.0;
        for phase in 0..=PHASES / 2 {
            let mut s = 0.0;
            if factor == 1.0 {
                s = xsin(PI * phase as f64 / PHASES as f64)
                    * if center & 1 != 0 { 1.0 } else { -1.0 };
            }
            for index in 0..length {
                let x =
                    PI * ((index as f64 - center as f64) - phase as f64 / PHASES as f64) * factor;
                let mut y = if x == 0.0 {
                    1.0
                } else if factor == 1.0 {
                    s / x
                } else {
                    xsin(x) / x
                };
                let w = 2.0 * x / (factor * length as f64 * PI);
                let mut a = 1.0 - w * w;
                if a < 0.0 {
                    a = 0.0;
                }
                y *= bessel_i0(9.0 * a.sqrt());
                table[index] = y;
                s = -s;
                if phase == 0 {
                    norm += y;
                }
            }
            for index in 0..length {
                coefficients[phase * allocation + index] = (table[index] / norm) as f32;
            }
            let mirror = PHASES - phase;
            for index in 0..length {
                coefficients[mirror * allocation + (length - 1 - index)] =
                    coefficients[phase * allocation + index];
            }
        }
        let last_start = PHASES * allocation;
        for index in (0..allocation - 1).rev() {
            coefficients[last_start + index + 1] = coefficients[index];
        }
        coefficients[last_start] = coefficients[allocation - 1];
        Self {
            rate,
            length,
            allocation,
            coefficients,
        }
    }

    fn output_count(&self, samples: usize) -> usize {
        let reflected = self.reflect_length(samples);
        let available = samples + (self.length - 1) / 2 + reflected;
        let mut output = 0usize;
        while self.output_position(output) + self.length <= available {
            output += 1;
        }
        output
    }

    fn output_position(&self, output: usize) -> usize {
        ((output as u64 * self.rate as u64 * PHASES as u64 / OUTPUT_RATE as u64) / PHASES as u64)
            as usize
    }

    fn reflect_length(&self, samples: usize) -> usize {
        let available = samples + (self.length - 1) / 2;
        let mut output = 0usize;
        while self.output_position(output) + self.length <= available {
            output += 1;
        }
        let count = available - self.output_position(output);
        (count.min(self.length) + 1) / 2
    }

    fn run_channel(&self, input: &[i16], output_count: usize) -> Result<Vec<f32>, String> {
        let reflected = self.reflect_length(input.len());
        let total = self.length + input.len() + reflected;
        let mut buffer = vec![0.0f32; total + self.length + 8];
        for (index, sample) in input.iter().enumerate() {
            buffer[self.length + index] = *sample as f32 / 32_768.0f32;
        }
        for k in 1..=self.length {
            buffer[self.length - k] = if k < input.len() {
                buffer[self.length + k]
            } else {
                0.0
            };
        }
        for index in 0..reflected {
            buffer[self.length + input.len() + index] =
                buffer[self.length + input.len() - 1 - index];
        }
        let mut source_index = self.length as isize;
        let mut phase_index = -(PHASES as isize) * ((self.length - 1) / 2) as isize;
        while phase_index < 0 {
            source_index -= 1;
            phase_index += PHASES as isize;
        }
        let increment = self.rate * PHASES;
        let mut phase_fraction = 0usize;
        let phase_division = increment / OUTPUT_RATE;
        let phase_remainder = increment % OUTPUT_RATE;
        let mut output = vec![0.0f32; output_count];
        for value in &mut output {
            let phase = phase_index as usize;
            let coefficient = phase * self.allocation;
            let source = source_index as usize;
            if coefficient + self.allocation + self.length > self.coefficients.len()
                || source + self.length > buffer.len()
            {
                return Err(format!(
                    "AST resampler index out of bounds: phase={phase}, source={source}, length={}, coeffs={}, buffer={}",
                    self.length,
                    self.coefficients.len(),
                    buffer.len()
                ));
            }
            let mut first = 0.0f32;
            let mut second = 0.0f32;
            for index in 0..self.length {
                first =
                    buffer[source + index].mul_add(self.coefficients[coefficient + index], first);
                second = buffer[source + index].mul_add(
                    self.coefficients[coefficient + self.allocation + index],
                    second,
                );
            }
            // The C reference evaluates the interpolation expression in f64:
            // `inv` is double and `frac` is long, so the float delta and `val`
            // are promoted before the final assignment rounds back to float.
            let delta = second - first;
            *value = (first as f64
                + delta as f64 * (1.0 / OUTPUT_RATE as f64) * phase_fraction as f64)
                as f32;
            phase_fraction += phase_remainder;
            phase_index += phase_division as isize;
            if phase_fraction >= OUTPUT_RATE {
                phase_fraction -= OUTPUT_RATE;
                phase_index += 1;
            }
            while phase_index >= PHASES as isize {
                source_index += 1;
                phase_index -= PHASES as isize;
            }
        }
        Ok(output)
    }
}

fn ksin(x: f64, y: f64, iy: bool) -> f64 {
    let (s1, s2, s3, s4, s5, s6) = (
        -1.66666666666666324348e-01,
        8.33333333332248946124e-03,
        -1.98412698298579493134e-04,
        2.75573137070700676789e-06,
        -2.50507602534068634195e-08,
        1.58969099521155010221e-10,
    );
    let z = x * x;
    let v = z * x;
    let r = s2 + z * (s3 + z * (s4 + z * (s5 + z * s6)));
    if !iy {
        return x + v * (s1 + z * r);
    }
    x - ((z * (0.5 * y - v * r) - y) - v * s1)
}

fn kcos(x: f64, y: f64) -> f64 {
    let (c1, c2, c3, c4, c5, c6) = (
        4.16666666666666019037e-02,
        -1.38888888888741095749e-03,
        2.48015872894702575469e-05,
        -2.75573143513906633035e-07,
        2.08757232129817482790e-09,
        -1.13596475577881948265e-11,
    );
    let z = x * x;
    let r = z * (c1 + z * (c2 + z * (c3 + z * (c4 + z * (c5 + z * c6)))));
    let half_z = 0.5 * z;
    let w = 1.0 - half_z;
    w + (((1.0 - w) - half_z) + (z * r - x * y))
}

fn xsin(x: f64) -> f64 {
    const INV_PIO2: f64 = 6.36619772367581382433e-01;
    const PIO2_1: f64 = 1.57079632673412561417e+00;
    const PIO2_2: f64 = 6.07710050630396597660e-11;
    const PIO2_2T: f64 = 2.02226624879595063154e-21;
    const PIO2_3: f64 = 2.02226624871116645580e-21;
    const PIO2_3T: f64 = 8.47842766036889956997e-32;
    let absolute = x.abs();
    let result = if absolute <= 7.85398163397448278999e-01 {
        ksin(absolute, 0.0, false)
    } else {
        let multiple = (absolute * INV_PIO2 + 0.5).floor();
        let quadrant = multiple as i32;
        let mut remainder = absolute - multiple * PIO2_1;
        let mut t = remainder;
        let mut w = multiple * PIO2_2;
        remainder = t - w;
        let _correction = multiple * PIO2_2T - ((t - remainder) - w);
        t = remainder;
        let third = multiple * PIO2_3;
        remainder = t - third;
        w = multiple * PIO2_3T - ((t - remainder) - third);
        let y0 = remainder - w;
        let y1 = (remainder - y0) - w;
        match quadrant & 3 {
            0 => ksin(y0, y1, true),
            1 => kcos(y0, y1),
            2 => -ksin(y0, y1, true),
            _ => -kcos(y0, y1),
        }
    };
    if x < 0.0 {
        -result
    } else {
        result
    }
}

fn bessel_i0(x: f64) -> f64 {
    let t = x * x / 4.0;
    let mut value = 1.0;
    let mut term = 1.0;
    for index in 1..200 {
        term *= t / (index as f64 * index as f64);
        let next = value + term;
        if next == value {
            break;
        }
        value = next;
    }
    value
}

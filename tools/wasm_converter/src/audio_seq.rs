//! JAudio sequence interpreter for the sound recipes.

use std::collections::BTreeMap;

#[derive(Clone, Debug)]
pub struct AudioNote {
    pub time: f64,
    pub bank: u16,
    pub program: u16,
    pub key: u8,
    pub velocity: u8,
    pub gate: Option<f64>,
    pub volume: f64,
    pub pitch_octaves: f64,
    pub adsr: Option<[i32; 5]>,
}

#[derive(Clone)]
struct Track {
    pc: usize,
    parent: Option<usize>,
    registers: BTreeMap<u8, u16>,
    ports: [u16; 16],
    bank: u16,
    program: u16,
    transpose: i32,
    bend_sense: u16,
    tempo: u16,
    timebase: u16,
    time: f64,
    stack: Vec<usize>,
    loops: Vec<(usize, i32)>,
    params: [f64; 2],
    done: bool,
    held: BTreeMap<u8, usize>,
    children: BTreeMap<u8, usize>,
    adsr: Option<[i32; 5]>,
}

impl Track {
    fn new(pc: usize, parent: Option<usize>, time: f64, tempo: u16, timebase: u16) -> Self {
        Self {
            pc,
            parent,
            registers: BTreeMap::new(),
            ports: [0; 16],
            bank: 0,
            program: 0,
            transpose: 0,
            bend_sense: 12,
            tempo,
            timebase,
            time,
            stack: Vec::new(),
            loops: Vec::new(),
            params: [1.0, 0.0],
            done: false,
            held: BTreeMap::new(),
            children: BTreeMap::new(),
            adsr: None,
        }
    }

    fn read_register(&self, register: u8) -> u16 {
        match register {
            0x00..=0x3f => self.registers.get(&register).copied().unwrap_or(0),
            0x40..=0x4f => self.ports[(register - 0x40) as usize],
            0x62 => self.timebase,
            0x63 => self.transpose as u16,
            0x64 => self.bend_sense,
            0x67 => self.bank,
            0x68 => self.program,
            _ => 0,
        }
    }

    fn write_register(&mut self, register: u8, value: u32) {
        let value = value as u16;
        self.registers.insert(0x0f, value);
        match register {
            0x00..=0x3f => {
                self.registers.insert(register, value);
            }
            0x40..=0x4f => self.ports[(register - 0x40) as usize] = value,
            0x62 => self.timebase = value,
            0x63 => self.transpose = value as i16 as i32,
            0x64 => self.bend_sense = value,
            0x67 => self.bank = value & 0xff,
            0x68 => self.program = value & 0xff,
            _ => {}
        }
    }
}

#[derive(Debug)]
enum Instruction {
    NoteOn {
        key: u8,
        flags: u8,
        velocity: u8,
        gate_ticks: Option<u32>,
    },
    NoteOff(u8),
    Command(u8, Vec<u32>),
    Extension(u8),
    Unknown,
}

pub fn simulate(
    data: &[u8],
    start: usize,
    ports: Option<&BTreeMap<u8, u16>>,
    seed: Option<u64>,
    max_time: f64,
    max_steps: usize,
) -> Vec<AudioNote> {
    let mut root = Track::new(start, None, 0.0, 120, 48);
    if let Some(ports) = ports {
        for (&port, &value) in ports {
            if let Some(target) = root.ports.get_mut(port as usize) {
                *target = value;
            }
        }
    }
    let mut tracks = vec![root];
    let mut notes = Vec::new();
    let mut random = seed.map(PythonRandom::new);
    for _ in 0..max_steps {
        let next = tracks
            .iter()
            .enumerate()
            .filter(|(_, track)| !track.done)
            .min_by(|(_, left), (_, right)| left.time.total_cmp(&right.time))
            .map(|(index, track)| (index, track.time));
        let Some((track_index, time)) = next else {
            break;
        };
        if time > max_time {
            break;
        }
        let pc = tracks[track_index].pc;
        let instruction = match decode(data, pc, |register| {
            tracks[track_index].read_register(register)
        }) {
            Ok((instruction, next_pc)) => {
                tracks[track_index].pc = next_pc;
                instruction
            }
            Err(()) => {
                tracks[track_index].done = true;
                continue;
            }
        };
        execute(
            instruction,
            data,
            track_index,
            &mut tracks,
            &mut notes,
            &mut random,
        );
    }
    notes
}

fn execute(
    instruction: Instruction,
    data: &[u8],
    index: usize,
    tracks: &mut Vec<Track>,
    notes: &mut Vec<AudioNote>,
    random: &mut Option<PythonRandom>,
) {
    match instruction {
        Instruction::NoteOn {
            key,
            flags,
            velocity,
            gate_ticks,
        } => {
            let slot = flags & 7;
            let gate = if slot == 0 {
                gate_ticks
                    .filter(|ticks| *ticks != 0)
                    .map(|ticks| ticks as f64 * tick_seconds(tracks, index))
            } else {
                None
            };
            let note_index = add_note(tracks, index, key, velocity, gate, notes);
            if slot == 0 {
                if let Some(gate) = gate {
                    if gate != 0.0 {
                        tracks[index].time += gate;
                    }
                }
            } else {
                tracks[index].held.insert(slot, note_index);
            }
        }
        Instruction::NoteOff(slot) => release_note(tracks, index, slot, notes),
        Instruction::Extension(_) | Instruction::Unknown => {}
        Instruction::Command(opcode, args) => match opcode {
            0xb1 => {
                let slot = (args[0] & 7) as u8;
                let note_index = add_note(tracks, index, args[1] as u8, args[2] as u8, None, notes);
                tracks[index].held.insert(slot, note_index);
            }
            0xb2 => release_note(tracks, index, (args[0] & 7) as u8, notes),
            0xb3 => {
                let gate = args[3] as f64 * tick_seconds(tracks, index);
                add_note(
                    tracks,
                    index,
                    args[1] as u8,
                    args[2] as u8,
                    Some(gate),
                    notes,
                );
                tracks[index].time += gate;
            }
            0xb8 | 0xba => {
                let value = args[1] as u8;
                let scaled = ((value as u32) << 8)
                    | if value & 0x80 == 0 {
                        (value as u32) << 1
                    } else {
                        0
                    };
                if let Some(parameter) = tracks[index].params.get_mut(args[0] as usize) {
                    *parameter = (scaled as i16) as f64 / 32767.0;
                }
            }
            0xb9 | 0xbb => {
                if let Some(parameter) = tracks[index].params.get_mut(args[0] as usize) {
                    *parameter = (args[1] as u16 as i16) as f64 / 32767.0;
                }
            }
            0xc1 => {
                let child_index = tracks.len();
                let parent = &tracks[index];
                let mut child = Track::new(
                    args[1] as usize,
                    Some(index),
                    parent.time,
                    parent.tempo,
                    parent.timebase,
                );
                child.bank = parent.bank;
                child.program = parent.program;
                tracks.push(child);
                tracks[index].children.insert(args[0] as u8, child_index);
            }
            0xc2 => {
                if let Some(&child) = tracks[index].children.get(&(args[0] as u8)) {
                    tracks[child].done = true;
                }
            }
            0xc3 => {
                let return_pc = tracks[index].pc;
                tracks[index].stack.push(return_pc);
                tracks[index].pc = args[0] as usize;
            }
            0xc4 => {
                if condition(&tracks[index], args[0] as u8) {
                    let return_pc = tracks[index].pc;
                    tracks[index].stack.push(return_pc);
                    tracks[index].pc = args[1] as usize;
                }
            }
            0xc5 | 0xf6 => return_from_call(tracks, index),
            0xc6 => {
                if condition(&tracks[index], args[0] as u8) {
                    return_from_call(tracks, index);
                }
            }
            0xc7 => tracks[index].pc = args[0] as usize,
            0xc8 => {
                if condition(&tracks[index], args[0] as u8) {
                    tracks[index].pc = args[1] as usize;
                }
            }
            0xc9 | 0xca => {
                let table = args[1] as usize + (args[0] as usize).saturating_mul(3);
                if let Some(target) = read_u24(data, table) {
                    if opcode == 0xca {
                        let return_pc = tracks[index].pc;
                        tracks[index].stack.push(return_pc);
                    }
                    tracks[index].pc = target as usize;
                } else {
                    tracks[index].done = true;
                }
            }
            0xcb => {
                let loop_start = tracks[index].pc;
                tracks[index].loops.push((loop_start, args[0] as i32));
            }
            0xcc => {
                if let Some((start, remaining)) = tracks[index].loops.last_mut() {
                    *remaining -= 1;
                    if *remaining > 0 {
                        tracks[index].pc = *start;
                    } else {
                        tracks[index].loops.pop();
                    }
                }
            }
            0xd0 => {
                let value = tracks[index].ports[(args[0] as usize) & 15];
                tracks[index].write_register(args[1] as u8, value as u32);
            }
            0xd1 => tracks[index].ports[(args[0] as usize) & 15] = args[1] as u16,
            0xd2 | 0xd3 => {
                tracks[index].registers.insert(0x0f, 0);
            }
            0xd4 => {
                if let Some(parent) = tracks[index].parent {
                    tracks[parent].ports[(args[0] as usize) & 15] = args[1] as u16;
                }
            }
            0xd5 => {
                if let Some(&child) = tracks[index].children.get(&((args[0] >> 4) as u8)) {
                    tracks[child].ports[(args[0] as usize) & 15] = args[1] as u16;
                }
            }
            0xd6 => {
                let value = tracks[index]
                    .parent
                    .map_or(0, |parent| tracks[parent].ports[(args[0] as usize) & 15]);
                tracks[index].write_register(args[1] as u8, value as u32);
            }
            0xd7 => {
                let value = tracks[index]
                    .children
                    .get(&((args[0] >> 4) as u8))
                    .map_or(0, |child| tracks[*child].ports[(args[0] as usize) & 15]);
                tracks[index].write_register(args[1] as u8, value as u32);
            }
            0xd8 => tracks[index].write_register(args[0] as u8, args[1]),
            0xd9 | 0xda => arithmetic(
                &mut tracks[index],
                args[0] as u8,
                args[1] as u8,
                args[2],
                random,
            ),
            0xdb => {
                let old = tracks[index].read_register(args[1] as u8);
                tracks[index].write_register(
                    args[1] as u8,
                    if args[0] == 0x0b {
                        (0u16.wrapping_sub(old)) as u32
                    } else {
                        old as u32
                    },
                );
            }
            0xdc => table_load(&mut tracks[index], data, args[0], args[1], args[2], args[3]),
            0xe0 => tracks[index].tempo = args[0] as u16,
            0xe1 => {
                tracks[index].bank = (args[0] >> 8) as u16;
                tracks[index].program = (args[0] & 0xff) as u16;
            }
            0xe2 => tracks[index].bank = (args[0] & 0xff) as u16,
            0xe3 => tracks[index].program = (args[0] & 0xff) as u16,
            0xe9 => {
                tracks[index].adsr = Some([
                    args[0] as u16 as i16 as i32,
                    args[1] as u16 as i16 as i32,
                    args[2] as u16 as i16 as i32,
                    args[3] as u16 as i16 as i32,
                    args[4] as i32,
                ]);
            }
            0xf0 | 0xf1 => tracks[index].time += args[0] as f64 * tick_seconds(tracks, index),
            0xff => tracks[index].done = true,
            _ => {}
        },
    }
}

fn add_note(
    tracks: &TrackVec,
    track_index: usize,
    key: u8,
    velocity: u8,
    gate: Option<f64>,
    notes: &mut Vec<AudioNote>,
) -> usize {
    let track = &tracks[track_index];
    let note = AudioNote {
        time: track.time,
        bank: track.bank,
        program: track.program,
        key: key.wrapping_add(track.transpose as u8),
        velocity,
        gate,
        volume: track.params[0],
        pitch_octaves: track.params[1] * track.bend_sense as f64 / 3.0,
        adsr: track.adsr,
    };
    notes.push(note);
    notes.len() - 1
}

struct PythonRandom {
    state: [u32; 624],
    index: usize,
}

impl PythonRandom {
    fn new(seed: u64) -> Self {
        let mut key = Vec::new();
        let mut value = seed;
        loop {
            key.push(value as u32);
            value >>= 32;
            if value == 0 {
                break;
            }
        }
        let mut random = Self {
            state: [0; 624],
            index: 624,
        };
        random.state[0] = 19_650_218;
        for index in 1..624 {
            random.state[index] = 1_812_433_253u32
                .wrapping_mul(random.state[index - 1] ^ (random.state[index - 1] >> 30))
                .wrapping_add(index as u32);
        }
        let mut i = 1usize;
        let mut j = 0usize;
        for _ in 0..624.max(key.len()) {
            random.state[i] = (random.state[i]
                ^ (random.state[i - 1] ^ (random.state[i - 1] >> 30)).wrapping_mul(1_664_525))
            .wrapping_add(key[j])
            .wrapping_add(j as u32);
            i += 1;
            j += 1;
            if i >= 624 {
                random.state[0] = random.state[623];
                i = 1;
            }
            if j >= key.len() {
                j = 0;
            }
        }
        for _ in 0..623 {
            random.state[i] = (random.state[i]
                ^ (random.state[i - 1] ^ (random.state[i - 1] >> 30)).wrapping_mul(1_566_083_941))
            .wrapping_sub(i as u32);
            i += 1;
            if i >= 624 {
                random.state[0] = random.state[623];
                i = 1;
            }
        }
        random.state[0] = 0x8000_0000;
        random
    }

    fn next_u32(&mut self) -> u32 {
        if self.index >= 624 {
            for i in 0..624 {
                let joined =
                    (self.state[i] & 0x8000_0000) | (self.state[(i + 1) % 624] & 0x7fff_ffff);
                self.state[i] = self.state[(i + 397) % 624]
                    ^ (joined >> 1)
                    ^ if joined & 1 != 0 { 0x9908_b0df } else { 0 };
            }
            self.index = 0;
        }
        let mut value = self.state[self.index];
        self.index += 1;
        value ^= value >> 11;
        value ^= (value << 7) & 0x9d2c_5680;
        value ^= (value << 15) & 0xefc6_0000;
        value ^= value >> 18;
        value
    }

    fn below(&mut self, upper: u32) -> u32 {
        let bits = 32 - upper.leading_zeros();
        loop {
            let value = self.next_u32() >> (32 - bits);
            if value < upper {
                return value;
            }
        }
    }
}

type TrackVec = Vec<Track>;

fn release_note(tracks: &mut TrackVec, index: usize, slot: u8, notes: &mut [AudioNote]) {
    if let Some(note_index) = tracks[index].held.remove(&slot) {
        notes[note_index].gate = Some(tracks[index].time - notes[note_index].time);
    }
}

fn tick_seconds(tracks: &TrackVec, index: usize) -> f64 {
    let mut root = index;
    while let Some(parent) = tracks[root].parent {
        root = parent;
    }
    60.0 / (tracks[root].tempo.max(1) as f64 * tracks[root].timebase.max(1) as f64)
}

fn condition(track: &Track, condition: u8) -> bool {
    let flags = track.read_register(0x0f);
    match condition {
        0 => true,
        1 => flags == 0,
        2 => flags != 0,
        3 => flags == 1,
        4 => flags >= 0x8000,
        5 => flags < 0x8000,
        _ => false,
    }
}

fn return_from_call(tracks: &mut TrackVec, index: usize) {
    if let Some(pc) = tracks[index].stack.pop() {
        tracks[index].pc = pc;
    } else {
        tracks[index].done = true;
    }
}

fn arithmetic(
    track: &mut Track,
    operation: u8,
    register: u8,
    argument: u32,
    random: &mut Option<PythonRandom>,
) {
    let current = track.read_register(register) as u32;
    let (target, value) = match operation {
        0 => (register, argument),
        1 => (register, current.wrapping_add(argument)),
        2 => (register, current.wrapping_sub(argument)),
        3 => (0x0f, current.wrapping_sub(argument)),
        4 => (0x21, current.wrapping_mul(argument)),
        5 => (register, current & argument),
        6 => (register, current | argument),
        7 => (register, current ^ argument),
        8 => (
            register,
            random
                .as_mut()
                .filter(|_| argument != 0)
                .map_or(0, |random| random.below(argument)),
        ),
        9 => (register, current.wrapping_shl(argument & 31)),
        10 => (register, current.wrapping_shr(argument & 31)),
        _ => return,
    };
    track.write_register(target, value);
}

fn table_load(
    track: &mut Track,
    data: &[u8],
    operation: u32,
    register: u32,
    table: u32,
    index: u32,
) {
    let table = table as usize;
    let index = index as usize;
    let value = match operation {
        0x0c => data
            .get(table.saturating_add(index))
            .copied()
            .map(u32::from),
        0x0d => read_u16(data, table.saturating_add(index.saturating_mul(2))).map(u32::from),
        0x0e => read_u24(data, table.saturating_add(index.saturating_mul(3))),
        0x0f => read_u32(data, table.saturating_add(index.saturating_mul(4))),
        _ => Some(0),
    };
    if let Some(value) = value {
        track.write_register(register as u8, value);
    }
}

fn decode(
    data: &[u8],
    mut pc: usize,
    mut read_register: impl FnMut(u8) -> u16,
) -> Result<(Instruction, usize), ()> {
    let opcode = read_u8(data, &mut pc)?;
    if opcode < 0x80 {
        let flags = read_u8(data, &mut pc)?;
        let velocity = read_u8(data, &mut pc)?;
        let gate_ticks = if flags & 7 == 0 {
            Some(read_midi(data, &mut pc)?)
        } else {
            None
        };
        return Ok((
            Instruction::NoteOn {
                key: opcode,
                flags,
                velocity,
                gate_ticks,
            },
            pc,
        ));
    }
    if opcode & 0xf0 == 0x80 {
        return Ok((Instruction::NoteOff(opcode & 7), pc));
    }
    let mut operation = opcode;
    let mut widths = 0u16;
    if opcode & 0xf0 == 0x90 {
        let count = (opcode & 7) + 1;
        let mut mask = read_u8(data, &mut pc)?;
        let mut bit = 3u16;
        for _ in 0..count {
            if mask & 0x80 != 0 {
                widths |= bit;
            }
            mask <<= 1;
            bit <<= 2;
        }
        operation = read_u8(data, &mut pc)?;
    }
    if operation == 0xb0 {
        return Ok((Instruction::Extension(read_u8(data, &mut pc)?), pc));
    }
    if operation == 0xfd {
        let mut percent_count = 0usize;
        loop {
            match read_u8(data, &mut pc)? {
                0 => break,
                b'%' => percent_count += 1,
                _ => {}
            }
        }
        for _ in 0..percent_count {
            read_u8(data, &mut pc)?;
        }
        return Ok((Instruction::Command(operation, Vec::new()), pc));
    }
    let Some((argument_count, base_widths)) = command_shape(operation) else {
        return Ok((Instruction::Unknown, pc));
    };
    let mut width = base_widths | widths;
    let mut arguments = Vec::with_capacity(argument_count);
    for _ in 0..argument_count {
        let argument = match width & 3 {
            0 => read_u8(data, &mut pc)? as u32,
            1 => read_u16(data, pc).ok_or(())? as u32,
            2 => read_u24(data, pc).ok_or(())?,
            _ => read_register(read_u8(data, &mut pc)?) as u32,
        };
        if width & 3 == 1 {
            pc += 2;
        } else if width & 3 == 2 {
            pc += 3;
        }
        arguments.push(argument);
        width >>= 2;
    }
    if operation == 0xf0 {
        arguments = vec![read_midi(data, &mut pc)?];
    }
    Ok((Instruction::Command(operation, arguments), pc))
}

fn command_shape(opcode: u8) -> Option<(usize, u16)> {
    Some(match opcode {
        0xb1 => (3, 0x00),
        0xb2 => (1, 0x00),
        0xb3 => (4, 0x40),
        0xb4 => (1, 0x00),
        0xb8 => (2, 0x00),
        0xb9 => (2, 0x04),
        0xba => (3, 0x10),
        0xbb => (3, 0x14),
        0xc1 => (2, 0x08),
        0xc2 => (1, 0x00),
        0xc3 => (1, 0x02),
        0xc4 => (2, 0x08),
        0xc5 => (0, 0x00),
        0xc6 => (1, 0x00),
        0xc7 => (1, 0x02),
        0xc8 => (2, 0x08),
        0xc9 | 0xca => (2, 0x0b),
        0xcb => (1, 0x01),
        0xcc => (0, 0x00),
        0xd0 => (2, 0x00),
        0xd1 => (2, 0x0c),
        0xd2 | 0xd3 => (1, 0x00),
        0xd4 | 0xd5 => (2, 0x0c),
        0xd6 | 0xd7 => (2, 0x00),
        0xd8 => (2, 0x04),
        0xd9 => (3, 0x30),
        0xda => (3, 0x10),
        0xdb => (2, 0x00),
        0xdc => (4, 0xe0),
        0xe0 | 0xe1 => (1, 0x01),
        0xe2 | 0xe3 => (1, 0x00),
        0xe7 => (2, 0x04),
        0xe8 => (2, 0x08),
        0xe9 => (5, 0x155),
        0xea => (2, 0x04),
        0xeb => (1, 0x00),
        0xec => (4, 0x55),
        0xed => (1, 0x02),
        0xf0 | 0xf6 | 0xf7 | 0xfc | 0xfe | 0xff => (0, 0x00),
        0xf1 | 0xf3 | 0xf4 | 0xf5 | 0xf9 => (
            1,
            if opcode == 0xf3 {
                0x02
            } else if matches!(opcode, 0xf4 | 0xf5 | 0xf9) {
                0x01
            } else {
                0x00
            },
        ),
        0xf8 => (2, 0x04),
        0xfd => (0, 0x00),
        _ => return None,
    })
}

fn read_u8(data: &[u8], pc: &mut usize) -> Result<u8, ()> {
    let value = *data.get(*pc).ok_or(())?;
    *pc += 1;
    Ok(value)
}

fn read_u16(data: &[u8], offset: usize) -> Option<u16> {
    Some(u16::from_be_bytes([
        *data.get(offset)?,
        *data.get(offset + 1)?,
    ]))
}

fn read_u24(data: &[u8], offset: usize) -> Option<u32> {
    Some(
        ((*data.get(offset)? as u32) << 16)
            | ((*data.get(offset + 1)? as u32) << 8)
            | *data.get(offset + 2)? as u32,
    )
}

fn read_u32(data: &[u8], offset: usize) -> Option<u32> {
    Some(u32::from_be_bytes([
        *data.get(offset)?,
        *data.get(offset + 1)?,
        *data.get(offset + 2)?,
        *data.get(offset + 3)?,
    ]))
}

fn read_midi(data: &[u8], pc: &mut usize) -> Result<u32, ()> {
    let byte = read_u8(data, pc)?;
    if byte & 0x80 == 0 {
        return Ok(byte as u32);
    }
    let mut value = (byte & 0x7f) as u32;
    for _ in 0..3 {
        value <<= 7;
        let byte = read_u8(data, pc)?;
        value |= (byte & 0x7f) as u32;
        if byte & 0x80 == 0 {
            return Ok(value);
        }
    }
    Ok(0)
}

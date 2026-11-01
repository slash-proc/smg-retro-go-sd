//! Canonical texture-bank deduplication and mip packing from `tools/assets/textures.py`.

use std::collections::{BTreeMap, HashMap, VecDeque};

const MIP_BYTES: usize = 1024;
const TEXTURE_BYTES: usize = 2048;
const PACKED_FAR_TEXTURES: u16 = 8;
const FLAT_FAR_TEXTURE: u32 = 0x8000_0000;
const DETAILED_FAR_TEXTURE: u32 = 0x4000_0000;

fn far_texture_payload(record: u32) -> Option<usize> {
    if record & FLAT_FAR_TEXTURE == 0 {
        Some(record as usize)
    } else if record & DETAILED_FAR_TEXTURE != 0 {
        Some(((record & 0x3fff_ff00) >> 6) as usize)
    } else {
        None
    }
}
const GRAVITY_BYTES: usize = 80;
const WIRE_GRAVITY: u8 = 8;
const FIGURE_POINTERS: [usize; 10] = [8, 12, 16, 20, 24, 32, 40, 44, 48, 56];
const STAGE_POINTERS: [usize; 26] = [
    28, 32, 36, 40, 44, 48, 52, 56, 60, 64, 68, 76, 84, 92, 100, 108, 116, 120, 124, 132, 140, 176,
    180, 188, 192, 200,
];

#[derive(Clone)]
struct TextureBank {
    origin: usize,
    field: usize,
    at: usize,
    records: Vec<Vec<u8>>,
}

struct FarBank {
    origin: usize,
    bank: usize,
    pairs: Vec<Option<(usize, usize)>>,
    table: String,
}

pub fn pool_textures(
    text: &mut String,
    header: &mut String,
    blobs: &mut BTreeMap<String, Vec<u8>>,
) -> Result<(), String> {
    let layout = crate::asset_assembler::assemble_layout(text, 0, |name| {
        blobs
            .get(name)
            .cloned()
            .ok_or_else(|| format!("missing asset compiler blob {name}"))
    })?;
    let memory = layout.data;
    let mut banks = Vec::new();
    let mut far_banks = Vec::new();
    let mut references = Vec::new();
    let mut replaced = Vec::new();

    let stage_names = header_list(header, "STAGE_BLOBS")?;
    let far_names = header_list(header, "STAGE_FAR_BLOBS")?;
    for name in stage_names {
        let origin = *layout
            .symbols
            .get(&name)
            .ok_or_else(|| format!("missing stage symbol {name}"))?;
        add_stage(
            &memory,
            origin,
            false,
            &name,
            &mut banks,
            &mut far_banks,
            &mut references,
            &mut replaced,
        )?;
    }
    for name in far_names.into_iter().filter(|name| name != "NULL") {
        let origin = *layout
            .symbols
            .get(&name)
            .ok_or_else(|| format!("missing stage symbol {name}"))?;
        add_stage(
            &memory,
            origin,
            true,
            &name,
            &mut banks,
            &mut far_banks,
            &mut references,
            &mut replaced,
        )?;
    }
    for name in header_figures(header) {
        let origin = *layout
            .symbols
            .get(&name)
            .ok_or_else(|| format!("missing figure symbol {name}"))?;
        add_figure(&memory, origin, &mut banks, &mut references, &mut replaced)?;
    }

    let mut removed = banks
        .iter()
        .map(|bank| (bank.at, bank.at + bank.records.len() * TEXTURE_BYTES))
        .collect::<Vec<_>>();
    removed.sort_unstable();
    let mut merged: Vec<(usize, usize)> = Vec::new();
    for (low, high) in removed {
        if let Some(last) = merged.last_mut().filter(|last| low <= last.1) {
            last.1 = last.1.max(high);
        } else {
            merged.push((low, high));
        }
    }
    let removed = merged;
    let inside_removed = |at: usize| removed.iter().any(|(low, high)| *low <= at && at < *high);
    let mut names_at: HashMap<usize, Vec<String>> = HashMap::new();
    for (name, at) in &layout.symbols {
        if !inside_removed(*at) {
            names_at.entry(*at).or_default().push(name.clone());
        }
    }
    for names in names_at.values_mut() {
        names.sort();
    }

    let mut words = layout.words;
    let mut labels_at: HashMap<usize, String> = HashMap::new();
    let label_at = |at: usize, labels: &mut HashMap<usize, String>| {
        labels
            .entry(at)
            .or_insert_with(|| format!("texture_retained_{at}"))
            .clone()
    };
    for &(slot, target, origin, tag) in &references {
        let target_label = label_at(target, &mut labels_at);
        let origin_label = label_at(origin, &mut labels_at);
        words.insert(slot, format!("{target_label} - {origin_label}"));
        if tag != 0 {
            words.insert(slot, format!("{target_label} - {origin_label} + {tag}"));
        }
    }

    let (pool_bytes, bank_tables) = build_mip_pool(&banks)?;
    let mut tables: Vec<(Vec<(usize, usize)>, String)> = Vec::new();
    let mut far_offsets = BTreeMap::new();
    for (bank, pairs) in banks.iter().zip(bank_tables) {
        if bank.field == usize::MAX {
            far_offsets.insert(bank.at, pairs[0]);
            continue;
        }
        let table_label = if let Some((_, label)) = tables.iter().find(|(known, _)| *known == pairs)
        {
            label.clone()
        } else {
            let label = format!("texture_refs_{}", tables.len());
            tables.push((pairs, label.clone()));
            label
        };
        let origin_label = label_at(bank.origin, &mut labels_at);
        words.insert(
            bank.origin + bank.field,
            format!("{table_label} - {origin_label}"),
        );
    }

    let mut far_lines = Vec::new();
    for bank in &far_banks {
        let origin_label = label_at(bank.origin, &mut labels_at);
        words.insert(bank.origin + 36, format!("{} - {origin_label}", bank.table));
        far_lines.extend([
            format!("    .section .rodata.assets.{}, \"a\"", bank.table),
            "    .balign 4".into(),
            format!("    .word {}", label_at(bank.bank, &mut labels_at)),
            "    .word 0".into(),
            format!("    .global {}", bank.table),
            format!("{}:", bank.table),
        ]);
        for (index, pair) in bank.pairs.iter().enumerate() {
            if let Some((source, _)) = pair {
                let (first, second) = far_offsets[source];
                far_lines.push(format!("    .word texture_mip_raw_pool + {first}"));
                far_lines.push(format!("    .word texture_mip_raw_pool + {second}"));
                if read_u32(&memory, bank.bank + index * 4)? & FLAT_FAR_TEXTURE == 0 {
                    words.insert(bank.bank + index * 4, format!("texture_mip_raw_pool + {first} - {origin_label}"));
                }
            } else {
                far_lines.push("    .word 0".into());
                far_lines.push("    .word 0".into());
            }
        }
    }

    let kept = retained_ranges(&removed, memory.len());
    let mut retained = Vec::new();
    for &(first, last) in &kept {
        retained.extend_from_slice(&memory[first..last]);
    }
    let mut lines = vec![
        "    .section .rodata.assets, \"a\"".into(),
        "    .balign 4".into(),
    ];
    let mut emitted = 0usize;
    for &(first, last) in &kept {
        let mut marks = vec![first, last];
        marks.extend(
            words
                .keys()
                .copied()
                .filter(|at| first <= *at && *at < last),
        );
        marks.extend(
            names_at
                .keys()
                .copied()
                .filter(|at| first <= *at && *at < last),
        );
        marks.extend(
            labels_at
                .keys()
                .copied()
                .filter(|at| first <= *at && *at < last),
        );
        marks.sort_unstable();
        marks.dedup();
        for pair in marks.windows(2) {
            let mut at = pair[0];
            let stop = pair[1];
            if let Some(names) = names_at.get(&at) {
                for name in names {
                    lines.push(format!("    .global {name}"));
                    lines.push(format!("{name}:"));
                }
            }
            if let Some(label) = labels_at.get(&at) {
                lines.push(format!("{label}:"));
            }
            if let Some(expression) = words.get(&at) {
                lines.push(format!("    .word {expression}"));
                at += 4;
                emitted += 4;
            }
            if at < stop {
                let size = stop - at;
                lines.push(format!(
                    "    .incbin \"texture_retained_section_0.bin\", {emitted}, {size}"
                ));
                emitted += size;
            }
        }
    }
    blobs.insert("texture_retained_section_0.bin".into(), retained);
    blobs.insert("texture_mip_raw_pool.bin".into(), pool_bytes);
    lines.extend([
        "    .section .rodata.assets.raw_texture_pool, \"a\"".into(),
        "    .balign 4".into(),
        "    .global texture_mip_raw_pool".into(),
        "texture_mip_raw_pool:".into(),
        "    .incbin \"texture_mip_raw_pool.bin\"".into(),
    ]);
    for (pairs, label) in &tables {
        lines.push("    .balign 4".into());
        lines.push(format!("    .global {label}"));
        lines.push(format!("{label}:"));
        for (first, second) in pairs {
            lines.push(format!("    .word texture_mip_raw_pool + {first}"));
            lines.push(format!("    .word texture_mip_raw_pool + {second}"));
        }
    }
    lines.extend(far_lines);
    *text = lines.join("\n") + "\n";
    *header = header.replacen(
        "#define ASSETS_GEN_H",
        "#define ASSETS_GEN_H\n#define GALAXY_RAW_TEXTURE_REFERENCES 2",
        1,
    );
    Ok(())
}

fn read_u16(data: &[u8], at: usize) -> Result<u16, String> {
    let bytes = data
        .get(at..at + 2)
        .ok_or("texture packer u16 out of bounds")?;
    Ok(u16::from_le_bytes([bytes[0], bytes[1]]))
}

fn read_u32(data: &[u8], at: usize) -> Result<u32, String> {
    let bytes = data
        .get(at..at + 4)
        .ok_or("texture packer u32 out of bounds")?;
    Ok(u32::from_le_bytes(bytes.try_into().unwrap()))
}

fn read_i32(data: &[u8], at: usize) -> Result<i32, String> {
    Ok(read_u32(data, at)? as i32)
}

fn push_reference(
    data: &[u8],
    slot: usize,
    origin: usize,
    tag: usize,
    references: &mut Vec<(usize, usize, usize, usize)>,
) -> Result<(), String> {
    let target = (origin as i64 + read_i32(data, slot)? as i64 - tag as i64)
        .try_into()
        .map_err(|_| "texture relocation target is out of bounds")?;
    references.push((slot, target, origin, tag));
    Ok(())
}

fn add_bank(
    data: &[u8],
    origin: usize,
    field: usize,
    count: usize,
    banks: &mut Vec<TextureBank>,
    replaced: &mut Vec<usize>,
) -> Result<(), String> {
    let at = origin
        .checked_add(read_u32(data, origin + field)? as usize)
        .ok_or("texture bank offset overflow")?;
    let mut records = Vec::with_capacity(count);
    for index in 0..count {
        let start = at + index * TEXTURE_BYTES;
        records.push(
            data.get(start..start + TEXTURE_BYTES)
                .ok_or("texture bank exceeds assembled assets")?
                .to_vec(),
        );
    }
    banks.push(TextureBank {
        origin,
        field,
        at,
        records,
    });
    replaced.push(origin + field);
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn add_stage(
    data: &[u8],
    origin: usize,
    far: bool,
    name: &str,
    banks: &mut Vec<TextureBank>,
    far_banks: &mut Vec<FarBank>,
    references: &mut Vec<(usize, usize, usize, usize)>,
    replaced: &mut Vec<usize>,
) -> Result<(), String> {
    let count = read_u32(data, origin + 8)? as usize;
    if count != 0 && far {
        let bank = origin + read_u32(data, origin + 36)? as usize;
        let packed = read_u16(data, origin + 186)? & PACKED_FAR_TEXTURES != 0;
        let mut pairs = Vec::with_capacity(count);
        for index in 0..count {
            let record = if packed {
                read_u32(data, bank + index * 4)?
            } else {
                (read_u32(data, origin + 36)? as usize + index * TEXTURE_BYTES) as u32
            };
            if let Some(offset) = far_texture_payload(record) {
                let at = origin + offset;
                let bytes = data.get(at..at + TEXTURE_BYTES).ok_or("far texture out of bounds")?;
                banks.push(TextureBank { origin, field: usize::MAX, at, records: vec![bytes.to_vec()] });
                pairs.push(Some((at, at + MIP_BYTES)));
            } else {
                pairs.push(None);
            }
        }
        let table = format!("texture_far_refs_{}", far_banks.len());
        far_banks.push(FarBank {
            origin,
            bank,
            pairs,
            table,
        });
        replaced.push(origin + 36);
    } else if count != 0 {
        add_bank(data, origin, 36, count, banks, replaced)?;
    }
    for offset in STAGE_POINTERS {
        let slot = origin + offset;
        if read_u32(data, slot)? != 0 && !replaced.contains(&slot) {
            push_reference(data, slot, origin, 0, references)?;
        }
    }
    for field in [188, 192] {
        let grid = read_u32(data, origin + field)? as usize;
        if grid != 0 {
            for offset in [20, 24] {
                let slot = origin + grid + offset;
                if read_u32(data, slot)? != 0 {
                    push_reference(data, slot, origin, 0, references)?;
                }
            }
        }
    }
    let gravities = origin + read_u32(data, origin + 76)? as usize;
    for index in 0..read_u32(data, origin + 72)? as usize {
        let entry = gravities + index * GRAVITY_BYTES;
        if data.get(entry) == Some(&WIRE_GRAVITY) && read_u32(data, entry + 44)? != 0 {
            push_reference(data, entry + 44, origin, 0, references)?;
        }
    }
    let _ = name;
    Ok(())
}

fn add_figure(
    data: &[u8],
    origin: usize,
    banks: &mut Vec<TextureBank>,
    references: &mut Vec<(usize, usize, usize, usize)>,
    replaced: &mut Vec<usize>,
) -> Result<(), String> {
    for (field, count_at) in [(24, 4), (48, 38)] {
        let count = read_u16(data, origin + count_at)? as usize;
        if count != 0 {
            add_bank(data, origin, field, count, banks, replaced)?;
        }
    }
    for offset in FIGURE_POINTERS {
        let slot = origin + offset;
        if read_u32(data, slot)? != 0 && !replaced.contains(&slot) {
            push_reference(data, slot, origin, 0, references)?;
        }
    }
    let keys = read_u16(data, origin + 6)?;
    if keys & 0x8000 != 0 {
        let table = origin + read_u32(data, origin + 12)? as usize;
        for key in 0..(keys & 0x7fff) as usize {
            let slot = table + key * 4;
            push_reference(data, slot, origin, 1, references)?;
            let descriptor = origin + (read_u32(data, slot)? & !1) as usize;
            push_reference(data, descriptor + 4, descriptor, 0, references)?;
        }
    }
    Ok(())
}

fn header_list(header: &str, name: &str) -> Result<Vec<String>, String> {
    let prefix = format!("#define {name}");
    let line = header
        .lines()
        .find(|line| line.starts_with(&prefix))
        .ok_or_else(|| format!("missing generated header list {name}"))?;
    let body = line
        .split_once('{')
        .and_then(|(_, rest)| rest.split_once('}').map(|(body, _)| body))
        .ok_or_else(|| format!("malformed generated header list {name}"))?;
    Ok(body
        .split(',')
        .map(str::trim)
        .filter(|entry| !entry.is_empty())
        .map(str::to_owned)
        .collect())
}

fn header_figures(header: &str) -> Vec<String> {
    header
        .lines()
        .filter_map(|line| {
            let name = line.strip_prefix("extern const uint8_t figure_")?;
            Some(format!("figure_{}", name.strip_suffix("[];")?))
        })
        .collect()
}

fn compact_mip1(mip: &[u8]) -> Result<Vec<u8>, String> {
    if mip.len() != MIP_BYTES {
        return Err("mip 1 has an invalid byte length".into());
    }
    let mut compact = Vec::with_capacity(512);
    for row in 0..16 {
        let row = &mip[row * 64..row * 64 + 64];
        if row[32..].iter().any(|byte| *byte != 0) {
            return Err("mip 1 must be 32x16 texels stored at a stride of 64".into());
        }
        compact.extend_from_slice(&row[..32]);
    }
    Ok(compact)
}

fn intern(table: &mut HashMap<Vec<u8>, usize>, values: &mut Vec<Vec<u8>>, value: &[u8]) -> usize {
    if let Some(index) = table.get(value) {
        *index
    } else {
        let index = values.len();
        let value = value.to_vec();
        table.insert(value.clone(), index);
        values.push(value);
        index
    }
}

fn overlap_units(units: &[Vec<u8>]) -> (Vec<u8>, Vec<usize>) {
    let count = units.len();
    let mut following: Vec<Option<(usize, usize)>> = vec![None; count];
    let mut preceding: Vec<Option<usize>> = vec![None; count];
    let mut chains = (0..count).collect::<Vec<_>>();
    fn chain_of(chains: &mut [usize], mut index: usize) -> usize {
        while chains[index] != index {
            chains[index] = chains[chains[index]];
            index = chains[index];
        }
        index
    }
    for step in 0..(MIP_BYTES / 4 - 1) {
        let size = MIP_BYTES - 4 - step * 4;
        let mut starts: HashMap<Vec<u8>, VecDeque<usize>> = HashMap::new();
        for (index, unit) in units.iter().enumerate() {
            if preceding[index].is_none() {
                starts
                    .entry(unit[..size].to_vec())
                    .or_default()
                    .push_back(index);
            }
        }
        for (x, unit) in units.iter().enumerate() {
            if following[x].is_some() {
                continue;
            }
            let Some(bucket) = starts.get_mut(&unit[unit.len() - size..]) else {
                continue;
            };
            while bucket
                .front()
                .is_some_and(|index| preceding[*index].is_some())
            {
                bucket.pop_front();
            }
            let y = bucket.iter().copied().find(|y| {
                preceding[*y].is_none() && chain_of(&mut chains, x) != chain_of(&mut chains, *y)
            });
            if let Some(y) = y {
                following[x] = Some((y, size));
                preceding[y] = Some(x);
                let root_x = chain_of(&mut chains, x);
                let root_y = chain_of(&mut chains, y);
                chains[root_y] = root_x;
            }
        }
    }
    let mut pool = Vec::new();
    let mut offsets = vec![0; count];
    for first in 0..count {
        if preceding[first].is_some() {
            continue;
        }
        let mut at = first;
        offsets[at] = pool.len();
        pool.extend_from_slice(&units[at]);
        while let Some((next, overlap)) = following[at] {
            offsets[next] = pool.len() - overlap;
            pool.extend_from_slice(&units[next][overlap..]);
            at = next;
        }
    }
    (pool, offsets)
}

fn build_mip_pool(banks: &[TextureBank]) -> Result<(Vec<u8>, Vec<Vec<(usize, usize)>>), String> {
    let mut mip0_map = HashMap::new();
    let mut mip0 = Vec::new();
    let mut mip1_map = HashMap::new();
    let mut mip1 = Vec::new();
    let mut bank_ids = Vec::new();
    for bank in banks {
        let mut ids = Vec::new();
        for record in &bank.records {
            if record.len() != TEXTURE_BYTES {
                return Err("texture record has an invalid byte length".into());
            }
            let id0 = intern(&mut mip0_map, &mut mip0, &record[..MIP_BYTES]);
            let compact = compact_mip1(&record[MIP_BYTES..])?;
            let id1 = intern(&mut mip1_map, &mut mip1, &compact);
            ids.push((id0, id1));
        }
        bank_ids.push(ids);
    }
    let mut units = mip0;
    if mip1.len() % 2 != 0 {
        mip1.push(vec![0; 512]);
    }
    for pair in mip1.chunks_exact(2) {
        let mut unit = Vec::with_capacity(MIP_BYTES);
        for row in 0..16 {
            unit.extend_from_slice(&pair[0][row * 32..row * 32 + 32]);
            unit.extend_from_slice(&pair[1][row * 32..row * 32 + 32]);
        }
        units.push(unit);
    }
    let mip0_count = units.len() - mip1.len() / 2;
    let (mut pool, unit_at) = overlap_units(&units);
    pool.extend_from_slice(&[0; 32]);
    let tables = bank_ids
        .into_iter()
        .map(|ids| {
            ids.into_iter()
                .map(|(id0, id1)| (unit_at[id0], unit_at[mip0_count + id1 / 2] + 32 * (id1 % 2)))
                .collect()
        })
        .collect();
    Ok((pool, tables))
}

fn retained_ranges(removed: &[(usize, usize)], end: usize) -> Vec<(usize, usize)> {
    let mut kept = Vec::new();
    let mut cursor = 0;
    for &(low, high) in removed.iter().chain(std::iter::once(&(end, end))) {
        if cursor < low {
            kept.push((cursor, low));
        }
        cursor = high;
    }
    kept
}

#[cfg(test)]
mod far_texture_record_tests {
    use super::*;
    #[test]
    fn far_record_decoding() {
        for offset in [0u32, 4, 0x100, 0x12340, 0x00ff_fffc] {
            assert_eq!(far_texture_payload(offset), Some(offset as usize));
            for average in 1..=255u32 {
                let legacy = FLAT_FAR_TEXTURE | average;
                assert_eq!(far_texture_payload(legacy), None);
                let detail = legacy | DETAILED_FAR_TEXTURE | ((offset / 4) << 8);
                assert_eq!(detail as u8, average as u8);
                assert_eq!(far_texture_payload(detail), Some(offset as usize));
            }
        }
    }
}

//! Canonical post-compiler asset packing, ported from `tools/assets/pack.py`.
//!
//! Packing works on the C compiler's assembly and its blobs before XIP
//! assembly. Each operation rewrites both together so pointer expressions keep
//! the same meaning after shared data is introduced.

use std::collections::BTreeMap;

pub(crate) const FIGURE_BYTES: usize = 64;
const TRIANGLE_BYTES: usize = 22;
const TEXTURE_BYTES: usize = 2048;
const STAGE_NAMES: [&str; 3] = [
    "stage_observatory",
    "stage_honeyhive",
    "stage_honeyhive_tower",
];
const STAGE_POINTERS: [(&str, usize); 26] = [
    ("points", 28),
    ("triangles", 32),
    ("textures", 36),
    ("chunks", 40),
    ("leaf_triangle_offsets", 44),
    ("leaf_triangles", 48),
    ("leaf_draw_ranks", 52),
    ("rank_triangles", 56),
    ("texture_water", 60),
    ("texture_scroll", 64),
    ("sky", 68),
    ("gravities", 76),
    ("collision_triangles", 84),
    ("collision_nodes", 92),
    ("objects", 100),
    ("starts", 108),
    ("rails", 116),
    ("rail_points", 120),
    ("rail_parts", 124),
    ("areas", 132),
    ("cameras", 140),
    ("point_home", 176),
    ("leaf_point_first", 180),
    ("gravity_grid", 188),
    ("area_grid", 192),
    ("parts", 200),
];
const GRAVITY_BYTES: usize = 80;
const WIRE_GRAVITY: u8 = 8;
const SKY_BYTES: usize = 128 * 256;
const STAGE_METADATA: [(&str, Option<usize>, usize); 7] = [
    ("objects", Some(96), 84),
    ("starts", Some(104), 32),
    ("areas", Some(128), 72),
    ("cameras", Some(136), 120),
    ("rails", Some(112), 32),
    ("rail_points", None, 28),
    ("rail_parts", None, 88),
];
const TICO_FIGURES: [&str; 6] = [
    "figure_tico",
    "figure_tico1",
    "figure_tico2",
    "figure_tico3",
    "figure_tico4",
    "figure_tico5",
];

#[derive(Clone)]
pub(crate) struct FigureHeader {
    fields: [u32; 14],
    half_fields: [u16; 4],
    extra_half_fields: [u16; 2],
}

impl FigureHeader {
    pub(crate) fn parse(data: &[u8]) -> Result<Self, String> {
        if data.len() < FIGURE_BYTES {
            return Err("figure header is truncated".into());
        }
        let mut half_fields = [0; 4];
        for (index, value) in half_fields.iter_mut().enumerate() {
            *value = u16_at(data, index * 2)?;
        }
        let extra_half_fields = [u16_at(data, 36)?, u16_at(data, 38)?];
        let mut fields = [0; 14];
        for (index, value) in fields.iter_mut().enumerate() {
            *value = u32_at(data, 8 + index * 4)?;
        }
        Ok(Self {
            fields,
            half_fields,
            extra_half_fields,
        })
    }

    pub(crate) fn point_count(&self) -> usize {
        self.half_fields[0] as usize
    }
    pub(crate) fn triangle_count(&self) -> usize {
        self.half_fields[1] as usize
    }
    pub(crate) fn texture_count(&self) -> usize {
        self.half_fields[2] as usize
    }
    pub(crate) fn keyframe_count(&self) -> usize {
        (self.half_fields[3] & 0x7fff) as usize
    }
    pub(crate) fn clip_count(&self) -> usize {
        self.fields[5] as usize
    }
    pub(crate) fn lod_triangle_count(&self) -> usize {
        self.extra_half_fields[0] as usize
    }
    pub(crate) fn lod_texture_count(&self) -> usize {
        self.extra_half_fields[1] as usize
    }
    pub(crate) fn lod_point_count(&self) -> usize {
        self.fields[11] as usize
    }
    pub(crate) fn extent(&self) -> u32 {
        self.fields[13]
    }

    pub(crate) fn pointer(&self, field_offset: usize) -> u32 {
        self.fields[(field_offset - 8) / 4]
    }

    fn set_pointer(&mut self, field_offset: usize, value: u32) {
        self.fields[(field_offset - 8) / 4] = value;
    }

    fn write(&self, output: &mut [u8]) {
        for (index, value) in self.half_fields.iter().enumerate() {
            output[index * 2..index * 2 + 2].copy_from_slice(&value.to_le_bytes());
        }
        for (index, value) in self.extra_half_fields.iter().enumerate() {
            let offset = 36 + index * 2;
            output[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
        }
        for (index, value) in self.fields.iter().enumerate() {
            output[8 + index * 4..12 + index * 4].copy_from_slice(&value.to_le_bytes());
        }
    }
}

pub(crate) fn u16_at(data: &[u8], offset: usize) -> Result<u16, String> {
    let bytes = data.get(offset..offset + 2).ok_or("truncated figure u16")?;
    Ok(u16::from_le_bytes([bytes[0], bytes[1]]))
}

pub(crate) fn u32_at(data: &[u8], offset: usize) -> Result<u32, String> {
    let bytes = data.get(offset..offset + 4).ok_or("truncated figure u32")?;
    Ok(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
}

fn put_u32(data: &mut [u8], offset: usize, value: u32) -> Result<(), String> {
    let bytes = data
        .get_mut(offset..offset + 4)
        .ok_or("figure pointer slot is out of bounds")?;
    bytes.copy_from_slice(&value.to_le_bytes());
    Ok(())
}

fn put_stage_u32(data: &mut [u8], offset: usize, value: u32) -> Result<(), String> {
    put_u32(data, offset, value)
}

fn align_stage_size(value: usize) -> usize {
    value.saturating_add(3) & !3
}

fn stage_pointer(data: &[u8], name: &str) -> Result<usize, String> {
    let offset = STAGE_POINTERS
        .iter()
        .find_map(|(field, offset)| (*field == name).then_some(*offset))
        .ok_or_else(|| format!("unknown stage pointer field {name}"))?;
    Ok(u32_at(data, offset)? as usize)
}

#[derive(Clone, Copy)]
struct SharedStageArray {
    slot: usize,
    at: usize,
    size: usize,
}

fn sky_arrays(data: &[u8]) -> Result<Vec<(String, SharedStageArray)>, String> {
    Ok(vec![(
        String::new(),
        SharedStageArray {
            slot: 68,
            at: stage_pointer(data, "sky")?,
            size: SKY_BYTES,
        },
    )])
}

fn metadata_arrays(data: &[u8]) -> Result<Vec<(String, SharedStageArray)>, String> {
    let mut arrays = Vec::new();
    for (kind, count_field, stride) in STAGE_METADATA {
        let count = if let Some(count_field) = count_field {
            u32_at(data, count_field)? as usize
        } else {
            let rails = stage_pointer(data, "rails")?;
            let rail_count = u32_at(data, 112)? as usize;
            let first = if kind == "rail_points" { 0 } else { 4 };
            let mut count = 0usize;
            for index in 0..rail_count {
                let at = rails + index * 32 + first;
                count = count.max(u16_at(data, at)? as usize + u16_at(data, at + 2)? as usize);
            }
            count
        };
        if count != 0 {
            arrays.push((
                kind.to_owned(),
                SharedStageArray {
                    slot: STAGE_POINTERS
                        .iter()
                        .find_map(|(field, offset)| (*field == kind).then_some(*offset))
                        .ok_or_else(|| format!("unknown metadata field {kind}"))?,
                    at: stage_pointer(data, kind)?,
                    size: count.checked_mul(stride).ok_or("metadata size overflow")?,
                },
            ));
        }
    }
    Ok(arrays)
}

fn grid_arrays(data: &[u8]) -> Result<Vec<(String, SharedStageArray)>, String> {
    let mut arrays = Vec::new();
    let gravity_count = u32_at(data, 72)? as usize;
    let gravities = stage_pointer(data, "gravities")?;
    if gravity_count != 0 {
        let mut has_wire = false;
        for index in 0..gravity_count {
            let at = gravities + index * GRAVITY_BYTES;
            let kind = *data.get(at).ok_or("truncated gravity record")?;
            if kind == WIRE_GRAVITY {
                has_wire = true;
                break;
            }
        }
        if !has_wire {
            arrays.push((
                "gravities".into(),
                SharedStageArray {
                    slot: 76,
                    at: gravities,
                    size: gravity_count * GRAVITY_BYTES,
                },
            ));
        }
    }

    for (field, name) in [("gravity_grid", "gravity"), ("area_grid", "area")] {
        let grid = stage_pointer(data, field)?;
        if grid == 0 {
            continue;
        }
        let cells = u32_at(data, grid + 20)? as usize;
        let lists = u32_at(data, grid + 24)? as usize;
        let outside = u16_at(data, grid + 28)? as usize;
        let top = u16_at(data, grid + 30)? as usize;
        let cell_prefix = data
            .get(cells..cells + top * 2)
            .ok_or("truncated grid cell prefix")?;
        let mut blocks = 0usize;
        for pair in cell_prefix.chunks_exact(2) {
            blocks = blocks.max(u16::from_le_bytes([pair[0], pair[1]]) as usize + 1);
        }
        let shift = 3usize
            .checked_mul(*data.get(grid + 19).ok_or("truncated grid block shift")? as usize)
            .ok_or("grid block shift overflow")?;
        let cell_words = top
            .checked_add(
                blocks
                    .checked_shl(shift as u32)
                    .ok_or("grid cell size overflow")?,
            )
            .ok_or("grid cell size overflow")?;
        let list_indices = data
            .get(cells + top * 2..cells + cell_words * 2)
            .ok_or("truncated grid cell lists")?;
        let mut list_end = outside + 1 + u16_at(data, lists + outside * 2)? as usize;
        for pair in list_indices.chunks_exact(2) {
            let index = u16::from_le_bytes([pair[0], pair[1]]) as usize;
            let count = u16_at(data, lists + index * 2)? as usize;
            list_end = list_end.max(index + 1 + count);
        }
        let list_words = list_end;
        arrays.push((
            format!("{name}_cells"),
            SharedStageArray {
                slot: grid + 20,
                at: cells,
                size: cell_words * 2,
            },
        ));
        arrays.push((
            format!("{name}_lists"),
            SharedStageArray {
                slot: grid + 24,
                at: lists,
                size: list_words * 2,
            },
        ));
    }
    arrays.sort_by(|left, right| left.0.cmp(&right.0));
    Ok(arrays)
}

fn stage_relocate(data: &mut [u8], threshold: usize, delta: isize) -> Result<(), String> {
    let move_pointer = |data: &mut [u8], offset: usize| -> Result<(), String> {
        let value = u32_at(data, offset)? as usize;
        if value >= threshold {
            let moved = (value as isize + delta) as u32;
            put_stage_u32(data, offset, moved)?;
        }
        Ok(())
    };
    for (_, offset) in STAGE_POINTERS {
        move_pointer(data, offset)?;
    }
    for field in ["gravity_grid", "area_grid"] {
        let grid = stage_pointer(data, field)?;
        if grid != 0 {
            move_pointer(data, grid + 20)?;
            move_pointer(data, grid + 24)?;
        }
    }
    let gravities = stage_pointer(data, "gravities")?;
    if gravities != 0 {
        for index in 0..u32_at(data, 72)? as usize {
            let entry = gravities + index * GRAVITY_BYTES;
            if data.get(entry) == Some(&WIRE_GRAVITY) {
                move_pointer(data, entry + 44)?;
            }
        }
    }
    Ok(())
}

pub(crate) fn align4(value: usize) -> usize {
    value.saturating_add(3) & !3
}

fn figure_array<'a>(data: &'a [u8], pointer: u32, size: usize) -> Result<&'a [u8], String> {
    let start = pointer as usize;
    data.get(
        start
            ..start
                .checked_add(size)
                .ok_or("figure array size overflow")?,
    )
    .ok_or_else(|| format!("figure array at {start} with size {size} is out of bounds"))
}

pub(crate) fn key_count(header: &FigureHeader) -> usize {
    header.keyframe_count()
}

pub(crate) fn descriptor_at(
    data: &[u8],
    header: &FigureHeader,
    key: usize,
) -> Result<usize, String> {
    let at = header.pointer(12) as usize + key * 4;
    Ok((u32_at(data, at)? & !1) as usize)
}

pub(crate) fn decode_pose(
    data: &[u8],
    at: usize,
    point_count: usize,
) -> Result<(Vec<u8>, usize), String> {
    let mode = u16_at(data, at + 2)?;
    let relative = i32::from_le_bytes(
        data.get(at + 4..at + 8)
            .ok_or("truncated figure pose descriptor")?
            .try_into()
            .unwrap(),
    );
    if mode == 0 {
        let end = at
            .checked_add(8 + point_count * 6)
            .ok_or("raw pose size overflow")?;
        let pose = data
            .get(at + 8..end)
            .ok_or("truncated raw figure pose")?
            .to_vec();
        return Ok((pose, align4(end)));
    }

    let anchor = (at as i64 + relative as i64)
        .try_into()
        .map_err(|_| "pose anchor is out of bounds")?;
    let bind_len = point_count
        .checked_mul(6)
        .ok_or("pose bind size overflow")?;
    let bind = data
        .get(anchor..anchor + bind_len)
        .ok_or("truncated figure pose anchor")?;
    let mut values = Vec::with_capacity(point_count * 3);
    let mut cursor = at + 8;
    for first in (0..point_count).step_by(32) {
        let low = [
            i16::from_le_bytes(data[cursor..cursor + 2].try_into().unwrap()) as i32,
            i16::from_le_bytes(data[cursor + 2..cursor + 4].try_into().unwrap()) as i32,
            i16::from_le_bytes(data[cursor + 4..cursor + 6].try_into().unwrap()) as i32,
        ];
        let packed = u16_at(data, cursor + 6)?;
        let scale = if packed & 0x8000 != 0 {
            2 + (mode >> 1)
        } else {
            0
        };
        let widths = [packed & 31, (packed >> 5) & 31, (packed >> 10) & 31];
        let points = (point_count - first).min(32);
        let total_bits = points * widths.iter().map(|width| *width as usize).sum::<usize>();
        let byte_count = total_bits.div_ceil(8);
        let bits = data
            .get(cursor + 8..cursor + 8 + byte_count)
            .ok_or("truncated packed figure pose")?;
        let mut bit_index = 0usize;
        for point in 0..points {
            for axis in 0..3 {
                let width = widths[axis] as usize;
                let mut value = 0i32;
                for bit in 0..width {
                    let position = bit_index + bit;
                    value |= (((bits[position / 8] >> (position % 8)) & 1) as i32) << bit;
                }
                bit_index += width;
                let bind_offset = ((first + point) * 3 + axis) * 2;
                let base =
                    i16::from_le_bytes(bind[bind_offset..bind_offset + 2].try_into().unwrap())
                        as i32;
                let result = base + low[axis] + (value << scale);
                values.push(result as i16);
            }
        }
        cursor += 8 + byte_count;
    }
    let mut pose = Vec::with_capacity(values.len() * 2);
    for value in values {
        pose.extend_from_slice(&value.to_le_bytes());
    }
    Ok((pose, align4(cursor)))
}

fn all_poses(data: &[u8], header: &FigureHeader) -> Result<Vec<Vec<u8>>, String> {
    (0..key_count(header))
        .map(|key| {
            decode_pose(
                data,
                descriptor_at(data, header, key)?,
                header.point_count(),
            )
            .map(|p| p.0)
        })
        .collect()
}

fn pose_descriptors(
    data: &[u8],
    header: &FigureHeader,
) -> Result<Vec<(Vec<u8>, usize, usize)>, String> {
    (0..key_count(header))
        .map(|key| {
            let at = descriptor_at(data, header, key)?;
            let (pose, end) = decode_pose(data, at, header.point_count())?;
            Ok((pose, at, end))
        })
        .collect()
}

fn remove_figure_ranges(
    name: &str,
    source: Vec<u8>,
    header: &FigureHeader,
    cuts: &[(usize, usize)],
    pointer_expressions: &BTreeMap<usize, String>,
    anchor_expression: Option<(usize, String)>,
) -> Result<(Vec<u8>, impl Fn(usize) -> usize, BTreeMap<usize, String>), String> {
    let mut cuts = cuts.to_vec();
    cuts.sort_unstable();
    for pair in cuts.windows(2) {
        if pair[0].0 + pair[0].1 > pair[1].0 {
            return Err(format!("overlapping pose descriptor ranges in {name}"));
        }
    }
    for &(at, size) in &cuts {
        if at.checked_add(size).is_none_or(|end| end > source.len()) {
            return Err(format!("pose descriptor range exceeds {name}.bin"));
        }
    }
    let moved_cuts = cuts.clone();
    let moved = move |at: usize| {
        at - moved_cuts
            .iter()
            .filter(|(start, size)| start + size <= at)
            .map(|(_, size)| *size)
            .sum::<usize>()
    };
    let mut output =
        Vec::with_capacity(source.len() - cuts.iter().map(|(_, size)| size).sum::<usize>());
    let mut words = BTreeMap::new();
    let mut cursor = 0;
    for &(at, size) in &cuts {
        output.extend_from_slice(&source[cursor..at]);
        cursor = at + size;
    }
    output.extend_from_slice(&source[cursor..]);

    for offset in [8, 12, 16, 20, 24, 32, 40, 44, 48, 56] {
        let pointer = header.pointer(offset) as usize;
        if pointer != 0 {
            if let Some(expression) = pointer_expressions.get(&offset) {
                put_u32(&mut output, moved(offset), 0)?;
                words.insert(moved(offset), format!("{expression} - {name}"));
            } else {
                put_u32(&mut output, moved(offset), moved(pointer) as u32)?;
            }
        }
    }
    let keyframes = header.pointer(12) as usize;
    let keys = key_count(header);
    let mut descriptors = BTreeMap::new();
    for key in 0..keys {
        let slot = keyframes + key * 4;
        let value = u32_at(&source, slot)?;
        let descriptor = (value & !1) as usize;
        if descriptor != 0 {
            put_u32(
                &mut output,
                moved(slot),
                moved(descriptor) as u32 | (value & 1),
            )?;
            descriptors.insert(descriptor, (value & 1) != 0);
        }
    }
    for (descriptor, _) in descriptors {
        let relative = i32::from_le_bytes(
            source[descriptor + 4..descriptor + 8]
                .try_into()
                .map_err(|_| "truncated pose anchor relocation")?,
        ) as isize;
        let target = (descriptor as isize + relative)
            .try_into()
            .map_err(|_| "pose anchor relocation is out of bounds")?;
        if let Some((anchor, expression)) = &anchor_expression {
            if target == *anchor {
                put_u32(&mut output, moved(descriptor) + 4, 0)?;
                words.insert(
                    moved(descriptor) + 4,
                    format!("{expression} - ({name} + {})", moved(descriptor)),
                );
                continue;
            }
        }
        let delta = moved(target) as isize - moved(descriptor) as isize;
        put_u32(&mut output, moved(descriptor) + 4, delta as i32 as u32)?;
    }
    Ok((output, moved, words))
}

fn tico_shape(
    data: &[u8],
    header: &FigureHeader,
) -> Result<(Vec<usize>, Vec<Vec<u8>>, Vec<Vec<u8>>, Vec<Vec<u8>>), String> {
    let counts = vec![
        header.point_count(),
        header.triangle_count(),
        header.keyframe_count(),
        header.clip_count(),
        header.lod_triangle_count(),
        header.lod_point_count(),
        header.extent() as usize,
    ];
    let arrays = [
        (8, header.point_count() * 6),
        (20, header.triangle_count() * 6),
        (32, header.clip_count() * 8),
        (44, header.lod_triangle_count() * 6),
        (56, header.lod_point_count() * 2),
    ]
    .into_iter()
    .map(|(field, size)| figure_array(data, header.pointer(field), size).map(<[u8]>::to_vec))
    .collect::<Result<Vec<_>, _>>()?;
    let mut vertices = Vec::new();
    for (field, count) in [
        (16, header.triangle_count()),
        (40, header.lod_triangle_count()),
    ] {
        let records = figure_array(data, header.pointer(field), count * TRIANGLE_BYTES)?;
        for record in records.chunks_exact(TRIANGLE_BYTES) {
            for vertex in 0..3 {
                vertices.push(record[vertex * 6..vertex * 6 + 2].to_vec());
            }
        }
    }
    Ok((counts, arrays, vertices, all_poses(data, header)?))
}

fn replace_once(text: &str, old: &str, new: &str) -> Result<String, String> {
    if text.matches(old).count() != 1 {
        return Err(format!("expected exactly one assembly fragment {old:?}"));
    }
    Ok(text.replacen(old, new, 1))
}

fn parse_incbin_line(line: &str) -> Option<(&str, usize, Option<usize>)> {
    let rest = line.strip_prefix(".incbin")?.trim();
    let rest = rest.strip_prefix('"')?;
    let close = rest.find('"')?;
    let filename = &rest[..close];
    let args = rest[close + 1..].trim();
    if args.is_empty() {
        return Some((filename, 0, None));
    }
    let args = args.strip_prefix(',')?;
    let mut args = args.split(',').map(str::trim);
    let start = args.next()?.parse().ok()?;
    let size = match args.next() {
        Some(value) => Some(value.parse().ok()?),
        None => None,
    };
    if args.next().is_some() {
        return None;
    }
    Some((filename, start, size))
}

fn parse_pool_base(expression: &str) -> Result<(String, usize), String> {
    let mut fields = expression.split(" + ");
    let label = fields.next().ok_or("empty pool pointer expression")?;
    if label.is_empty() || label.chars().any(|ch| matches!(ch, '(' | ')' | '-')) {
        return Err(format!("unsupported pool pointer expression {expression}"));
    }
    let offset = match fields.next() {
        Some(value) => value
            .parse::<usize>()
            .map_err(|_| format!("unsupported pool pointer expression {expression}"))?,
        None => 0,
    };
    if fields.next().is_some() {
        return Err(format!("unsupported pool pointer expression {expression}"));
    }
    Ok((label.to_owned(), offset))
}

fn replace_word_boundary(text: &str, pattern: &str, replacement: &str) -> String {
    let mut output = String::with_capacity(text.len());
    let mut cursor = 0;
    for (at, _) in text.match_indices(pattern) {
        let before_word = text[..at]
            .chars()
            .next_back()
            .is_some_and(|ch| ch.is_ascii_alphanumeric() || ch == '_');
        let end = at + pattern.len();
        let after_word = text[end..]
            .chars()
            .next()
            .is_some_and(|ch| ch.is_ascii_alphanumeric() || ch == '_');
        if before_word || after_word || at < cursor {
            continue;
        }
        output.push_str(&text[cursor..at]);
        output.push_str(replacement);
        cursor = end;
    }
    output.push_str(&text[cursor..]);
    output
}

fn find_body(text: &str, name: &str) -> Result<(usize, usize, BTreeMap<usize, String>), String> {
    let label = format!("{name}:\n");
    let label_at = text
        .find(&label)
        .ok_or_else(|| format!("assembly has no label {name}"))?;
    let start = label_at + label.len();
    let mut end = start;
    let mut cursor = 0usize;
    let mut words = BTreeMap::new();
    for raw_line in text[start..].split_inclusive('\n') {
        let line = raw_line.trim();
        if let Some((filename, offset, size)) = parse_incbin_line(line) {
            if filename != format!("{name}.bin") {
                break;
            }
            cursor = offset + size.unwrap_or(0);
        } else if let Some(expression) = line.strip_prefix(".word ") {
            words.insert(cursor, expression.trim().to_owned());
            cursor += 4;
        } else {
            break;
        }
        end += raw_line.len();
    }
    Ok((start, end, words))
}

fn emit_body(name: &str, length: usize, words: &BTreeMap<usize, String>) -> String {
    let mut output = String::new();
    let mut cursor = 0;
    for (&at, expression) in words {
        if at > cursor {
            output.push_str(&format!(
                "    .incbin \"{name}.bin\", {cursor}, {}\n",
                at - cursor
            ));
        }
        output.push_str(&format!("    .word {expression}\n"));
        cursor = at + 4;
    }
    if cursor < length {
        output.push_str(&format!("    .incbin \"{name}.bin\", {cursor}\n"));
    }
    output
}

fn pool_section(label: &str) -> String {
    format!(
        "\n    .section .rodata.assets, \"a\", %progbits\n    .global {label}\n    .balign 4\n{label}:\n    .incbin \"{label}.bin\"\n"
    )
}

fn find_stage_arrays(
    data: &[u8],
    mode: StageArrayMode,
) -> Result<Vec<(String, SharedStageArray)>, String> {
    match mode {
        StageArrayMode::Sky => sky_arrays(data),
        StageArrayMode::Metadata => metadata_arrays(data),
        StageArrayMode::Grid => grid_arrays(data),
    }
}

#[derive(Clone, Copy)]
enum StageArrayMode {
    Sky,
    Metadata,
    Grid,
}

/// Input/output buffers from the canonical asset compiler.
pub struct PackedAssets {
    pub text: String,
    pub header: String,
    pub blobs: BTreeMap<String, Vec<u8>>,
}

impl PackedAssets {
    /// First canonical pass: share the Tico animation bank.
    pub fn share_tico_bank(&mut self) -> Result<(), String> {
        let sources = TICO_FIGURES
            .iter()
            .map(|name| {
                self.blobs
                    .get(&format!("{name}.bin"))
                    .cloned()
                    .ok_or_else(|| format!("missing compiler blob {name}.bin"))
            })
            .collect::<Result<Vec<_>, _>>()?;
        let headers = sources
            .iter()
            .map(|data| FigureHeader::parse(data))
            .collect::<Result<Vec<_>, _>>()?;
        let base = &sources[0];
        let base_header = &headers[0];
        let base_shape = tico_shape(base, base_header)?;
        for index in 1..sources.len() {
            let header = &headers[index];
            let data = &sources[index];
            let aliased = header.lod_texture_count() == header.texture_count()
                && header.pointer(48) == header.pointer(24);
            if !aliased || tico_shape(data, header)? != base_shape {
                return Err(format!(
                    "{} does not share its shape and poses with figure_tico",
                    TICO_FIGURES[index]
                ));
            }
        }

        let keys = key_count(base_header);
        let shift = (sources.len() - 1) * FIGURE_BYTES;
        let mut bank = vec![0; sources.len() * FIGURE_BYTES];
        bank.extend_from_slice(&base[FIGURE_BYTES..]);
        for (variant, (data, parsed)) in sources.iter().zip(&headers).enumerate() {
            let origin = variant * FIGURE_BYTES;
            let mut header = parsed.clone();
            for offset in [8, 12, 16, 20, 24, 32, 40, 44, 48, 56] {
                let pointer = base_header.pointer(offset);
                header.set_pointer(
                    offset,
                    if pointer == 0 {
                        0
                    } else {
                        (pointer as usize + shift - origin) as u32
                    },
                );
            }
            if variant != 0 {
                for (field, size) in [
                    (16, header.triangle_count() * TRIANGLE_BYTES),
                    (24, header.texture_count() * TEXTURE_BYTES),
                    (40, header.lod_triangle_count() * TRIANGLE_BYTES),
                ] {
                    while bank.len() & 3 != 0 {
                        bank.push(0);
                    }
                    let relative = bank.len() - origin;
                    let source = figure_array(data, parsed.pointer(field), size)?;
                    header.set_pointer(field, relative as u32);
                    bank.extend_from_slice(source);
                }
                header.set_pointer(48, header.pointer(24));
                while bank.len() & 3 != 0 {
                    bank.push(0);
                }
                let relative = bank.len() - origin;
                header.set_pointer(12, relative as u32);
                bank.resize(bank.len() + keys * 4, 0);
            }
            let table = header.pointer(12) as usize + origin;
            for key in 0..keys {
                let descriptor =
                    (u32_at(base, base_header.pointer(12) as usize + key * 4)? & !1) as usize;
                let value = descriptor + shift - origin;
                put_u32(&mut bank, table + key * 4, (value as u32) | 1)?;
            }
            header.write(&mut bank[origin..origin + FIGURE_BYTES]);
        }

        self.blobs.insert("tico_shared_bank.bin".into(), bank);
        for (index, name) in TICO_FIGURES.iter().enumerate() {
            let old =
                format!("    .global {name}\n    .balign 4\n{name}:\n    .incbin \"{name}.bin\"\n");
            let new = if index == 0 {
                "    .global figure_tico\n    .balign 4\nfigure_tico:\n    .incbin \"tico_shared_bank.bin\"\n".to_owned()
            } else {
                format!(
                    "    .global {name}\n    .set {name}, figure_tico + {}\n",
                    index * FIGURE_BYTES
                )
            };
            self.text = replace_once(&self.text, &old, &new)?;
        }
        Ok(())
    }

    /// Share byte-identical stage arrays and update their relative pointers.
    fn share_stage_arrays(
        &mut self,
        names: &[&str],
        prefix: &str,
        mode: StageArrayMode,
    ) -> Result<(), String> {
        let blobs = names
            .iter()
            .map(|name| {
                self.blobs
                    .get(&format!("{name}.bin"))
                    .cloned()
                    .map(|bytes| ((*name).to_owned(), bytes))
                    .ok_or_else(|| format!("missing compiler stage blob {name}.bin"))
            })
            .collect::<Result<BTreeMap<_, _>, _>>()?;
        let found = names
            .iter()
            .map(|name| Ok((*name, find_stage_arrays(blobs.get(*name).unwrap(), mode)?)))
            .collect::<Result<Vec<_>, String>>()?;
        let mut cuts: BTreeMap<&str, Vec<(usize, usize)>> =
            names.iter().map(|name| (*name, Vec::new())).collect();
        let mut slots: BTreeMap<&str, Vec<(usize, String)>> =
            names.iter().map(|name| (*name, Vec::new())).collect();
        let mut labels = Vec::new();

        for (kind, _) in &found[0].1 {
            let rows = found
                .iter()
                .map(|(_, arrays)| arrays.iter().find(|(candidate, _)| candidate == kind))
                .collect::<Vec<_>>();
            if rows.iter().any(Option::is_none) {
                continue;
            }
            let rows = rows.into_iter().map(Option::unwrap).collect::<Vec<_>>();
            let first = rows[0].1;
            let first_data = &blobs[names[0]][first.at..first.at + first.size];
            let equal = rows.iter().zip(names).all(|(row, name)| {
                let array = row.1;
                blobs[*name][array.at..array.at + array.size] == *first_data
            });
            if !equal {
                continue;
            }
            let label = format!("{prefix}{kind}");
            self.blobs
                .insert(format!("{label}.bin"), first_data.to_vec());
            labels.push(label.clone());
            for (name, row) in names.iter().zip(rows.iter()) {
                let array = row.1;
                cuts.get_mut(name)
                    .unwrap()
                    .push((array.at, align_stage_size(array.size)));
                slots
                    .get_mut(name)
                    .unwrap()
                    .push((array.slot, format!("{label} - {name}")));
            }
        }

        for name in names {
            let source = &blobs[*name];
            let mut output = source.clone();
            for (slot, _) in &slots[name] {
                put_stage_u32(&mut output, *slot, 0)?;
            }
            cuts.get_mut(name)
                .unwrap()
                .sort_unstable_by(|a, b| b.cmp(a));
            for &(at, size) in &cuts[name] {
                let end = at.checked_add(size).ok_or("stage cut overflow")?;
                if end > output.len() {
                    return Err(format!("stage cut {at}..{end} exceeds {name}.bin"));
                }
                let mut shortened = Vec::with_capacity(output.len() - size);
                shortened.extend_from_slice(&output[..at]);
                shortened.extend_from_slice(&output[end..]);
                stage_relocate(&mut shortened, end, -(size as isize))?;
                output = shortened;
            }

            let (body_start, body_end, mut words) = find_body(&self.text, name)?;
            for (slot, expression) in &slots[name] {
                words.insert(*slot, expression.clone());
            }
            let mut moved_words = BTreeMap::new();
            for (slot, expression) in words {
                let removed = cuts[name]
                    .iter()
                    .map(|(at, size)| if at + size <= slot { *size } else { 0 })
                    .sum::<usize>();
                moved_words.insert(slot.saturating_sub(removed), expression);
            }
            let body = emit_body(name, output.len(), &moved_words);
            self.text.replace_range(body_start..body_end, &body);
            self.blobs.insert(format!("{name}.bin"), output);
        }
        for label in labels {
            self.text.push_str(&pool_section(&label));
        }
        Ok(())
    }

    /// Run the stage-array sharing passes in the canonical `pack.py` order.
    pub fn share_canonical_stage_arrays(&mut self) -> Result<(), String> {
        self.share_stage_arrays(&STAGE_NAMES, "stage_sky_shared_0", StageArrayMode::Sky)?;
        self.share_stage_arrays(
            &["stage_honeyhive", "stage_honeyhive_tower"],
            "stage_metadata_shared_0_",
            StageArrayMode::Metadata,
        )?;
        self.share_stage_arrays(
            &["stage_honeyhive", "stage_honeyhive_tower"],
            "stage_grid_shared_0_",
            StageArrayMode::Grid,
        )?;
        Ok(())
    }

    /// Share identical texture records between the three canonical figure pairs.
    pub fn share_canonical_texture_overlaps(&mut self) -> Result<(), String> {
        let pairs = [
            ("figure_plant", "figure_planttop"),
            ("figure_broadbeanmovestepa", "figure_broadbeanmovestepb"),
            ("figure_powerstar", "figure_powerstarempty"),
        ];
        for (number, (first_name, second_name)) in pairs.into_iter().enumerate() {
            self.share_texture_overlap(number, first_name, second_name)?;
        }
        Ok(())
    }

    /// Pool identical pose descriptors shared by the canonical figure pairs.
    pub fn share_canonical_pose_descriptors(&mut self) -> Result<(), String> {
        self.share_pose_descriptors("figure_ghostmario", "figure_mario", 0)?;
        self.share_pose_descriptors("figure_flowerblue", "figure_flower", 1)?;
        Ok(())
    }

    /// Share the canonical whole bind/axis arrays and contained plant textures.
    pub fn share_canonical_whole_arrays(&mut self) -> Result<(), String> {
        let groups: [(&str, &[&str]); 8] = [
            (
                "axes",
                &["figure_mario", "figure_mariofire", "figure_marioice"],
            ),
            (
                "bind",
                &["figure_mario", "figure_mariofire", "figure_marioice"],
            ),
            (
                "axes",
                &[
                    "figure_starbit0",
                    "figure_starbit1",
                    "figure_starbit2",
                    "figure_starbit3",
                    "figure_starbit4",
                    "figure_starbit5",
                ],
            ),
            (
                "bind",
                &[
                    "figure_starbit0",
                    "figure_starbit1",
                    "figure_starbit2",
                    "figure_starbit3",
                    "figure_starbit4",
                    "figure_starbit5",
                ],
            ),
            ("axes", &["figure_coin", "figure_purplecoin"]),
            ("bind", &["figure_coin", "figure_purplecoin"]),
            ("axes", &["figure_bluechip", "figure_yellowchip"]),
            ("axes", &["figure_launchstar", "figure_slingstar"]),
        ];
        let mut plans: BTreeMap<
            String,
            (
                Vec<(usize, usize)>,
                BTreeMap<usize, String>,
                Option<(usize, String)>,
            ),
        > = BTreeMap::new();
        let mut pools = Vec::new();
        let mut shared_binds = Vec::new();
        for (number, (kind, names)) in groups.iter().enumerate() {
            let (field, count_offset) = if *kind == "bind" { (8, 0) } else { (20, 2) };
            let mut arrays = Vec::new();
            for name in *names {
                let data = self
                    .blobs
                    .get(&format!("{name}.bin"))
                    .ok_or_else(|| format!("missing compiler blob {name}.bin"))?;
                let header = FigureHeader::parse(data)?;
                let count = if count_offset == 0 {
                    header.point_count()
                } else {
                    header.triangle_count()
                };
                let bytes = figure_array(data, header.pointer(field), count * 6)?.to_vec();
                arrays.push((name.to_string(), header, bytes));
            }
            if arrays.iter().any(|(_, _, bytes)| bytes != &arrays[0].2) {
                return Err(format!("{} arrays differ in {}", kind, names.join(", ")));
            }
            let label = format!("figure_whole_array_shared_{number}");
            pools.push((label.clone(), arrays[0].2.clone()));
            for (name, header, bytes) in arrays {
                let at = header.pointer(field) as usize;
                let plan = plans
                    .entry(name.clone())
                    .or_insert_with(|| (Vec::new(), BTreeMap::new(), None));
                plan.0.push((at, bytes.len()));
                plan.1.insert(field, label.clone());
                if *kind == "bind" {
                    plan.2 = Some((at, label.clone()));
                    shared_binds.push((name.clone(), header.pointer(8), label.clone()));
                }
            }
        }

        // figure_plantstalk's texture run is contained in figure_plant's bank.
        let contained = self.contained_plant_texture_expression()?;
        let target = "figure_plantstalk";
        let target_data = self
            .blobs
            .get(&format!("{target}.bin"))
            .ok_or("missing compiler blob figure_plantstalk.bin")?;
        let target_header = FigureHeader::parse(target_data)?;
        let texture_size = target_header.texture_count() * TEXTURE_BYTES;
        let texture_at = target_header.pointer(24) as usize;
        plans
            .entry(target.to_owned())
            .or_insert_with(|| (Vec::new(), BTreeMap::new(), None))
            .0
            .push((texture_at, texture_size));
        let target_plan = plans.get_mut(target).unwrap();
        target_plan.1.insert(24, contained.clone());
        if target_header.lod_triangle_count() != 0 {
            target_plan.1.insert(48, contained);
        }

        for (name, (cuts, pointer_expressions, bind_expression)) in plans {
            let source = self
                .blobs
                .get(&format!("{name}.bin"))
                .cloned()
                .ok_or_else(|| format!("missing compiler blob {name}.bin"))?;
            let header = FigureHeader::parse(&source)?;
            let (output, moved, mut words) = remove_figure_ranges(
                &name,
                source,
                &header,
                &cuts,
                &pointer_expressions,
                bind_expression,
            )?;
            let (_, _, old_words) = find_body(&self.text, &name)?;
            for (slot, expression) in old_words {
                if !cuts
                    .iter()
                    .any(|(start, size)| *start <= slot && slot < *start + *size)
                {
                    words.insert(moved(slot), expression);
                }
            }
            self.replace_figure_blob(&name, output, words)?;
        }
        for (name, original_bind, shared_label) in shared_binds {
            let pattern = format!("{name} + {original_bind}");
            self.text = replace_word_boundary(&self.text, &pattern, &shared_label);
        }
        for (label, bytes) in pools {
            self.blobs.insert(format!("{label}.bin"), bytes);
            self.text.push_str(&pool_section(&label));
        }
        Ok(())
    }

    /// Deduplicate all texture mips and retain only non-texture asset bytes.
    pub fn pool_canonical_textures(&mut self) -> Result<(), String> {
        crate::texture_packer::pool_textures(&mut self.text, &mut self.header, &mut self.blobs)
    }

    /// Reanchor packed animation poses against cheaper shared bind poses.
    pub fn reanchor_canonical_poses(&mut self) -> Result<(), String> {
        let figures = self
            .header
            .lines()
            .filter_map(|line| {
                let name = line.strip_prefix("extern const uint8_t figure_")?;
                Some(format!("figure_{}", name.strip_suffix("[];")?))
            })
            .collect::<Vec<_>>();
        crate::pose_reanchor::reanchor(&mut self.text, &mut self.blobs, &figures)
    }

    fn contained_plant_texture_expression(&self) -> Result<String, String> {
        let donor = "figure_plant";
        let target = "figure_plantstalk";
        let donor_data = self
            .blobs
            .get(&format!("{donor}.bin"))
            .ok_or("missing compiler blob figure_plant.bin")?;
        let target_data = self
            .blobs
            .get(&format!("{target}.bin"))
            .ok_or("missing compiler blob figure_plantstalk.bin")?;
        let donor_header = FigureHeader::parse(donor_data)?;
        let target_header = FigureHeader::parse(target_data)?;
        let size = target_header.texture_count() * TEXTURE_BYTES;
        let target_bytes = figure_array(target_data, target_header.pointer(24), size)?;
        let (_, _, donor_words) = find_body(&self.text, donor)?;
        let pointer = if let Some(expression) = donor_words.get(&24) {
            let (base, _) = expression
                .rsplit_once(" - ")
                .ok_or("unsupported donor texture pointer expression")?;
            base.to_owned()
        } else {
            format!("{donor} + {}", donor_header.pointer(24))
        };
        let (label, base_offset) = parse_pool_base(&pointer)?;
        let bank = self
            .blobs
            .get(&format!("{label}.bin"))
            .ok_or_else(|| format!("missing texture pool blob {label}.bin"))?;
        let bank = bank
            .get(base_offset..)
            .ok_or("donor texture pointer exceeds its bank")?;
        let matches = (0..=bank.len().saturating_sub(size))
            .step_by(TEXTURE_BYTES)
            .filter(|at| bank[*at..*at + size] == *target_bytes)
            .collect::<Vec<_>>();
        let match_at = *matches
            .first()
            .ok_or("figure_plant texture pool does not contain figure_plantstalk")?;
        let absolute_base = if let Some(expression) = donor_words.get(&24) {
            format!("({expression}) + {donor}")
        } else {
            pointer
        };
        Ok(format!("{absolute_base} + {match_at}"))
    }

    fn share_pose_descriptors(
        &mut self,
        target: &str,
        donor: &str,
        group: usize,
    ) -> Result<(), String> {
        let target_data = self
            .blobs
            .get(&format!("{target}.bin"))
            .cloned()
            .ok_or_else(|| format!("missing compiler blob {target}.bin"))?;
        let donor_data = self
            .blobs
            .get(&format!("{donor}.bin"))
            .cloned()
            .ok_or_else(|| format!("missing compiler blob {donor}.bin"))?;
        let target_header = FigureHeader::parse(&target_data)?;
        let donor_header = FigureHeader::parse(&donor_data)?;
        if target_header.point_count() != donor_header.point_count() {
            return Err(format!("{target} and {donor} have different point counts"));
        }
        let donor_poses = pose_descriptors(&donor_data, &donor_header)?;
        let target_poses = pose_descriptors(&target_data, &target_header)?;
        let mut common = target_poses
            .iter()
            .filter_map(|(pose, _, _)| {
                donor_poses
                    .iter()
                    .any(|(candidate, _, _)| candidate == pose)
                    .then_some(pose.clone())
            })
            .collect::<Vec<_>>();
        common.sort();
        common.dedup();
        if common.is_empty() {
            return Err(format!("{target} shares no poses with {donor}"));
        }
        let labels = common
            .iter()
            .enumerate()
            .map(|(index, pose)| (pose.clone(), format!("pose_shared_{group}_{index}")))
            .collect::<BTreeMap<_, _>>();

        for (name, source, header, poses) in [
            (target, target_data.as_slice(), &target_header, target_poses),
            (
                donor,
                donor_data.as_slice(),
                &donor_header,
                donor_poses.clone(),
            ),
        ] {
            let mut output = source.to_vec();
            let mut cuts = Vec::new();
            let mut shared_slots = BTreeMap::new();
            for (key, (pose, at, end)) in poses.iter().enumerate() {
                if let Some(label) = labels.get(pose) {
                    cuts.push((*at, end - at));
                    let slot = header.pointer(12) as usize + key * 4;
                    put_u32(&mut output, slot, 0)?;
                    shared_slots.insert(slot, label.clone());
                }
            }
            cuts.sort_unstable();
            cuts.dedup();
            let (mut output, moved, mut words) =
                remove_figure_ranges(name, output, header, &cuts, &BTreeMap::new(), None)?;
            words.extend(
                shared_slots
                    .into_iter()
                    .map(|(slot, label)| (moved(slot), format!("{label} - {name} + 1"))),
            );
            self.replace_figure_blob(name, std::mem::take(&mut output), words)?;
        }

        let donor_bind = FigureHeader::parse(
            self.blobs
                .get(&format!("{donor}.bin"))
                .ok_or_else(|| format!("missing packed donor blob {donor}.bin"))?,
        )?
        .pointer(8);
        let mut pool = if group == 0 {
            String::from("\n    .section .rodata.assets, \"a\", %progbits\n")
        } else {
            String::new()
        };
        for pose in common {
            let label = &labels[&pose];
            let (_, at, end) = donor_poses
                .iter()
                .find(|(candidate, _, _)| candidate == &pose)
                .ok_or("shared pose missing from donor")?;
            self.blobs
                .insert(format!("{label}.bin"), donor_data[*at..*end].to_vec());
            pool.push_str(&format!(
                "    .balign 4\n{label}:\n    .incbin \"{label}.bin\", 0, 4\n    .word {donor} + {donor_bind} - {label}\n    .incbin \"{label}.bin\", 8\n"
            ));
        }
        self.text.push_str(&pool);
        Ok(())
    }

    fn share_texture_overlap(
        &mut self,
        number: usize,
        first_name: &str,
        second_name: &str,
    ) -> Result<(), String> {
        let names = [first_name, second_name];
        let sources = names
            .iter()
            .map(|name| {
                self.blobs
                    .get(&format!("{name}.bin"))
                    .cloned()
                    .ok_or_else(|| format!("missing compiler blob {name}.bin"))
            })
            .collect::<Result<Vec<_>, _>>()?;
        let headers = sources
            .iter()
            .map(|data| FigureHeader::parse(data))
            .collect::<Result<Vec<_>, _>>()?;
        let textures = sources
            .iter()
            .zip(&headers)
            .zip(names)
            .map(|((data, header), name)| {
                if header.keyframe_count() != 0 || header.clip_count() != 0 {
                    return Err(format!("{name} is not a static figure"));
                }
                if header.lod_triangle_count() != 0
                    && (header.lod_texture_count() != header.texture_count()
                        || header.pointer(48) != header.pointer(24))
                {
                    return Err(format!("{name} has a separate LOD texture bank"));
                }
                let bytes = figure_array(
                    data,
                    header.pointer(24),
                    header.texture_count() * TEXTURE_BYTES,
                )?;
                let records = bytes.chunks_exact(TEXTURE_BYTES).collect::<Vec<_>>();
                for i in 0..records.len() {
                    if records[..i].contains(&records[i]) {
                        return Err(format!("{name} has duplicate textures"));
                    }
                }
                Ok(records)
            })
            .collect::<Result<Vec<_>, String>>()?;

        let common = textures[0]
            .iter()
            .copied()
            .filter(|record| textures[1].contains(record))
            .collect::<Vec<_>>();
        let mut records = textures[0]
            .iter()
            .copied()
            .filter(|record| !common.contains(record))
            .collect::<Vec<_>>();
        records.extend(common.iter().copied());
        records.extend(
            textures[1]
                .iter()
                .copied()
                .filter(|record| !common.contains(record)),
        );
        let starts = [0, textures[0].len() - common.len()];
        let label = format!("figure_texture_buoy_overlap_bank_{number}");
        for index in 0..2 {
            let name = names[index];
            let source = &sources[index];
            let header = &headers[index];
            let view = &records[starts[index]..starts[index] + textures[index].len()];
            let mapping = textures[index]
                .iter()
                .map(|record| {
                    view.iter()
                        .position(|candidate| candidate == record)
                        .ok_or_else(|| format!("texture mapping missing in {name}"))
                })
                .collect::<Result<Vec<_>, _>>()?;
            let mut output = source.clone();
            for (field, count) in [
                (16, header.triangle_count()),
                (40, header.lod_triangle_count()),
            ] {
                let at = header.pointer(field) as usize;
                for triangle in 0..count {
                    let slot = at + triangle * TRIANGLE_BYTES + 18;
                    let old = u16_at(&output, slot)? as usize;
                    let mapped = *mapping
                        .get(old)
                        .ok_or_else(|| format!("texture index {old} is out of range in {name}"))?;
                    let bytes = output
                        .get_mut(slot..slot + 2)
                        .ok_or("triangle texture index is out of bounds")?;
                    bytes.copy_from_slice(&(mapped as u16).to_le_bytes());
                }
            }
            let cut_at = header.pointer(24) as usize;
            let cut_size = header.texture_count() * TEXTURE_BYTES;
            let cut_end = cut_at.checked_add(cut_size).ok_or("texture cut overflow")?;
            if cut_end > output.len() {
                return Err(format!("texture bank exceeds {name}.bin"));
            }
            output.drain(cut_at..cut_end);
            for field in [8, 12, 16, 20, 24, 32, 40, 44, 48, 56] {
                if field == 24 || field == 48 {
                    continue;
                }
                let pointer = u32_at(&output, field)? as usize;
                if pointer >= cut_end {
                    put_u32(&mut output, field, (pointer - cut_size) as u32)?;
                }
            }
            let mut words = BTreeMap::new();
            let expression = format!("{label} + {} - {name}", starts[index] * TEXTURE_BYTES);
            words.insert(24, expression.clone());
            if header.lod_triangle_count() != 0 {
                words.insert(48, expression);
            }
            self.replace_figure_blob(name, output, words)?;
        }

        let mut bank = Vec::with_capacity(records.len() * TEXTURE_BYTES);
        for record in records {
            bank.extend_from_slice(record);
        }
        self.blobs.insert(format!("{label}.bin"), bank);
        self.text.push_str(&format!(
            "\n    .section .rodata.assets.shared_textures, \"a\", %progbits\n    .global {label}\n    .balign 4\n{label}:\n    .incbin \"{label}.bin\"\n"
        ));
        Ok(())
    }

    fn replace_figure_blob(
        &mut self,
        name: &str,
        data: Vec<u8>,
        words: BTreeMap<usize, String>,
    ) -> Result<(), String> {
        let (start, end, _) = find_body(&self.text, name)?;
        let body = emit_body(name, data.len(), &words);
        self.text.replace_range(start..end, &body);
        self.blobs.insert(format!("{name}.bin"), data);
        Ok(())
    }
}

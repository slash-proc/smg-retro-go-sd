//! Pose reanchoring pass ported from `tools/assets/poses.py`.

use crate::asset_assembler::{AssemblyLayout, AssemblyStatementKind};
use crate::asset_packer::{
    align4, decode_pose, key_count, u16_at, u32_at, FigureHeader, FIGURE_BYTES,
};
use std::collections::{HashMap, HashSet};

const MAX_POINTS: usize = 224;
const MAX_SAMPLES: usize = 128;
const MAX_NEW_ANCHORS: usize = 32;
const UNUSABLE: u32 = u32::MAX;

#[derive(Clone)]
struct Key {
    pose: Vec<u8>,
    span: (usize, usize),
    mode: u16,
    anchor: Option<usize>,
}

struct Model {
    name: String,
    origin: usize,
    header: FigureHeader,
    keys: Vec<Key>,
    table: (usize, usize),
    bind: Vec<u8>,
    bind_at: usize,
    clips: Vec<u8>,
}

#[derive(Clone)]
struct Anchor {
    pose: Vec<u8>,
    at: Option<usize>,
    charge: usize,
    kind: AnchorKind,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum AnchorKind {
    Bind,
    RawDescriptor,
    New,
}

struct Plan {
    rows: Vec<Key>,
    row_of: HashMap<(usize, usize), usize>,
    anchors: Vec<Anchor>,
    choice: Vec<Option<usize>>,
    kept: Vec<usize>,
}

pub fn reanchor(
    text: &mut String,
    blobs: &mut std::collections::BTreeMap<String, Vec<u8>>,
    figure_names: &[String],
) -> Result<(), String> {
    let assembly = crate::asset_assembler::assemble_layout(text, 0, |name| {
        blobs
            .get(name)
            .cloned()
            .ok_or_else(|| format!("missing asset compiler blob {name}"))
    })?;
    let models = read_models(&assembly, figure_names)?;
    let mut labels_at: HashMap<usize, Vec<String>> = HashMap::new();
    for (name, at) in &assembly.symbols {
        labels_at.entry(*at).or_default().push(name.clone());
    }
    for names in labels_at.values_mut() {
        names.sort();
    }
    let mut extra_labels = HashMap::new();
    let mut cuts = Vec::new();
    let mut patches = HashMap::new();
    let mut replacements = HashMap::new();
    let mut pool_lines = Vec::new();

    for (number, group) in groups_sharing_data(&models).into_iter().enumerate() {
        let Some(plan) = plan_group(&group, &assembly)? else {
            continue;
        };
        let points = group[0].header.point_count();
        let prefix = format!("pose_anchor_{number}");
        let mut writer = PoolWriter::new(&prefix);
        let mut anchor_labels = HashMap::new();
        for &index in &plan.kept {
            let label = format!("{prefix}_raw_{index}");
            if plan.anchors[index].kind == AnchorKind::RawDescriptor {
                let mut descriptor = Vec::with_capacity(8 + plan.anchors[index].pose.len());
                descriptor.extend_from_slice(&(points as u16).to_le_bytes());
                descriptor.extend_from_slice(&0u16.to_le_bytes());
                descriptor.extend_from_slice(&0i32.to_le_bytes());
                descriptor.extend_from_slice(&plan.anchors[index].pose);
                writer.add_descriptor(
                    &label,
                    &descriptor,
                    &label_at(group[0].bind_at, &labels_at, &mut extra_labels),
                );
                anchor_labels.insert(index, format!("{label} + 8"));
            } else {
                writer.add_pose(&label, &plan.anchors[index].pose);
                anchor_labels.insert(index, label);
            }
        }

        let mode = if plan.rows.iter().any(|row| row.mode == 3) {
            3
        } else {
            1
        };
        let mut descriptors = Vec::new();
        for (row_index, row) in plan.rows.iter().enumerate() {
            let label = format!("{prefix}_descriptor_{row_index}");
            let anchor_index = plan.choice[row_index];
            let (descriptor, base) = if let Some(anchor_index) = anchor_index {
                let descriptor =
                    encode_pose(&row.pose, &plan.anchors[anchor_index].pose, points, mode);
                let mut bytes = Vec::with_capacity(8 + descriptor.len());
                bytes.extend_from_slice(&(points as u16).to_le_bytes());
                bytes.extend_from_slice(&mode.to_le_bytes());
                bytes.extend_from_slice(&0i32.to_le_bytes());
                bytes.extend_from_slice(&descriptor);
                let base = anchor_labels
                    .get(&anchor_index)
                    .cloned()
                    .or_else(|| {
                        plan.anchors[anchor_index]
                            .at
                            .map(|at| label_at(at, &labels_at, &mut extra_labels))
                    })
                    .ok_or("new pose anchor has no emitted label")?;
                (bytes, base)
            } else {
                let (start, end) = row.span;
                let descriptor = assembly
                    .data
                    .get(start..end)
                    .ok_or("pose descriptor exceeds assembled assets")?
                    .to_vec();
                let base_at = row.anchor.unwrap_or(group[0].bind_at);
                (descriptor, label_at(base_at, &labels_at, &mut extra_labels))
            };
            let mut padded = descriptor;
            padded.resize(align4(padded.len()), 0);
            writer.add_descriptor(&label, &padded, &base);
            descriptors.push(label.clone());
            cuts.push(row.span);
            replacements.insert(row.span.0, label);
        }

        let mut tables = HashMap::new();
        for (model_index, model) in group.iter().enumerate() {
            if tables.contains_key(&model.origin) {
                continue;
            }
            let label = format!("{prefix}_table_{model_index}");
            let names = model
                .keys
                .iter()
                .map(|key| descriptors[plan.row_of[&key.span]].clone())
                .collect::<Vec<_>>();
            writer.add_table(&label, &names, &model.name);
            tables.insert(model.origin, label.clone());
            patches.insert(model.origin + 12, format!("{label} - {}", model.name));
            cuts.push(model.table);
            replacements.insert(model.table.0, label);
        }
        for model in group {
            if let Some(label) = tables.get(&model.origin) {
                patches.insert(model.origin + 12, format!("{label} - {}", model.name));
            }
        }
        blobs.insert(format!("{prefix}.bin"), writer.data);
        pool_lines.push("    .section .rodata.assets, \"a\", %progbits".to_owned());
        pool_lines.extend(writer.lines);
    }

    cuts = merge_ranges(cuts);
    let is_removed = |at: usize| cuts.iter().any(|(low, high)| *low <= at && at < *high);
    let mut lines = Vec::new();
    for statement in &assembly.statements {
        let at = statement.address;
        match statement.kind {
            AssemblyStatementKind::Label if is_removed(at) => {
                if let Some(label) = statement.name.as_ref().and_then(|_| replacements.get(&at)) {
                    lines.push(format!(
                        "    .set {}, {label}",
                        statement.name.as_deref().unwrap()
                    ));
                }
            }
            AssemblyStatementKind::Align if is_removed(at) => {}
            AssemblyStatementKind::Word if is_removed(at) => {}
            AssemblyStatementKind::Word => {
                if let Some(label) = extra_labels.get(&at) {
                    lines.push(format!("{label}:"));
                }
                let expression = patches
                    .get(&at)
                    .map(String::as_str)
                    .or(statement.expression.as_deref())
                    .ok_or("word statement missing expression")?;
                lines.push(format!("    .word {expression}"));
            }
            AssemblyStatementKind::Incbin => {
                let end = at + statement.size;
                let mut edges = vec![at, end];
                for (low, high) in &cuts {
                    for edge in [*low, *high] {
                        if at < edge && edge < end {
                            edges.push(edge);
                        }
                    }
                }
                for patch in patches.keys() {
                    for edge in [*patch, *patch + 4] {
                        if at < edge && edge < end {
                            edges.push(edge);
                        }
                    }
                }
                edges.extend(
                    extra_labels
                        .keys()
                        .filter(|label| at < **label && **label < end)
                        .copied(),
                );
                edges.sort_unstable();
                edges.dedup();
                for pair in edges.windows(2) {
                    let first = pair[0];
                    let last = pair[1];
                    if is_removed(first) {
                        continue;
                    }
                    if let Some(label) = extra_labels.get(&first) {
                        lines.push(format!("{label}:"));
                    }
                    if let Some(expression) = patches.get(&first) {
                        lines.push(format!("    .word {expression}"));
                    } else if first < last {
                        let name = statement
                            .name
                            .as_deref()
                            .ok_or("incbin statement missing name")?;
                        let offset = statement.start + first - at;
                        lines.push(format!(
                            "    .incbin \"{name}\", {offset}, {}",
                            last - first
                        ));
                    }
                }
            }
            _ => lines.push(statement.text.clone()),
        }
    }
    lines.extend(pool_lines);
    *text = lines.join("\n") + "\n";
    Ok(())
}

fn read_models(layout: &AssemblyLayout, names: &[String]) -> Result<Vec<Model>, String> {
    let mut models = Vec::new();
    for name in names {
        let origin = *layout
            .symbols
            .get(name)
            .ok_or_else(|| format!("missing figure {name}"))?;
        let header = FigureHeader::parse(&layout.data[origin..origin + FIGURE_BYTES])?;
        let mut keys = Vec::with_capacity(key_count(&header));
        for key in 0..key_count(&header) {
            let table = origin + header.pointer(12) as usize;
            let descriptor = (u32_at(&layout.data, table + key * 4)? & !1) as usize;
            let at = origin + descriptor;
            let (pose, end) = decode_pose(&layout.data, at, header.point_count())?;
            let mode = u16_at(&layout.data, at + 2)?;
            let relative = i32::from_le_bytes(layout.data[at + 4..at + 8].try_into().unwrap());
            let anchor = if mode == 0 {
                None
            } else {
                Some(
                    (at as i64 + relative as i64)
                        .try_into()
                        .map_err(|_| "pose anchor out of bounds")?,
                )
            };
            keys.push(Key {
                pose,
                span: (at, end),
                mode,
                anchor,
            });
        }
        let table = origin + header.pointer(12) as usize;
        let bind_at = origin + header.pointer(8) as usize;
        let bind_size = header.point_count() * 6;
        let bind = layout.data[bind_at..bind_at + bind_size].to_vec();
        let clips_at = origin + header.pointer(32) as usize;
        let clips_size = header.clip_count() * 8;
        let clips = layout.data[clips_at..clips_at + clips_size].to_vec();
        models.push(Model {
            name: name.clone(),
            origin,
            header,
            keys,
            table: (table, table + key_count_from_len(&layout.data, origin)? * 4),
            bind,
            bind_at,
            clips,
        });
    }
    Ok(models)
}

fn key_count_from_len(data: &[u8], origin: usize) -> Result<usize, String> {
    Ok((u16_at(data, origin + 6)? & 0x7fff) as usize)
}

fn groups_sharing_data(models: &[Model]) -> Vec<Vec<&Model>> {
    let mut parent = (0..models.len()).collect::<Vec<_>>();
    fn root(parent: &mut [usize], mut index: usize) -> usize {
        while parent[index] != index {
            parent[index] = parent[parent[index]];
            index = parent[index];
        }
        index
    }
    let mut users: HashMap<(usize, usize), Vec<usize>> = HashMap::new();
    for (index, model) in models.iter().enumerate() {
        for key in &model.keys {
            users.entry(key.span).or_default().push(index);
        }
        users.entry(model.table).or_default().push(index);
    }
    for members in users.values() {
        for &index in members.iter().skip(1) {
            let first = root(&mut parent, members[0]);
            let next = root(&mut parent, index);
            parent[next] = first;
        }
    }
    let mut groups: HashMap<usize, Vec<&Model>> = HashMap::new();
    for (index, model) in models.iter().enumerate() {
        if !model.keys.is_empty() {
            let key = root(&mut parent, index);
            groups.entry(key).or_default().push(model);
        }
    }
    let mut groups = groups.into_values().collect::<Vec<_>>();
    groups.sort_by_key(|group| group[0].origin);
    groups
}

fn plan_group(group: &[&Model], assembly: &AssemblyLayout) -> Result<Option<Plan>, String> {
    let points = group[0].header.point_count();
    if points > MAX_POINTS
        || group.iter().any(|model| {
            u16_at(&assembly.data, model.origin + 6).is_ok_and(|count| count & 0x8000 == 0)
        })
    {
        return Ok(None);
    }
    let mut rows = Vec::<Key>::new();
    let mut row_of = HashMap::new();
    for model in group {
        for key in &model.keys {
            if !row_of.contains_key(&key.span) {
                row_of.insert(key.span, rows.len());
                rows.push(key.clone());
            }
        }
    }
    let mut tables = HashSet::new();
    let mut old_bytes = rows
        .iter()
        .map(|row| row.span.1 - row.span.0)
        .sum::<usize>();
    for model in group {
        if tables.insert(model.table) {
            old_bytes += model.table.1 - model.table.0;
        }
    }

    let mut seen = HashSet::new();
    let mut anchors = Vec::new();
    for model in group {
        if seen.insert(model.bind.clone()) {
            anchors.push(Anchor {
                pose: model.bind.clone(),
                at: Some(model.bind_at),
                charge: 0,
                kind: AnchorKind::Bind,
            });
        }
    }
    for row in &rows {
        if row.mode == 0 && seen.insert(row.pose.clone()) {
            anchors.push(Anchor {
                pose: row.pose.clone(),
                at: Some(row.span.0 + 8),
                charge: row.span.1 - row.span.0,
                kind: AnchorKind::RawDescriptor,
            });
        }
    }
    let free_count = anchors.len();

    let mut picks = HashSet::new();
    for model in group {
        for clip in model.clips.chunks_exact(8) {
            let first = u16::from_le_bytes([clip[0], clip[1]]) as usize;
            let count = u16::from_le_bytes([clip[2], clip[3]]) as usize;
            if count != 0 {
                for key in [first, first + count - 1, first + count / 2] {
                    picks.insert(row_of[&model.keys[key].span]);
                }
            }
        }
    }
    picks.extend(samples(rows.len(), MAX_SAMPLES));
    let mut picks = picks.into_iter().collect::<Vec<_>>();
    picks.sort_unstable();
    if picks.len() > MAX_SAMPLES {
        picks = samples(picks.len(), MAX_SAMPLES)
            .into_iter()
            .map(|index| picks[index])
            .collect();
    }
    let pose_bytes = align4(points * 6);
    for &index in &picks {
        if seen.insert(rows[index].pose.clone()) {
            anchors.push(Anchor {
                pose: rows[index].pose.clone(),
                at: None,
                charge: pose_bytes,
                kind: AnchorKind::New,
            });
        }
    }
    let costs = anchors
        .iter()
        .map(|anchor| {
            rows.iter()
                .map(|row| pose_cost(row, &anchor.pose, points, &assembly.data))
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();

    let mut best = rows
        .iter()
        .map(|row| row.span.1 - row.span.0)
        .collect::<Vec<_>>();
    let mut choice = vec![None; rows.len()];
    let take = |anchor: usize, best: &mut Vec<usize>, choice: &mut Vec<Option<usize>>| {
        for row in 0..rows.len() {
            let cost = costs[anchor][row] as usize;
            if cost < best[row] {
                best[row] = cost;
                choice[row] = Some(anchor);
            }
        }
    };
    for anchor in 0..free_count {
        take(anchor, &mut best, &mut choice);
    }
    let mut picked = HashSet::new();
    for _ in 0..MAX_NEW_ANCHORS {
        let mut best_candidate = None;
        let mut best_score = 0isize;
        for candidate in free_count..anchors.len() {
            if picked.contains(&candidate) {
                continue;
            }
            let saving = best
                .iter()
                .zip(&costs[candidate])
                .map(|(old, new)| old.saturating_sub(*new as usize))
                .sum::<usize>();
            let score = saving as isize - pose_bytes as isize;
            if score > best_score {
                best_score = score;
                best_candidate = Some(candidate);
            }
        }
        let Some(candidate) = best_candidate else {
            break;
        };
        picked.insert(candidate);
        take(candidate, &mut best, &mut choice);
    }
    let used = choice.iter().flatten().copied().collect::<HashSet<_>>();
    let charges = used
        .iter()
        .filter(|index| anchors[**index].kind != AnchorKind::New)
        .map(|index| anchors[*index].charge)
        .sum::<usize>()
        + picked.len() * pose_bytes;
    let total =
        best.iter().sum::<usize>() + charges + tables.iter().map(|(a, b)| b - a).sum::<usize>();
    if total >= old_bytes {
        return Ok(None);
    }
    let mut kept = picked;
    kept.extend(
        used.into_iter()
            .filter(|index| anchors[*index].kind == AnchorKind::RawDescriptor),
    );
    let mut kept = kept.into_iter().collect::<Vec<_>>();
    kept.sort_unstable();
    Ok(Some(Plan {
        rows,
        row_of,
        anchors,
        choice,
        kept,
    }))
}

fn samples(count: usize, limit: usize) -> Vec<usize> {
    if count == 0 {
        return Vec::new();
    }
    if count == 1 {
        return vec![0];
    }
    let taken = count.min(limit);
    (0..taken)
        .map(|index| index * (count - 1) / (taken - 1))
        .collect()
}

fn pose_cost(row: &Key, base: &[u8], points: usize, data: &[u8]) -> u32 {
    if row.mode != 1 && row.mode != 3 {
        return UNUSABLE;
    }
    let values = row
        .pose
        .chunks_exact(2)
        .map(|v| i16::from_le_bytes([v[0], v[1]]) as i32)
        .collect::<Vec<_>>();
    let bases = base
        .chunks_exact(2)
        .map(|v| i16::from_le_bytes([v[0], v[1]]) as i32)
        .collect::<Vec<_>>();
    let old_widths = block_widths(data, row, points);
    let mut bytes = 8usize;
    for first in (0..points).step_by(32) {
        let count = (points - first).min(32);
        let mut deltas = Vec::with_capacity(count * 3);
        let mut low = [i32::MAX; 3];
        let mut high = [i32::MIN; 3];
        for point in 0..count {
            for axis in 0..3 {
                let at = (first + point) * 3 + axis;
                let delta = (values[at] - bases[at]) as i16 as i32;
                deltas.push(delta);
                low[axis] = low[axis].min(delta);
                high[axis] = high[axis].max(delta);
            }
        }
        let step = if row.mode == 3 { 3 } else { 2 };
        let mut scale = step;
        let mut varies = false;
        for point in 0..count {
            for axis in 0..3 {
                let delta = deltas[point * 3 + axis];
                if ((delta - low[axis]) & ((1 << step) - 1)) != 0 {
                    scale = 0;
                }
                varies |= delta != low[axis];
            }
        }
        if !varies {
            scale = 0;
        }
        let packed = old_widths[first / 32];
        let mut new_widths = [0u32; 3];
        let mut old_axis_widths = [0u32; 3];
        let mut bits = 0usize;
        for axis in 0..3 {
            let mut range = ((high[axis] - low[axis]) as u32) >> scale;
            while range != 0 {
                new_widths[axis] += 1;
                range >>= 1;
            }
            old_axis_widths[axis] = (packed >> (5 * axis)) & 31;
            bits += new_widths[axis] as usize;
            if (old_axis_widths[axis] == 0 || old_axis_widths[axis] == 16)
                && new_widths[axis] != old_axis_widths[axis]
            {
                return UNUSABLE;
            }
        }
        let old_reads = count_refills(count, old_axis_widths);
        let new_reads = count_refills(count, new_widths);
        if (0..3).any(|axis| new_reads[axis] > old_reads[axis]) {
            return UNUSABLE;
        }
        bytes += 8 + (count * bits + 7) / 8;
    }
    align4(bytes) as u32
}

fn block_widths(data: &[u8], row: &Key, points: usize) -> Vec<u32> {
    let mut widths = Vec::new();
    let mut at = row.span.0 + 8;
    for first in (0..points).step_by(32) {
        let packed = u16_at(data, at + 6).unwrap_or(0) & 0x7fff;
        widths.push(packed as u32);
        let size = (0..3)
            .map(|axis| ((packed >> (5 * axis)) & 31) as usize)
            .sum::<usize>();
        at += 8 + (((points - first).min(32) * size + 7) / 8);
    }
    widths
}

fn count_refills(points: usize, widths: [u32; 3]) -> [u32; 3] {
    let mut available = 0u32;
    let mut reads = [0; 3];
    for _ in 0..points {
        for axis in 0..3 {
            if available < widths[axis] {
                available += 32 - widths[axis];
                reads[axis] += 1;
            } else {
                available -= widths[axis];
            }
        }
    }
    reads
}

fn encode_pose(pose: &[u8], base: &[u8], points: usize, mode: u16) -> Vec<u8> {
    let values = pose
        .chunks_exact(2)
        .map(|v| i16::from_le_bytes([v[0], v[1]]) as i32)
        .collect::<Vec<_>>();
    let bases = base
        .chunks_exact(2)
        .map(|v| i16::from_le_bytes([v[0], v[1]]) as i32)
        .collect::<Vec<_>>();
    let mut output = Vec::new();
    for first in (0..points).step_by(32) {
        let count = (points - first).min(32);
        let mut deltas = Vec::with_capacity(count * 3);
        let mut low = [i32::MAX; 3];
        let mut high = [i32::MIN; 3];
        for point in 0..count {
            for axis in 0..3 {
                let at = (first + point) * 3 + axis;
                let delta = (values[at] - bases[at]) as i16 as i32;
                deltas.push(delta);
                low[axis] = low[axis].min(delta);
                high[axis] = high[axis].max(delta);
            }
        }
        let step = if mode == 3 { 3 } else { 2 };
        let mut scale = step;
        let mut varies = false;
        for point in 0..count {
            for axis in 0..3 {
                let delta = deltas[point * 3 + axis];
                if ((delta - low[axis]) & ((1 << step) - 1)) != 0 {
                    scale = 0;
                }
                varies |= high[axis] > low[axis];
            }
        }
        if !varies {
            scale = 0;
        }
        let widths = (0..3)
            .map(|axis| {
                let range = ((high[axis] - low[axis]) as u32) >> scale;
                if range == 0 {
                    0
                } else {
                    32 - range.leading_zeros()
                }
            })
            .collect::<Vec<_>>();
        let mut flags = widths[0] | (widths[1] << 5) | (widths[2] << 10);
        if scale != 0 {
            flags |= 0x8000;
        }
        for value in low {
            output.extend_from_slice(&(value as i16).to_le_bytes());
        }
        output.extend_from_slice(&(flags as u16).to_le_bytes());
        let mut bit_position = 0usize;
        let mut bits = Vec::new();
        for delta in deltas.chunks_exact(3) {
            for axis in 0..3 {
                let value = ((delta[axis] - low[axis]) >> scale) as u32;
                for bit in 0..widths[axis] as usize {
                    let position = bit_position + bit;
                    if bits.len() <= position / 8 {
                        bits.push(0);
                    }
                    bits[position / 8] |= (((value >> bit) & 1) as u8) << (position % 8);
                }
                bit_position += widths[axis] as usize;
            }
        }
        output.extend_from_slice(&bits);
    }
    output
}

fn merge_ranges(mut ranges: Vec<(usize, usize)>) -> Vec<(usize, usize)> {
    ranges.sort_unstable();
    let mut merged: Vec<(usize, usize)> = Vec::new();
    for (low, high) in ranges {
        if let Some(last) = merged.last_mut().filter(|last| low <= last.1) {
            last.1 = last.1.max(high);
        } else {
            merged.push((low, high));
        }
    }
    merged
}

fn label_at(
    at: usize,
    labels_at: &HashMap<usize, Vec<String>>,
    extra: &mut HashMap<usize, String>,
) -> String {
    labels_at
        .get(&at)
        .and_then(|labels| labels.first())
        .cloned()
        .unwrap_or_else(|| {
            extra
                .entry(at)
                .or_insert_with(|| format!("pose_anchor_retained_{at}"))
                .clone()
        })
}

struct PoolWriter {
    prefix: String,
    data: Vec<u8>,
    lines: Vec<String>,
}

impl PoolWriter {
    fn new(prefix: &str) -> Self {
        Self {
            prefix: prefix.into(),
            data: Vec::new(),
            lines: Vec::new(),
        }
    }
    fn start(&mut self, label: &str, data: &[u8]) -> usize {
        let at = align4(self.data.len());
        self.data.resize(at, 0);
        self.data.extend_from_slice(data);
        self.lines.push("    .balign 4".into());
        self.lines.push(format!("{label}:"));
        at
    }
    fn add_pose(&mut self, label: &str, pose: &[u8]) {
        let at = self.start(label, pose);
        self.lines.push(format!(
            "    .incbin \"{}.bin\", {at}, {}",
            self.prefix,
            pose.len()
        ));
    }
    fn add_descriptor(&mut self, label: &str, descriptor: &[u8], base: &str) {
        let at = self.start(label, descriptor);
        self.lines
            .push(format!("    .incbin \"{}.bin\", {at}, 4", self.prefix));
        self.lines.push(format!("    .word {base} - {label}"));
        self.lines.push(format!(
            "    .incbin \"{}.bin\", {}, {}",
            self.prefix,
            at + 8,
            descriptor.len().saturating_sub(8)
        ));
    }
    fn add_table(&mut self, label: &str, descriptors: &[String], figure: &str) {
        let zero = vec![0; descriptors.len() * 4];
        let at = self.start(label, &zero);
        for (index, descriptor) in descriptors.iter().enumerate() {
            let _ = at + index * 4;
            self.lines
                .push(format!("    .word {descriptor} - {figure} + 1"));
        }
    }
}

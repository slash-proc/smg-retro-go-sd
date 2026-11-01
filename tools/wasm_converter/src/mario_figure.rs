//! Recipe-driven static Mario figure geometry.

use crate::{
    asset_formats, asset_packs,
    j3d_animation::{self, Bck, Correction},
    j3d_model::{self, J3dModel},
};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

const PACK: &str = "mario_figure/mario_figure.pak";

#[derive(Debug)]
pub enum FigureError {
    MissingRecipe,
    InvalidRecipe,
    InvalidArchive,
    InvalidModel,
    InvalidResidual,
    MissingAnchor,
    InvalidAnimation,
}

type VSet = (Vec<i64>, Vec<bool>);

fn integer(value: &Value) -> Result<i64, FigureError> {
    value.as_i64().ok_or(FigureError::InvalidRecipe)
}

fn string(value: &Value) -> Result<&str, FigureError> {
    value.as_str().ok_or(FigureError::InvalidRecipe)
}

fn array(value: &Value) -> Result<&[Value], FigureError> {
    value
        .as_array()
        .map(Vec::as_slice)
        .ok_or(FigureError::InvalidRecipe)
}

fn round_int(value: f64, scale: f64) -> i64 {
    (value * scale + 0.5).floor() as i64
}

fn uvarint(data: &[u8], cursor: &mut usize) -> Result<u64, FigureError> {
    let mut value = 0u64;
    let mut shift = 0;
    loop {
        let byte = *data.get(*cursor).ok_or(FigureError::InvalidResidual)?;
        *cursor += 1;
        if shift >= 64 || (shift == 63 && byte & 0x7e != 0) {
            return Err(FigureError::InvalidResidual);
        }
        value |= ((byte & 0x7f) as u64) << shift;
        if byte < 0x80 {
            return Ok(value);
        }
        shift += 7;
    }
}

fn signed_varint(data: &[u8], cursor: &mut usize) -> Result<i64, FigureError> {
    let value = uvarint(data, cursor)?;
    Ok(((value >> 1) as i64) ^ (-((value & 1) as i64)))
}

fn residual_apply(
    stream: &str,
    mut values: Vec<i64>,
    negative: &[bool],
    pack: &[u8],
    entries: &[(String, usize, usize)],
) -> Result<VSet, FigureError> {
    let res_name = format!("res/{stream}");
    if let Some((_, offset, len)) = entries.iter().find(|(name, _, _)| name == &res_name) {
        let res = pack
            .get(
                *offset
                    ..offset
                        .checked_add(*len)
                        .ok_or(FigureError::InvalidResidual)?,
            )
            .ok_or(FigureError::InvalidResidual)?;
        let mut cursor = 0;
        for value in &mut values {
            *value += signed_varint(res, &mut cursor)?;
        }
    }
    let neg_name = format!("neg/{stream}");
    let mut flips = BTreeSet::new();
    if let Some((_, offset, len)) = entries.iter().find(|(name, _, _)| name == &neg_name) {
        let neg = pack
            .get(
                *offset
                    ..offset
                        .checked_add(*len)
                        .ok_or(FigureError::InvalidResidual)?,
            )
            .ok_or(FigureError::InvalidResidual)?;
        let mut cursor = 0;
        let mut pos = 0i64;
        while cursor < neg.len() {
            pos += signed_varint(neg, &mut cursor)?;
            flips.insert(pos);
        }
    }
    let negative = values
        .iter()
        .enumerate()
        .map(|(i, value)| *value == 0 && (negative[i] != flips.contains(&(i as i64))))
        .collect();
    Ok((values, negative))
}

fn anchors(model: &J3dModel, meta: &Value) -> Result<BTreeMap<String, VSet>, FigureError> {
    let groups = meta
        .get("anchors")
        .and_then(Value::as_object)
        .ok_or(FigureError::InvalidRecipe)?;
    let mut result = BTreeMap::new();
    for (name, points) in groups {
        let mut values = Vec::new();
        let mut negatives = Vec::new();
        for pair in array(points)? {
            let pair = array(pair)?;
            if pair.len() != 2 {
                return Err(FigureError::InvalidRecipe);
            }
            let point = model
                .anchor_point(integer(&pair[0])? as usize, integer(&pair[1])? as usize)
                .ok_or(FigureError::MissingAnchor)?;
            for value in point {
                values.push(round_int(value, 400.0));
                negatives.push(value < 0.0);
            }
        }
        result.insert(name.clone(), (values, negatives));
    }
    Ok(result)
}

fn uv_values(model: &J3dModel, meta: &Value) -> Result<VSet, FigureError> {
    let mut values = Vec::new();
    let mut negatives = Vec::new();
    for pair in array(meta.get("uv_anchors").ok_or(FigureError::InvalidRecipe)?)? {
        let pair = array(pair)?;
        if pair.len() != 2 {
            return Err(FigureError::InvalidRecipe);
        }
        let uv = *model
            .uvs
            .get(integer(&pair[0])? as usize)
            .ok_or(FigureError::MissingAnchor)?;
        let v = if integer(&pair[1])? != 0 {
            1.0 - uv[1]
        } else {
            uv[1]
        };
        for value in [uv[0], v] {
            values.push(round_int(value, 1e6));
            negatives.push(value < 0.0);
        }
    }
    Ok((values, negatives))
}

fn formatted(value: i64, digits: usize, negative_zero: bool) -> String {
    let mut body = value.unsigned_abs().to_string();
    if body.len() <= digits {
        body.insert_str(0, &"0".repeat(digits + 1 - body.len()));
    }
    let dot = body.len() - digits;
    body.insert(dot, '.');
    if value < 0 || negative_zero {
        format!("-{body}")
    } else {
        body
    }
}

fn append_formatted(out: &mut Vec<u8>, value: i64, digits: usize, negative_zero: bool) {
    let magnitude = value.unsigned_abs();
    let scale = 10u64.pow(digits as u32);
    let whole = magnitude / scale;
    let fraction = magnitude % scale;
    if value < 0 || negative_zero {
        out.push(b'-');
    }
    let mut whole_digits = [0u8; 20];
    let mut cursor = whole_digits.len();
    let mut remaining = whole;
    loop {
        cursor -= 1;
        whole_digits[cursor] = b'0' + (remaining % 10) as u8;
        remaining /= 10;
        if remaining == 0 {
            break;
        }
    }
    out.extend_from_slice(&whole_digits[cursor..]);
    out.push(b'.');
    let mut divisor = scale / 10;
    for _ in 0..digits {
        out.push(b'0' + ((fraction / divisor) % 10) as u8);
        divisor /= 10;
    }
}

fn formatted_row(
    values: &[i64],
    negatives: &[bool],
    start: usize,
    digits: usize,
    multiplier: i64,
) -> Result<String, FigureError> {
    let mut parts = Vec::with_capacity(3);
    for k in 0..3 {
        let value = *values.get(start + k).ok_or(FigureError::InvalidRecipe)?;
        let negative = *negatives.get(start + k).ok_or(FigureError::InvalidRecipe)?;
        parts.push(formatted(
            value
                .checked_mul(multiplier)
                .ok_or(FigureError::InvalidRecipe)?,
            digits,
            negative,
        ));
    }
    Ok(parts.join(" "))
}

fn render_static(
    meta: &Value,
    vsets: &BTreeMap<String, VSet>,
    uv: &[i64],
) -> Result<Vec<(String, Vec<u8>)>, FigureError> {
    let mut files = BTreeMap::<String, Vec<u8>>::new();
    let objs = meta
        .get("objs")
        .and_then(Value::as_object)
        .ok_or(FigureError::InvalidRecipe)?;
    for (path, segments) in objs {
        let mut lines = Vec::new();
        for segment in array(segments)? {
            let segment = array(segment)?;
            if segment.len() != 2 && segment.len() != 3 {
                return Err(FigureError::InvalidRecipe);
            }
            let tag = string(&segment[0])?;
            match tag {
                "v" => {
                    let digits =
                        integer(segment.get(2).ok_or(FigureError::InvalidRecipe)?)? as usize;
                    let mut values = Vec::new();
                    let mut negatives = Vec::new();
                    for name in array(&segment[1])? {
                        let (v, n) = vsets.get(string(name)?).ok_or(FigureError::InvalidRecipe)?;
                        values.extend_from_slice(v);
                        negatives.extend_from_slice(n);
                    }
                    let multiplier =
                        10i64
                            .checked_pow(
                                digits.checked_sub(5).ok_or(FigureError::InvalidRecipe)? as u32
                            )
                            .ok_or(FigureError::InvalidRecipe)?;
                    for start in (0..values.len()).step_by(3) {
                        lines.push(format!(
                            "v {}",
                            formatted_row(&values, &negatives, start, digits, multiplier)?
                        ));
                    }
                }
                "vt" => {
                    for index in array(&segment[1])? {
                        let index = integer(index)? as usize;
                        lines.push(format!(
                            "vt {} {}",
                            formatted(
                                *uv.get(index * 2).ok_or(FigureError::InvalidRecipe)?,
                                6,
                                false
                            ),
                            formatted(
                                *uv.get(index * 2 + 1).ok_or(FigureError::InvalidRecipe)?,
                                6,
                                false
                            )
                        ));
                    }
                }
                "f" => {
                    let indices = array(&segment[1])?;
                    if indices.len() % 6 != 0 {
                        return Err(FigureError::InvalidRecipe);
                    }
                    for triangle in indices.chunks_exact(6) {
                        lines.push(format!(
                            "f {}/{} {}/{} {}/{}",
                            integer(&triangle[0])?,
                            integer(&triangle[1])?,
                            integer(&triangle[2])?,
                            integer(&triangle[3])?,
                            integer(&triangle[4])?,
                            integer(&triangle[5])?
                        ));
                    }
                }
                _ => lines.push(format!("{} {}", tag, string(&segment[1])?)),
            }
        }
        lines.push(String::new());
        files.insert(
            format!("assets/figures/{path}"),
            lines.join("\n").into_bytes(),
        );
    }
    let mtls = meta
        .get("mtls")
        .and_then(Value::as_object)
        .ok_or(FigureError::InvalidRecipe)?;
    for (path, text) in mtls {
        files.insert(
            format!("assets/figures/{path}"),
            string(text)?.as_bytes().to_vec(),
        );
    }
    let copies = meta
        .get("copies")
        .and_then(Value::as_object)
        .ok_or(FigureError::InvalidRecipe)?;
    for (path, source) in copies {
        let bytes = files
            .get(&format!("assets/figures/{}", string(source)?))
            .ok_or(FigureError::InvalidRecipe)?
            .clone();
        files.insert(format!("assets/figures/{path}"), bytes);
    }
    Ok(files.into_iter().collect())
}

/// Build the static OBJ/MTL and UV-anchor files from the Mario figure recipe.
pub fn generate_static(mario_archive: &[u8]) -> Result<Vec<(String, Vec<u8>)>, FigureError> {
    let pack = asset_packs::EMBEDDED_PACKS
        .iter()
        .find(|p| p.name == PACK)
        .ok_or(FigureError::MissingRecipe)?;
    let meta_bytes = asset_packs::get(PACK, "meta.json").ok_or(FigureError::MissingRecipe)?;
    let meta: Value = serde_json::from_slice(meta_bytes).map_err(|_| FigureError::InvalidRecipe)?;
    let recipe_entries = pack
        .entries
        .iter()
        .map(|(name, offset, size)| (name.to_string(), *offset, *size))
        .collect::<Vec<_>>();
    let bdl = asset_formats::rarc_find(mario_archive, "mario.bdl")
        .map_err(|_| FigureError::InvalidArchive)?;
    let model = J3dModel::parse(&bdl).map_err(|_| FigureError::InvalidModel)?;
    let vsets = anchors(&model, &meta)?;
    // Apply the recipe's per-stream integer corrections after geometric prediction.
    let mut corrected = BTreeMap::new();
    for (name, (values, negatives)) in vsets {
        corrected.insert(
            name.clone(),
            residual_apply(
                &format!("vset:{name}"),
                values,
                &negatives,
                pack.data,
                &recipe_entries,
            )?,
        );
    }
    let (uv, uv_negatives) = uv_values(&model, &meta)?;
    let (uv, _) = residual_apply("uv", uv, &uv_negatives, pack.data, &recipe_entries)?;
    render_static(&meta, &corrected, &uv)
}

const OBJ_SCALE: f64 = 0.004;
type Matrix = [f64; 12];

#[derive(Clone)]
struct SkinGroup {
    bones: Vec<Vec<usize>>,
    weights: Vec<Vec<f64>>,
    rest: Vec<[f64; 3]>,
}

#[derive(Clone)]
struct FigureClip {
    name: String,
    frame_count: usize,
    group: String,
    animation: String,
    start: i64,
    step: i64,
    wing: Option<(String, Option<f64>)>,
}

struct FigureSources<'a> {
    mario: J3dModel,
    wing: J3dModel,
    mario_anime: &'a [u8],
    wing_archive: &'a [u8],
    corrections: BTreeMap<usize, Correction>,
    mario_bcks: BTreeMap<String, Bck>,
    wing_bcks: BTreeMap<String, Bck>,
    bvas: BTreeMap<String, Vec<Vec<u8>>>,
    mario_poses: BTreeMap<(String, i64), Vec<Matrix>>,
    wing_poses: BTreeMap<(String, u64), Vec<Matrix>>,
}

impl<'a> FigureSources<'a> {
    fn new(
        mario_archive: &[u8],
        mario_anime: &'a [u8],
        wing_archive: &'a [u8],
        meta: &Value,
    ) -> Result<Self, FigureError> {
        let mario_data = asset_formats::rarc_find(mario_archive, "mario.bdl")
            .map_err(|_| FigureError::InvalidArchive)?;
        let wing_data = asset_formats::rarc_find(wing_archive, "beemariowing.bdl")
            .map_err(|_| FigureError::InvalidArchive)?;
        let mario = J3dModel::parse(&mario_data).map_err(|_| FigureError::InvalidModel)?;
        let wing = J3dModel::parse(&wing_data).map_err(|_| FigureError::InvalidModel)?;
        let mut corrections = BTreeMap::new();
        for item in array(meta.get("corrections").ok_or(FigureError::InvalidRecipe)?)? {
            let fields = array(item)?;
            if fields.len() != 7 {
                return Err(FigureError::InvalidRecipe);
            }
            let joint = integer(&fields[0])? as usize;
            let rotation = [
                integer(&fields[1])? as f64 / 1e10,
                integer(&fields[2])? as f64 / 1e10,
                integer(&fields[3])? as f64 / 1e10,
            ];
            let translation = [
                integer(&fields[4])? as f64 / 1e7,
                integer(&fields[5])? as f64 / 1e7,
                integer(&fields[6])? as f64 / 1e7,
            ];
            let matrix = j3d_animation::rotation_vector_matrix(rotation, translation);
            corrections.insert(
                joint,
                j3d_animation::correction(&mario, joint, matrix)
                    .ok_or(FigureError::InvalidRecipe)?,
            );
        }
        Ok(Self {
            mario,
            wing,
            mario_anime,
            wing_archive,
            corrections,
            mario_bcks: BTreeMap::new(),
            wing_bcks: BTreeMap::new(),
            bvas: BTreeMap::new(),
            mario_poses: BTreeMap::new(),
            wing_poses: BTreeMap::new(),
        })
    }

    fn joint(&self, name: &str) -> Result<usize, FigureError> {
        self.mario
            .names
            .iter()
            .position(|candidate| candidate == name)
            .ok_or(FigureError::InvalidModel)
    }

    fn load_mario_bck(&mut self, name: &str) -> Result<(), FigureError> {
        if !self.mario_bcks.contains_key(name) {
            let data = asset_formats::rarc_find(self.mario_anime, &format!("{name}.bck"))
                .map_err(|_| FigureError::InvalidArchive)?;
            self.mario_bcks.insert(
                name.to_owned(),
                Bck::parse(&data).map_err(|_| FigureError::InvalidAnimation)?,
            );
        }
        Ok(())
    }

    fn load_wing_bck(&mut self, name: &str) -> Result<(), FigureError> {
        if !self.wing_bcks.contains_key(name) {
            let data = asset_formats::rarc_find(self.wing_archive, &format!("{name}.bck"))
                .map_err(|_| FigureError::InvalidArchive)?;
            self.wing_bcks.insert(
                name.to_owned(),
                Bck::parse(&data).map_err(|_| FigureError::InvalidAnimation)?,
            );
        }
        Ok(())
    }

    fn load_bva(&mut self, name: &str) -> Result<(), FigureError> {
        if !self.bvas.contains_key(name) {
            let data = asset_formats::rarc_find(self.wing_archive, &format!("{name}.bva"))
                .map_err(|_| FigureError::InvalidArchive)?;
            self.bvas.insert(
                name.to_owned(),
                j3d_animation::bva_visibility(&data).map_err(|_| FigureError::InvalidAnimation)?,
            );
        }
        Ok(())
    }

    fn mario_pose(
        &mut self,
        animation: Option<&str>,
        frame: i64,
    ) -> Result<Vec<Matrix>, FigureError> {
        let key = (animation.unwrap_or("").to_owned(), frame);
        if let Some(pose) = self.mario_poses.get(&key) {
            return Ok(pose.clone());
        }
        let raw = if let Some(name) = animation {
            self.load_mario_bck(name)?;
            self.mario_bcks
                .get(name)
                .unwrap()
                .raw(&self.mario, frame as f64)
        } else {
            self.mario
                .joints
                .iter()
                .map(|joint| j3d_animation::RawJoint {
                    scale: joint.scale,
                    rotation: joint.rotation,
                    translation: joint.translation,
                })
                .collect()
        };
        let local = j3d_animation::pose(&self.mario, &raw);
        let pose = j3d_animation::deform(&self.mario, &local, &self.corrections);
        self.mario_poses.insert(key.clone(), pose.clone());
        Ok(pose)
    }

    fn wing_pose(&mut self, animation: &str, frame: f64) -> Result<Vec<Matrix>, FigureError> {
        let key = (animation.to_owned(), frame.to_bits());
        if let Some(pose) = self.wing_poses.get(&key) {
            return Ok(pose.clone());
        }
        self.load_wing_bck(animation)?;
        let raw = self
            .wing_bcks
            .get(animation)
            .unwrap()
            .raw(&self.wing, frame);
        let local = j3d_animation::pose(&self.wing, &raw);
        let pose = j3d_animation::deform(&self.wing, &local, &BTreeMap::new());
        self.wing_poses.insert(key, pose.clone());
        Ok(pose)
    }

    fn wing_frame(
        &mut self,
        animation: &str,
        frame: i64,
        start: i64,
        fixed: Option<f64>,
    ) -> Result<(f64, Vec<Matrix>), FigureError> {
        self.load_wing_bck(animation)?;
        let bck = self.wing_bcks.get(animation).unwrap();
        let frame = if let Some(fixed) = fixed {
            fixed
        } else {
            let mut frame = (frame - start) as f64;
            if bck.loop_mode == 2 && bck.frame_count != 0 {
                frame = frame.rem_euclid(bck.frame_count as f64);
            }
            frame
        };
        let sample = frame.min(bck.frame_count as f64);
        Ok((frame, self.wing_pose(animation, sample)?))
    }

    fn wing_visibility(
        &mut self,
        animation: &str,
        shape: usize,
        frame: f64,
    ) -> Result<bool, FigureError> {
        self.load_bva(animation)?;
        let values = self
            .bvas
            .get(animation)
            .unwrap()
            .get(shape)
            .ok_or(FigureError::InvalidRecipe)?;
        let index = (frame as i64)
            .max(0)
            .min(values.len().saturating_sub(1) as i64) as usize;
        Ok(values.get(index).copied().unwrap_or(0) != 0)
    }
}

fn build_groups(
    meta: &Value,
    vsets: &BTreeMap<String, VSet>,
) -> Result<BTreeMap<String, SkinGroup>, FigureError> {
    let groups = meta
        .get("groups")
        .and_then(Value::as_object)
        .ok_or(FigureError::InvalidRecipe)?;
    let mario = &vsets.get("mario").ok_or(FigureError::InvalidRecipe)?.0;
    let mut result = BTreeMap::new();
    for (name, entries) in groups {
        let mut group = SkinGroup {
            bones: Vec::new(),
            weights: Vec::new(),
            rest: Vec::new(),
        };
        for (vertex, entry) in array(entries)?.iter().enumerate() {
            let entry = array(entry)?;
            if entry.len() != 3 {
                return Err(FigureError::InvalidRecipe);
            }
            let bones = array(&entry[0])?
                .iter()
                .map(|bone| integer(bone).map(|x| x as usize))
                .collect::<Result<Vec<_>, _>>()?;
            let weights_q = array(&entry[1])?
                .iter()
                .map(integer)
                .collect::<Result<Vec<_>, _>>()?;
            let pd = array(&entry[2])?
                .iter()
                .map(integer)
                .collect::<Result<Vec<_>, _>>()?;
            if pd.len() != 3 {
                return Err(FigureError::InvalidRecipe);
            }
            let sum: i64 = weights_q.iter().sum();
            let mut weights = weights_q
                .iter()
                .map(|&weight| weight as f64 / 1e8)
                .collect::<Vec<_>>();
            weights.push((100_000_000 - sum) as f64 / 1e8);
            let start = vertex.checked_mul(3).ok_or(FigureError::InvalidRecipe)?;
            let base = mario
                .get(start..start + 3)
                .ok_or(FigureError::InvalidRecipe)?;
            let rest = std::array::from_fn(|axis| (base[axis] * 1000 + pd[axis]) as f64 / 1e8);
            group.bones.push(bones);
            group.weights.push(weights);
            group.rest.push(rest);
        }
        result.insert(name.clone(), group);
    }
    Ok(result)
}

fn parse_clips(meta: &Value, figure: &str) -> Result<Vec<FigureClip>, FigureError> {
    let entries = array(
        meta.get("figures")
            .and_then(|f| f.get(figure))
            .ok_or(FigureError::InvalidRecipe)?,
    )?;
    let mut clips = Vec::with_capacity(entries.len());
    for entry in entries {
        let entry = array(entry)?;
        if entry.len() < 6 {
            return Err(FigureError::InvalidRecipe);
        }
        let wing = if entry.len() > 6 {
            let spec = array(&entry[6])?;
            if spec.len() != 2 {
                return Err(FigureError::InvalidRecipe);
            }
            Some((
                string(&spec[0])?.to_owned(),
                if spec[1].is_null() {
                    None
                } else {
                    Some(
                        spec[1]
                            .as_f64()
                            .filter(|value| value.is_finite())
                            .ok_or(FigureError::InvalidRecipe)?,
                    )
                },
            ))
        } else {
            None
        };
        clips.push(FigureClip {
            name: string(&entry[0])?.to_owned(),
            frame_count: integer(&entry[1])? as usize,
            group: integer(&entry[2])?.to_string(),
            animation: string(&entry[3])?.to_owned(),
            start: integer(&entry[4])?,
            step: integer(&entry[5])?,
            wing,
        });
    }
    Ok(clips)
}

fn rest_point(base: &[i64], pd: &[Value]) -> Result<[f64; 3], FigureError> {
    if base.len() != 3 || pd.len() != 3 {
        return Err(FigureError::InvalidRecipe);
    }
    Ok([
        (base[0] * 1000 + integer(&pd[0])?) as f64 / 1e8,
        (base[1] * 1000 + integer(&pd[1])?) as f64 / 1e8,
        (base[2] * 1000 + integer(&pd[2])?) as f64 / 1e8,
    ])
}

fn mat_apply(m: &Matrix, p: [f64; 3]) -> [f64; 3] {
    [
        m[0] * p[0] + m[1] * p[1] + m[2] * p[2] + m[3],
        m[4] * p[0] + m[5] * p[1] + m[6] * p[2] + m[7],
        m[8] * p[0] + m[9] * p[1] + m[10] * p[2] + m[11],
    ]
}

fn quaternion_of(m: &Matrix) -> [f64; 4] {
    let mut c = [[0.0; 3]; 3];
    for k in 0..3 {
        let x = m[k];
        let y = m[4 + k];
        let z = m[8 + k];
        let len = (x * x + y * y + z * z).sqrt();
        c[k] = if len > 0.0 {
            [x / len, y / len, z / len]
        } else {
            [0.0; 3]
        };
    }
    let det = c[0][0] * (c[1][1] * c[2][2] - c[1][2] * c[2][1])
        - c[1][0] * (c[0][1] * c[2][2] - c[0][2] * c[2][1])
        + c[2][0] * (c[0][1] * c[1][2] - c[0][2] * c[1][1]);
    if det < 0.0 {
        for column in &mut c {
            for value in column {
                *value = -*value;
            }
        }
    }
    let (mut q0, mut q1, mut q2, mut q3);
    if c[2][2] < 0.0 {
        if c[0][0] > c[1][1] {
            let mut s = 2.0 * (1.0 + c[0][0] - c[1][1] - c[2][2]).sqrt();
            if c[1][2] < c[2][1] {
                s = -s;
            }
            q1 = 0.25 * s;
            s = 1.0 / s;
            q0 = (c[1][2] - c[2][1]) * s;
            q2 = (c[0][1] + c[1][0]) * s;
            q3 = (c[2][0] + c[0][2]) * s;
        } else {
            let mut s = 2.0 * (1.0 - c[0][0] + c[1][1] - c[2][2]).sqrt();
            if c[2][0] < c[0][2] {
                s = -s;
            }
            q2 = 0.25 * s;
            s = 1.0 / s;
            q0 = (c[2][0] - c[0][2]) * s;
            q1 = (c[0][1] + c[1][0]) * s;
            q3 = (c[1][2] + c[2][1]) * s;
        }
    } else if c[0][0] < -c[1][1] {
        let mut s = 2.0 * (1.0 - c[0][0] - c[1][1] + c[2][2]).sqrt();
        if c[0][1] < c[1][0] {
            s = -s;
        }
        q3 = 0.25 * s;
        s = 1.0 / s;
        q0 = (c[0][1] - c[1][0]) * s;
        q1 = (c[2][0] + c[0][2]) * s;
        q2 = (c[1][2] + c[2][1]) * s;
    } else {
        let mut s = 2.0 * (1.0 + c[0][0] + c[1][1] + c[2][2]).sqrt();
        q0 = 0.25 * s;
        s = 1.0 / s;
        q1 = (c[1][2] - c[2][1]) * s;
        q2 = (c[2][0] - c[0][2]) * s;
        q3 = (c[0][1] - c[1][0]) * s;
    }
    let len2 = q0 * q0 + q1 * q1 + q2 * q2 + q3 * q3;
    if (len2 - 1.0).abs() >= 0.0006 {
        let len = len2.sqrt();
        q0 /= len;
        q1 /= len;
        q2 /= len;
        q3 /= len;
    }
    [q1, q2, q3, q0]
}

fn rigid_of(m: &Matrix) -> Matrix {
    let q = quaternion_of(m);
    let (x, y, z, w) = (q[0], q[1], q[2], q[3]);
    [
        1.0 - 2.0 * (y * y + z * z),
        2.0 * (x * y - z * w),
        2.0 * (x * z + y * w),
        m[3],
        2.0 * (x * y + z * w),
        1.0 - 2.0 * (x * x + z * z),
        2.0 * (y * z - x * w),
        m[7],
        2.0 * (x * z - y * w),
        2.0 * (y * z + x * w),
        1.0 - 2.0 * (x * x + y * y),
        m[11],
    ]
}

fn skin(
    pose: &[Matrix],
    bones: &[usize],
    weights: &[f64],
    point: [f64; 3],
) -> Result<[f64; 3], FigureError> {
    let mut out = [0.0; 3];
    for (&bone, &weight) in bones.iter().zip(weights) {
        let m = pose.get(bone).ok_or(FigureError::InvalidRecipe)?;
        out[0] += weight * (m[0] * point[0] + m[1] * point[1] + m[2] * point[2] + m[3] * OBJ_SCALE);
        out[1] += weight * (m[4] * point[0] + m[5] * point[1] + m[6] * point[2] + m[7] * OBJ_SCALE);
        out[2] +=
            weight * (m[8] * point[0] + m[9] * point[1] + m[10] * point[2] + m[11] * OBJ_SCALE);
    }
    Ok(out)
}

fn part_rest(
    sources: &mut FigureSources<'_>,
    part: &[Value],
    point: [f64; 3],
) -> Result<[f64; 3], FigureError> {
    let kind = integer(part.first().ok_or(FigureError::InvalidRecipe)?)?;
    let joint = integer(part.get(1).ok_or(FigureError::InvalidRecipe)?)? as usize;
    let mario_pose = sources.mario_pose(None, 0)?;
    let matrix = if kind == 0 {
        mario_pose[sources.joint("Head")?]
    } else {
        let spine = sources.joint("Spine2")?;
        let body = j3d_model::mmul(&mario_pose[spine], &sources.mario.bind[spine]);
        let body = rigid_of(&body);
        let wing = sources.wing_pose("wait", 0.0)?;
        j3d_model::mmul(&body, wing.get(joint).ok_or(FigureError::InvalidRecipe)?)
    };
    let inverse = j3d_model::minv(&matrix);
    let scaled = point.map(|v| v / OBJ_SCALE);
    Ok(mat_apply(&inverse, scaled))
}

fn part_matrix(
    sources: &mut FigureSources<'_>,
    part: &[Value],
    pose: &[Matrix],
    wing: Option<(&str, f64, &[Matrix])>,
    bee_body: Option<&Matrix>,
) -> Result<(Matrix, bool), FigureError> {
    let kind = integer(part.first().ok_or(FigureError::InvalidRecipe)?)?;
    let joint = integer(part.get(1).ok_or(FigureError::InvalidRecipe)?)? as usize;
    if kind == 0 {
        return Ok((
            *pose
                .get(sources.joint("Head")?)
                .ok_or(FigureError::InvalidRecipe)?,
            true,
        ));
    }
    let shape = integer(part.get(2).ok_or(FigureError::InvalidRecipe)?)? as usize;
    let (animation, frame, wing_pose) = wing.ok_or(FigureError::InvalidRecipe)?;
    let body = bee_body.ok_or(FigureError::InvalidRecipe)?;
    let matrix = j3d_model::mmul(
        body,
        wing_pose.get(joint).ok_or(FigureError::InvalidRecipe)?,
    );
    let visible = sources.wing_visibility(animation, shape, frame)?;
    Ok((matrix, visible))
}

fn key_values(
    sources: &mut FigureSources<'_>,
    figure: &str,
    clip: &FigureClip,
    groups: &BTreeMap<String, SkinGroup>,
    parts: &[Vec<Value>],
    part_rest_points: &[[f64; 3]],
) -> Result<(Vec<i64>, Vec<bool>), FigureError> {
    let group = groups.get(&clip.group).ok_or(FigureError::InvalidRecipe)?;
    let mut values = Vec::new();
    let mut negatives = Vec::new();
    for index in 0..clip.frame_count {
        let frame = clip.start + index as i64 * clip.step;
        let pose = sources.mario_pose(Some(&clip.animation), frame)?;
        for vertex in 0..group.bones.len() {
            let point = skin(
                &pose,
                &group.bones[vertex],
                &group.weights[vertex],
                group.rest[vertex],
            )?;
            for value in point {
                values.push(round_int(value, 1e5));
                negatives.push(value < 0.0);
            }
        }
        if let Some((wing_name, fixed)) = &clip.wing {
            let (wing_frame, wing_pose) =
                sources.wing_frame(wing_name, frame, clip.start, *fixed)?;
            let spine = sources.joint("Spine2")?;
            let body = j3d_model::mmul(&pose[spine], &sources.mario.bind[spine]);
            let bee_body = rigid_of(&body);
            let mut points = Vec::with_capacity(parts.len());
            for (part_index, part) in parts.iter().enumerate() {
                let (matrix, visible) = part_matrix(
                    sources,
                    part.as_slice(),
                    &pose,
                    Some((wing_name, wing_frame, &wing_pose)),
                    Some(&bee_body),
                )?;
                let local = *part_rest_points
                    .get(part_index)
                    .ok_or(FigureError::InvalidRecipe)?;
                let global = mat_apply(&matrix, local).map(|value| value * OBJ_SCALE);
                points.push((global, visible));
            }
            for triangle in points.chunks_exact(3) {
                if triangle[0].1 {
                    for (point, _) in triangle {
                        for value in point {
                            values.push(round_int(*value, 1e5));
                            negatives.push(*value < 0.0);
                        }
                    }
                } else {
                    let center: [f64; 3] = std::array::from_fn(|axis| {
                        (triangle[0].0[axis] + triangle[1].0[axis] + triangle[2].0[axis]) / 3.0
                    });
                    for _ in 0..3 {
                        for value in center {
                            values.push(round_int(value, 1e5));
                            negatives.push(value < 0.0);
                        }
                    }
                }
            }
        } else if figure == "mariobee" {
            return Err(FigureError::InvalidRecipe);
        }
    }
    Ok((values, negatives))
}

fn render_keyframes(
    figure: &str,
    clips: &[FigureClip],
    values: &[i64],
    negatives: &[bool],
) -> Result<Vec<(String, Vec<u8>)>, FigureError> {
    let total_frames: usize = clips.iter().map(|clip| clip.frame_count).sum();
    if total_frames == 0 || values.len() % total_frames != 0 || values.len() % 3 != 0 {
        return Err(FigureError::InvalidRecipe);
    }
    let vertices = values.len() / total_frames / 3;
    let mut cursor = 0usize;
    let mut files = Vec::with_capacity(total_frames);
    for clip in clips {
        for frame in 0..clip.frame_count {
            let mut bytes = Vec::with_capacity(vertices * 36 + 1);
            for _ in 0..vertices {
                bytes.extend_from_slice(b"v ");
                for axis in 0..3 {
                    if axis != 0 {
                        bytes.push(b' ');
                    }
                    append_formatted(
                        &mut bytes,
                        *values.get(cursor).ok_or(FigureError::InvalidRecipe)?,
                        5,
                        *negatives.get(cursor).ok_or(FigureError::InvalidRecipe)?,
                    );
                    cursor += 1;
                }
                bytes.push(b'\n');
            }
            let name = format!("assets/figures/{}/k_{}_{:02}.obj", figure, clip.name, frame);
            files.push((name, bytes));
        }
    }
    if cursor != values.len() {
        return Err(FigureError::InvalidRecipe);
    }
    Ok(files)
}

/// Build every Mario figure base asset and animation keyframe from the selected archives.
pub fn generate_all(
    mario_archive: &[u8],
    mario_anime_archive: &[u8],
    wing_archive: &[u8],
) -> Result<Vec<(String, Vec<u8>)>, FigureError> {
    let mut files = generate_static(mario_archive)?;
    let pack = asset_packs::EMBEDDED_PACKS
        .iter()
        .find(|p| p.name == PACK)
        .ok_or(FigureError::MissingRecipe)?;
    let meta: Value = serde_json::from_slice(
        asset_packs::get(PACK, "meta.json").ok_or(FigureError::MissingRecipe)?,
    )
    .map_err(|_| FigureError::InvalidRecipe)?;
    let recipe_entries = pack
        .entries
        .iter()
        .map(|(name, offset, size)| (name.to_string(), *offset, *size))
        .collect::<Vec<_>>();
    let predicted = anchors(
        &J3dModel::parse(
            &asset_formats::rarc_find(mario_archive, "mario.bdl")
                .map_err(|_| FigureError::InvalidArchive)?,
        )
        .map_err(|_| FigureError::InvalidModel)?,
        &meta,
    )?;
    let mut vsets = BTreeMap::new();
    for (name, (values, negative)) in predicted {
        vsets.insert(
            name.clone(),
            residual_apply(
                &format!("vset:{name}"),
                values,
                &negative,
                pack.data,
                &recipe_entries,
            )?,
        );
    }
    let groups = build_groups(&meta, &vsets)?;
    let figure_names = meta
        .get("figures")
        .and_then(Value::as_object)
        .ok_or(FigureError::InvalidRecipe)?
        .keys()
        .cloned()
        .collect::<Vec<_>>();
    let mut sources = FigureSources::new(mario_archive, mario_anime_archive, wing_archive, &meta)?;
    let all_parts = array(meta.get("parts").ok_or(FigureError::InvalidRecipe)?)?;
    let part_pd = array(meta.get("part_pd").ok_or(FigureError::InvalidRecipe)?)?;
    for figure in figure_names {
        let clips = parse_clips(&meta, &figure)?;
        let mut parts = Vec::new();
        let mut rest_points = Vec::new();
        if figure == "mariobee" {
            let bee = vsets
                .get("beeparts")
                .ok_or(FigureError::InvalidRecipe)?
                .0
                .clone();
            for (index, part) in all_parts.iter().enumerate() {
                if index >= part_pd.len() {
                    return Err(FigureError::InvalidRecipe);
                }
                let base = bee
                    .get(index * 3..index * 3 + 3)
                    .ok_or(FigureError::InvalidRecipe)?;
                let point = rest_point(base, array(&part_pd[index])?)?;
                parts.push(array(part)?.to_vec());
                rest_points.push(part_rest(&mut sources, &parts[index], point)?);
            }
        }
        let mut values = Vec::new();
        let mut negatives = Vec::new();
        for clip in &clips {
            let (clip_values, clip_negatives) =
                key_values(&mut sources, &figure, clip, &groups, &parts, &rest_points)?;
            values.extend(clip_values);
            negatives.extend(clip_negatives);
        }
        let (values, negatives) = residual_apply(
            &format!("keys:{figure}"),
            values,
            &negatives,
            pack.data,
            &recipe_entries,
        )?;
        files.extend(render_keyframes(&figure, &clips, &values, &negatives)?);
    }
    Ok(files)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "requires the extracted source tree and native construction output"]
    fn static_recipe_assets_match_native_outputs() {
        let source_root = std::env::var("SMG_SOURCE_ROOT").expect("set SMG_SOURCE_ROOT");
        let expected_root = std::env::var("SMG_EXPECTED_ASSETS").expect("set SMG_EXPECTED_ASSETS");
        let archive =
            std::fs::read(format!("{source_root}/ObjectData/Mario.arc")).expect("read Mario.arc");
        let generated = generate_static(&archive).expect("generate static Mario figure assets");
        for (name, bytes) in generated {
            let relative = name.strip_prefix("assets/").expect("asset name prefix");
            let expected_path = std::path::Path::new(&expected_root).join(relative);
            let expected = std::fs::read(&expected_path).unwrap_or_else(|error| {
                panic!("read expected asset {}: {error}", expected_path.display())
            });
            if bytes != expected {
                let offset = bytes
                    .iter()
                    .zip(&expected)
                    .position(|(a, b)| a != b)
                    .unwrap_or(bytes.len().min(expected.len()));
                let start = offset.saturating_sub(48);
                let generated_end = (offset + 96).min(bytes.len());
                let expected_end = (offset + 96).min(expected.len());
                eprintln!(
                    "first diff at {offset}: generated={:?}; expected={:?}",
                    String::from_utf8_lossy(&bytes[start..generated_end]),
                    String::from_utf8_lossy(&expected[start..expected_end])
                );
            }
            assert_eq!(
                bytes,
                expected,
                "native output differs: {}",
                expected_path.display()
            );
        }
    }

    #[test]
    #[ignore = "requires extracted Mario archives and native construction output"]
    fn complete_recipe_assets_match_native_outputs() {
        let source_root = std::env::var("SMG_SOURCE_ROOT").expect("set SMG_SOURCE_ROOT");
        let expected_root = std::env::var("SMG_EXPECTED_ASSETS").expect("set SMG_EXPECTED_ASSETS");
        let mario =
            std::fs::read(format!("{source_root}/ObjectData/Mario.arc")).expect("read Mario.arc");
        let anime = std::fs::read(format!("{source_root}/ObjectData/MarioAnime.arc"))
            .expect("read MarioAnime.arc");
        let wing = std::fs::read(format!("{source_root}/ObjectData/BeeMarioWing.arc"))
            .expect("read BeeMarioWing.arc");
        let generated =
            generate_all(&mario, &anime, &wing).expect("generate all Mario figure assets");
        for (name, bytes) in generated {
            let relative = name.strip_prefix("assets/").expect("asset name prefix");
            let expected_path = std::path::Path::new(&expected_root).join(relative);
            let expected = std::fs::read(&expected_path).unwrap_or_else(|error| {
                panic!("read expected asset {}: {error}", expected_path.display())
            });
            if bytes != expected {
                let offset = bytes
                    .iter()
                    .zip(&expected)
                    .position(|(a, b)| a != b)
                    .unwrap_or(bytes.len().min(expected.len()));
                let start = offset.saturating_sub(48);
                let generated_end = (offset + 96).min(bytes.len());
                let expected_end = (offset + 96).min(expected.len());
                eprintln!(
                    "first diff at {offset}: generated={:?}; expected={:?}",
                    String::from_utf8_lossy(&bytes[start..generated_end]),
                    String::from_utf8_lossy(&expected[start..expected_end])
                );
            }
            assert_eq!(
                bytes,
                expected,
                "native output differs: {}",
                expected_path.display()
            );
        }
    }
}

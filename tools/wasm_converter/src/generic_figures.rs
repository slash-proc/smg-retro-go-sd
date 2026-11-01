//! Interpreter for the static, unskinned `figures.pak` recipes.

use crate::{
    asset_formats, asset_packs,
    j3d_animation::{self, Bck, RawJoint},
    j3d_model::{minv, mmul, J3dModel},
};
use serde_json::Value;
use std::{
    collections::{BTreeMap, HashMap},
    rc::Rc,
};

const PACK: &str = "figures/figures.pak";
const POW10: [f64; 31] = [
    1e-15, 1e-14, 1e-13, 1e-12, 1e-11, 1e-10, 1e-9, 1e-8, 1e-7, 1e-6, 1e-5, 1e-4, 1e-3, 1e-2, 1e-1,
    1.0, 1e1, 1e2, 1e3, 1e4, 1e5, 1e6, 1e7, 1e8, 1e9, 1e10, 1e11, 1e12, 1e13, 1e14, 1e15,
];

fn pow10(exponent: i32) -> f64 {
    if (-15..=15).contains(&exponent) {
        POW10[(exponent + 15) as usize]
    } else {
        10f64.powi(exponent)
    }
}

#[derive(Debug)]
pub enum FigureError {
    MissingRecipe,
    InvalidRecipe,
    InvalidStream,
    MissingArchive,
    InvalidModel,
    MissingReference,
    UnsupportedPose,
    InvalidAnimation,
    InvalidProfile(usize, usize),
    FileFailed(String, Box<FigureError>),
    OperationFailed(String, Box<FigureError>),
    InvalidStreamAt(&'static str),
    InvalidAnchor(usize, usize),
    RecipeFailed(String, Box<FigureError>),
}

#[derive(Clone, Copy, Debug)]
struct Token {
    negative: bool,
    magnitude: u64,
    decimals: i32,
}

impl Token {
    fn signed(self) -> i64 {
        let value = self.magnitude as i64;
        if self.negative {
            -value
        } else {
            value
        }
    }

    fn as_f64(self) -> f64 {
        let scale = if self.decimals < 0 {
            1.0
        } else {
            10f64.powi(self.decimals)
        };
        let value = self.magnitude as f64 / scale;
        if self.negative {
            -value
        } else {
            value
        }
    }

    fn append_render(self, out: &mut Vec<u8>) {
        let mut buffer = itoa::Buffer::new();
        let digits = buffer.format(self.magnitude).as_bytes();
        if self.negative {
            out.push(b'-');
        }
        if self.decimals < 0 {
            out.extend_from_slice(digits);
        } else if self.decimals == 0 {
            out.extend_from_slice(digits);
            out.push(b'.');
        } else {
            let decimals = self.decimals as usize;
            if digits.len() <= decimals {
                out.extend_from_slice(b"0.");
                out.resize(out.len() + decimals - digits.len(), b'0');
                out.extend_from_slice(digits);
            } else {
                out.extend_from_slice(&digits[..digits.len() - decimals]);
                out.push(b'.');
                out.extend_from_slice(&digits[digits.len() - decimals..]);
            }
        }
    }
}

fn append_i64(value: i64, out: &mut Vec<u8>) {
    let mut buffer = itoa::Buffer::new();
    out.extend_from_slice(buffer.format(value).as_bytes());
}

fn parse_token(text: &str) -> Result<Token, FigureError> {
    let negative = text.starts_with('-');
    let body = text.strip_prefix('-').unwrap_or(text);
    if let Some((whole, fraction)) = body.split_once('.') {
        let digits = format!("{whole}{fraction}");
        Ok(Token {
            negative,
            magnitude: digits.parse().map_err(|_| FigureError::InvalidRecipe)?,
            decimals: fraction.len() as i32,
        })
    } else {
        Ok(Token {
            negative,
            magnitude: body.parse().map_err(|_| FigureError::InvalidRecipe)?,
            decimals: -1,
        })
    }
}

fn fixed_token(value: f64, digits: usize) -> Result<Token, FigureError> {
    if value.is_finite() && digits <= 15 {
        let scale = pow10(digits as i32);
        let scaled = value * scale;
        let floor = scaled.floor();
        let fraction = scaled - floor;
        let nearest = if fraction > 0.5 { floor + 1.0 } else { floor };
        let uncertainty = (scaled.abs() + 1.0) * f64::EPSILON * 4.0;
        let distance_to_half = (fraction - 0.5).abs();
        if nearest.is_finite()
            && nearest > i64::MIN as f64
            && nearest < i64::MAX as f64
            && distance_to_half > uncertainty
        {
            let integer = nearest as i64;
            return Ok(Token {
                negative: value.is_sign_negative(),
                magnitude: integer.unsigned_abs(),
                decimals: if digits == 0 { -1 } else { digits as i32 },
            });
        }
    }
    parse_token(&format!("{value:.digits$}"))
}

fn general_token(value: f64, significant: usize) -> Result<Token, FigureError> {
    if !value.is_finite() {
        return Err(FigureError::InvalidRecipe);
    }
    let absolute = value.abs();
    let mut exponent = if absolute == 0.0 {
        0
    } else {
        absolute.log10().floor() as i32
    };
    if absolute != 0.0 {
        while absolute < pow10(exponent) {
            exponent -= 1;
        }
        while absolute >= pow10(exponent + 1) {
            exponent += 1;
        }

        // `%g` rounds to the requested significant digits before choosing
        // fixed versus exponential notation. Use a cheap boundary check and
        // preserve the exact formatter for the narrow tie/roundover region.
        let next_power = pow10(exponent + 1);
        let half_unit = 0.5 * pow10(exponent - significant as i32 + 1);
        let roundover = next_power - half_unit;
        let uncertainty = roundover.abs().max(1.0) * f64::EPSILON * 8.0;
        if (absolute - roundover).abs() <= uncertainty {
            return general_token_formatted(value, significant);
        }
        if absolute > roundover {
            exponent += 1;
        }
    }
    // Python's %g switches to exponential form at these limits, then the
    // native recipe parser expands exponent notation to fifteen decimals.
    if exponent < -4 || exponent >= significant as i32 {
        fixed_token(value, 15)
    } else {
        let digits = (significant as i32 - exponent - 1).max(0) as usize;
        let mut token = fixed_token(value, digits)?;
        while token.decimals > 0 && token.magnitude % 10 == 0 {
            token.magnitude /= 10;
            token.decimals -= 1;
        }
        if token.decimals == 0 {
            token.decimals = -1;
        }
        Ok(token)
    }
}

fn general_token_formatted(value: f64, significant: usize) -> Result<Token, FigureError> {
    let exponent_text = format!(
        "{value:.precision$e}",
        precision = significant.saturating_sub(1)
    );
    let exponent = exponent_text
        .rsplit_once('e')
        .and_then(|(_, e)| e.parse::<i32>().ok())
        .ok_or(FigureError::InvalidRecipe)?;
    if exponent < -4 || exponent >= significant as i32 {
        fixed_token(value, 15)
    } else {
        let digits = (significant as i32 - exponent - 1).max(0) as usize;
        let mut text = format!("{value:.digits$}");
        if text.contains('.') {
            while text.ends_with('0') {
                text.pop();
            }
            if text.ends_with('.') {
                text.pop();
            }
        }
        parse_token(&text)
    }
}

#[derive(Clone)]
struct IntReader<'a> {
    bytes: &'a [u8],
    cursor: usize,
}

impl<'a> IntReader<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, cursor: 0 }
    }

    fn unsigned(&mut self) -> Result<u64, FigureError> {
        let mut value = 0u64;
        let mut shift = 0u32;
        loop {
            let byte = *self
                .bytes
                .get(self.cursor)
                .ok_or(FigureError::InvalidStream)?;
            self.cursor += 1;
            if shift >= 64 || (shift == 63 && byte & 0x7e != 0) {
                return Err(FigureError::InvalidStream);
            }
            value |= ((byte & 0x7f) as u64) << shift;
            if byte < 0x80 {
                return Ok(value);
            }
            shift += 7;
        }
    }

    fn int(&mut self) -> Result<i64, FigureError> {
        let value = self.unsigned()?;
        Ok(((value >> 1) as i64) ^ (-((value & 1) as i64)))
    }
}

#[derive(Clone)]
struct Source {
    model: J3dModel,
    points: Vec<[f64; 3]>,
    vertex_ids: Vec<Vec<usize>>,
    uvs: Vec<[f64; 2]>,
    triangles: Vec<[usize; 3]>,
    raw_points: Vec<[f64; 3]>,
    raw_triangles: Vec<[usize; 3]>,
}

fn fkey3(value: [f64; 3]) -> [u64; 3] {
    value.map(|x| {
        if x == 0.0 {
            0.0f64.to_bits()
        } else {
            x.to_bits()
        }
    })
}
fn fkey2(value: [f64; 2]) -> [u64; 2] {
    value.map(|x| {
        if x == 0.0 {
            0.0f64.to_bits()
        } else {
            x.to_bits()
        }
    })
}

impl Source {
    fn new(archive: &[u8], spec: &str, unhide: bool) -> Result<Self, FigureError> {
        let (_, model_name) = spec.split_once(':').ok_or(FigureError::InvalidRecipe)?;
        let bytes =
            asset_formats::rarc_find(archive, model_name).map_err(|_| FigureError::InvalidModel)?;
        let mut model = J3dModel::parse(&bytes).map_err(|_| FigureError::InvalidModel)?;
        if unhide {
            model.unhide_zero_scale();
        }
        let mut point_ids = HashMap::<[u64; 3], usize>::new();
        let mut points = Vec::new();
        let mut vertex_ids = Vec::with_capacity(model.shapes.len());
        let mut triangles = Vec::new();
        for shape in &model.shapes {
            let mut ids = Vec::with_capacity(shape.vertices.len());
            for vertex in &shape.vertices {
                let position = *vertex.attributes.get(&9).unwrap_or(&0);
                let point = model
                    .anchor_point(position, vertex.draw_matrix)
                    .ok_or(FigureError::InvalidModel)?;
                let key = fkey3(point);
                let id = *point_ids.entry(key).or_insert_with(|| {
                    let id = points.len();
                    points.push(point);
                    id
                });
                ids.push(id);
            }
            triangles.extend(
                shape
                    .triangles
                    .iter()
                    .map(|t| [ids[t[0]], ids[t[1]], ids[t[2]]]),
            );
            vertex_ids.push(ids);
        }

        let mut uv_ids = HashMap::<[u64; 2], usize>::new();
        let mut uvs = Vec::new();
        for shape in &model.shapes {
            for vertex in &shape.vertices {
                if let Some(&index) = vertex.attributes.get(&13) {
                    if let Some(&uv) = model.uvs.get(index) {
                        let key = fkey2(uv);
                        uv_ids.entry(key).or_insert_with(|| {
                            let id = uvs.len();
                            uvs.push(uv);
                            id
                        });
                    }
                }
            }
        }

        let mut raw_ids = HashMap::<[u64; 3], usize>::new();
        let mut raw_points = Vec::new();
        let mut raw_point_indices = Vec::with_capacity(model.positions.len());
        for point in &model.positions {
            let key = fkey3(*point);
            let id = *raw_ids.entry(key).or_insert_with(|| {
                let id = raw_points.len();
                raw_points.push(*point);
                id
            });
            raw_point_indices.push(id);
        }
        let mut raw_triangles = Vec::new();
        for shape in &model.shapes {
            let ids = shape
                .vertices
                .iter()
                .map(|vertex| {
                    let pos = *vertex.attributes.get(&9).unwrap_or(&0);
                    raw_point_indices
                        .get(pos.min(raw_point_indices.len().saturating_sub(1)))
                        .copied()
                        .ok_or(FigureError::InvalidModel)
                })
                .collect::<Result<Vec<_>, _>>()?;
            raw_triangles.extend(
                shape
                    .triangles
                    .iter()
                    .map(|t| [ids[t[0]], ids[t[1]], ids[t[2]]]),
            );
        }
        Ok(Self {
            model,
            points,
            vertex_ids,
            uvs,
            triangles,
            raw_points,
            raw_triangles,
        })
    }

    fn posed_points(
        &self,
        overrides: &[(usize, bool, [f64; 3])],
    ) -> Result<Vec<[f64; 3]>, FigureError> {
        let raw = self
            .model
            .joints
            .iter()
            .map(|joint| RawJoint {
                scale: joint.scale,
                rotation: joint.rotation,
                translation: joint.translation,
            })
            .collect::<Vec<_>>();
        let world = j3d_animation::fitted_world_overrides(&self.model, &raw, overrides);
        let bind = j3d_animation::world_matrices(&self.model, &raw, true);
        let matrices = world
            .iter()
            .zip(&bind)
            .map(|(world, bind)| mmul(world, &minv(bind)))
            .collect::<Vec<_>>();
        let mut posed = vec![None; self.points.len()];
        for (shape, ids) in self.model.shapes.iter().zip(&self.vertex_ids) {
            for (vertex, &point_id) in shape.vertices.iter().zip(ids) {
                if posed[point_id].is_some() {
                    continue;
                }
                let mut point = [0.0; 3];
                for (joint, weight) in self
                    .model
                    .influences(vertex.draw_matrix)
                    .ok_or(FigureError::InvalidModel)?
                {
                    let transformed = Figure::apply(
                        matrices.get(joint).ok_or(FigureError::InvalidModel)?,
                        self.points[point_id],
                    );
                    for axis in 0..3 {
                        point[axis] += weight * transformed[axis];
                    }
                }
                posed[point_id] = Some(point);
            }
        }
        posed
            .into_iter()
            .map(|point| point.ok_or(FigureError::InvalidModel))
            .collect()
    }
}

#[derive(Default, Clone)]
struct FileRec {
    vertices: Vec<Vec<Token>>,
    texcoords: Vec<Vec<Token>>,
    anchors: Vec<usize>,
}

type Profile = Vec<(usize, f64)>;
type Matrix = [f64; 12];

#[derive(Clone, Copy)]
enum NumberFormat {
    Quantized { digits: usize, grid: f64 },
    Fixed(usize),
    General(usize),
}

impl NumberFormat {
    fn parse(text: &str) -> Result<Self, FigureError> {
        match text.as_bytes().first() {
            Some(b'q') => {
                let (digits, grid) = text
                    .get(1..)
                    .and_then(|s| s.split_once(':'))
                    .ok_or(FigureError::InvalidRecipe)?;
                Ok(Self::Quantized {
                    digits: digits.parse().map_err(|_| FigureError::InvalidRecipe)?,
                    grid: grid.parse().map_err(|_| FigureError::InvalidRecipe)?,
                })
            }
            Some(b'f') => Ok(Self::Fixed(
                text[1..].parse().map_err(|_| FigureError::InvalidRecipe)?,
            )),
            Some(b'g') => Ok(Self::General(
                text[1..].parse().map_err(|_| FigureError::InvalidRecipe)?,
            )),
            _ => Err(FigureError::InvalidRecipe),
        }
    }
}

#[derive(Clone, Debug)]
struct PoseKey {
    animation: i64,
    frame: i64,
    divisor: i64,
    overrides: Vec<(usize, bool, [f64; 3])>,
}

struct Skin {
    source: usize,
    bcks: Vec<Bck>,
    inverse_rest: Vec<Matrix>,
    weights: Vec<Profile>,
    fine: i32,
    world: u64,
    bkey: Option<PoseKey>,
    bkey_matrices: Option<Vec<Matrix>>,
    rest: Vec<[f64; 3]>,
}

struct Figure<'a> {
    cfg: &'a Value,
    archives: &'a BTreeMap<String, Vec<u8>>,
    sources: Vec<Rc<Source>>,
    streams: [IntReader<'a>; 4], // m, k, u, f
    files: BTreeMap<String, FileRec>,
    clouds: HashMap<String, Vec<Vec<f64>>>,
    perm: [usize; 3],
    sign: [f64; 3],
    scale: f64,
    offset: [f64; 3],
    skin: Option<Skin>,
}

fn value_array<'a>(value: &'a Value) -> Result<&'a [Value], FigureError> {
    value
        .as_array()
        .map(Vec::as_slice)
        .ok_or(FigureError::InvalidRecipe)
}
fn value_usize(value: &Value) -> Result<usize, FigureError> {
    value
        .as_u64()
        .and_then(|x| usize::try_from(x).ok())
        .ok_or(FigureError::InvalidRecipe)
}
fn value_str(value: &Value) -> Result<&str, FigureError> {
    value.as_str().ok_or(FigureError::InvalidRecipe)
}

fn parse_pose_key(value: &Value) -> Result<PoseKey, FigureError> {
    let key = value_array(value)?;
    if key.len() < 3 {
        return Err(FigureError::InvalidRecipe);
    }
    let mut overrides = Vec::new();
    if let Some(value) = key.get(3) {
        for item in value_array(value)? {
            let item = value_array(item)?;
            if item.len() != 5 {
                return Err(FigureError::InvalidRecipe);
            }
            overrides.push((
                value_usize(&item[0])?,
                value_usize(&item[1])? != 0,
                [
                    item[2].as_f64().ok_or(FigureError::InvalidRecipe)?,
                    item[3].as_f64().ok_or(FigureError::InvalidRecipe)?,
                    item[4].as_f64().ok_or(FigureError::InvalidRecipe)?,
                ],
            ));
        }
    }
    Ok(PoseKey {
        animation: key[0].as_i64().ok_or(FigureError::InvalidRecipe)?,
        frame: key[1].as_i64().ok_or(FigureError::InvalidRecipe)?,
        divisor: key[2].as_i64().ok_or(FigureError::InvalidRecipe)?,
        overrides,
    })
}

fn profiles(model: &J3dModel) -> Result<Vec<Profile>, FigureError> {
    let mut result = vec![Vec::new()];
    for joint in 0..model.joints.len() {
        result.push(vec![(joint, 1.0)]);
    }
    for draw in 0..model.draw_matrices.len() {
        let mut influences = model.influences(draw).ok_or(FigureError::InvalidModel)?;
        influences.sort_by(|(aj, aw), (bj, bw)| bw.total_cmp(aw).then_with(|| aj.cmp(bj)));
        for limit in [2, 3, 4] {
            let mut profile = influences.iter().take(limit).copied().collect::<Profile>();
            let total = fsum(profile.iter().map(|(_, weight)| *weight));
            let total = if total == 0.0 { 1.0 } else { total };
            for (_, weight) in &mut profile {
                *weight /= total;
            }
            if !result.contains(&profile) {
                result.push(profile);
            }
        }
    }
    result.sort_by(|a, b| {
        for ((aj, aw), (bj, bw)) in a.iter().zip(b) {
            let order = aj.cmp(bj).then_with(|| aw.total_cmp(bw));
            if !order.is_eq() {
                return order;
            }
        }
        a.len().cmp(&b.len())
    });
    Ok(result)
}

fn fsum(values: impl IntoIterator<Item = f64>) -> f64 {
    let mut iter = values.into_iter();
    let Some(mut total) = iter.next() else {
        return 0.0;
    };
    let mut compensation = 0.0;
    for value in iter {
        let next = total + value;
        if total.abs() >= value.abs() {
            compensation += (total - next) + value;
        } else {
            compensation += (value - next) + total;
        }
        total = next;
    }
    if compensation.is_finite() {
        total + compensation
    } else {
        total
    }
}

impl<'a> Figure<'a> {
    fn new(
        cfg: &'a Value,
        archives: &'a BTreeMap<String, Vec<u8>>,
        streams: [&'a [u8]; 4],
        source_cache: &mut HashMap<SourceKey, Rc<Source>>,
        remaining_uses: &HashMap<SourceKey, usize>,
    ) -> Result<Self, FigureError> {
        let src_values = value_array(cfg.get("src").ok_or(FigureError::InvalidRecipe)?)?;
        let unhide = cfg
            .get("unhide")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        let mut sources = Vec::with_capacity(src_values.len());
        for (index, item) in src_values.iter().enumerate() {
            let spec = value_str(item)?;
            let (archive_name, model_name) =
                spec.split_once(':').ok_or(FigureError::InvalidRecipe)?;
            let archive = archives
                .get(archive_name)
                .ok_or(FigureError::MissingArchive)?;
            let unhide = unhide
                .iter()
                .any(|value| value.as_u64() == Some(index as u64));
            let key = (archive_name.to_owned(), model_name.to_owned(), unhide);
            let source = if let Some(source) = source_cache.get(&key) {
                Rc::clone(source)
            } else {
                let source = Rc::new(Source::new(archive, spec, unhide)?);
                if remaining_uses.get(&key).copied().unwrap_or(0) > 1 {
                    source_cache.insert(key, Rc::clone(&source));
                }
                source
            };
            sources.push(source);
        }
        let identity = [0u8, 1, 2];
        let default_xf = Value::Null;
        let xf = cfg.get("xf").unwrap_or(&default_xf);
        let perm = if xf.is_null() {
            identity.map(|x| x as usize)
        } else {
            value_array(xf.get("perm").ok_or(FigureError::InvalidRecipe)?)?
                .iter()
                .map(value_usize)
                .collect::<Result<Vec<_>, _>>()?
                .try_into()
                .map_err(|_| FigureError::InvalidRecipe)?
        };
        let sign = if xf.is_null() {
            [1.0; 3]
        } else {
            value_array(xf.get("sign").ok_or(FigureError::InvalidRecipe)?)?
                .iter()
                .map(|v| v.as_f64().ok_or(FigureError::InvalidRecipe))
                .collect::<Result<Vec<_>, _>>()?
                .try_into()
                .map_err(|_| FigureError::InvalidRecipe)?
        };
        let scale = xf.get("s").and_then(Value::as_f64).unwrap_or(0.004);
        let offset = if let Some(t) = xf.get("t") {
            value_array(t)?
                .iter()
                .map(|v| v.as_f64().ok_or(FigureError::InvalidRecipe))
                .collect::<Result<Vec<_>, _>>()?
                .try_into()
                .map_err(|_| FigureError::InvalidRecipe)?
        } else {
            [0.0; 3]
        };
        Ok(Self {
            cfg,
            archives,
            sources,
            streams: streams.map(IntReader::new),
            files: BTreeMap::new(),
            clouds: HashMap::new(),
            perm,
            sign,
            scale,
            offset,
            skin: None,
        })
    }

    fn transform(&self, point: [f64; 3]) -> [f64; 3] {
        core::array::from_fn(|i| self.sign[i] * point[self.perm[i]] * self.scale + self.offset[i])
    }

    fn inverse_transform(&self, point: [f64; 3]) -> [f64; 3] {
        let mut result = [0.0; 3];
        for axis in 0..3 {
            result[self.perm[axis]] =
                (point[axis] - self.offset[axis]) * self.sign[axis] / self.scale;
        }
        result
    }

    fn setup_skin(&mut self, spec: &Value, vertex_count: usize) -> Result<(), FigureError> {
        let source = value_usize(spec.get("src").ok_or(FigureError::InvalidRecipe)?)?;
        let archive_name = value_str(spec.get("arc").ok_or(FigureError::InvalidRecipe)?)?;
        let archive = self
            .archives
            .get(archive_name)
            .ok_or(FigureError::MissingArchive)?;
        let bck_names = value_array(spec.get("bcks").ok_or(FigureError::InvalidRecipe)?)?;
        let bcks = bck_names
            .iter()
            .map(|name| {
                let name = value_str(name)?;
                let bytes = asset_formats::rarc_find(archive, name)
                    .map_err(|_| FigureError::InvalidAnimation)?;
                Bck::parse(&bytes).map_err(|_| FigureError::InvalidAnimation)
            })
            .collect::<Result<Vec<_>, _>>()?;
        let model = &self
            .sources
            .get(source)
            .ok_or(FigureError::InvalidRecipe)?
            .model;
        let raw_rest = model
            .joints
            .iter()
            .map(|joint| RawJoint {
                scale: joint.scale.map(|x| if x.abs() <= 1e-6 { 1.0 } else { x }),
                rotation: joint.rotation,
                translation: joint.translation,
            })
            .collect::<Vec<_>>();
        let inverse_rest = j3d_animation::world_matrices(model, &raw_rest, true)
            .iter()
            .map(minv)
            .collect::<Vec<_>>();
        let profiles = profiles(model)?;
        let mut weights = Vec::with_capacity(vertex_count);
        for _ in 0..vertex_count {
            let index = usize::try_from(self.streams[0].unsigned()?)
                .map_err(|_| FigureError::InvalidStream)?;
            weights.push(
                profiles
                    .get(index)
                    .cloned()
                    .ok_or(FigureError::InvalidProfile(index, profiles.len()))?,
            );
        }
        let bkey = spec.get("bkey").map(parse_pose_key).transpose()?;
        self.skin = Some(Skin {
            source,
            bcks,
            inverse_rest,
            weights,
            fine: spec.get("fine").and_then(Value::as_i64).unwrap_or(0) as i32,
            world: spec.get("exact").and_then(Value::as_u64).unwrap_or(0),
            bkey,
            bkey_matrices: None,
            rest: Vec::with_capacity(vertex_count),
        });
        let bkey = self.skin.as_ref().and_then(|skin| skin.bkey.clone());
        if let Some(key) = bkey {
            let matrices = self.skin_matrices(&key)?;
            self.skin.as_mut().unwrap().bkey_matrices = Some(matrices);
        }
        Ok(())
    }

    fn skin_matrices(&self, key: &PoseKey) -> Result<Vec<Matrix>, FigureError> {
        let skin = self.skin.as_ref().ok_or(FigureError::InvalidRecipe)?;
        let model = &self
            .sources
            .get(skin.source)
            .ok_or(FigureError::InvalidRecipe)?
            .model;
        let raw = if key.animation < 0 {
            model
                .joints
                .iter()
                .map(|joint| RawJoint {
                    scale: joint.scale,
                    rotation: joint.rotation,
                    translation: joint.translation,
                })
                .collect::<Vec<_>>()
        } else {
            let bck = skin
                .bcks
                .get(key.animation as usize)
                .ok_or(FigureError::InvalidRecipe)?;
            if key.divisor == 0 {
                return Err(FigureError::InvalidRecipe);
            }
            bck.raw(model, key.frame as f64 / key.divisor as f64)
        };
        let world = match skin.world {
            0 => j3d_animation::fitted_world_overrides(model, &raw, &key.overrides),
            1 => j3d_animation::world_matrices(model, &raw, true),
            2 => j3d_animation::world_matrices(model, &raw, false),
            _ => return Err(FigureError::InvalidRecipe),
        };
        Ok(world
            .iter()
            .zip(&skin.inverse_rest)
            .map(|(world, rest)| mmul(world, rest))
            .collect())
    }

    fn blend_matrix(matrices: &[Matrix], profile: &Profile) -> Option<Matrix> {
        if profile.is_empty() {
            return None;
        }
        let mut out = [0.0; 12];
        for &(joint, weight) in profile {
            let matrix = matrices.get(joint)?;
            for index in 0..12 {
                out[index] += weight * matrix[index];
            }
        }
        Some(out)
    }

    fn apply(matrix: &Matrix, point: [f64; 3]) -> [f64; 3] {
        [
            matrix[0] * point[0] + matrix[1] * point[1] + matrix[2] * point[2] + matrix[3],
            matrix[4] * point[0] + matrix[5] * point[1] + matrix[6] * point[2] + matrix[7],
            matrix[8] * point[0] + matrix[9] * point[1] + matrix[10] * point[2] + matrix[11],
        ]
    }

    fn determinant(matrix: &Matrix) -> f64 {
        matrix[0] * (matrix[5] * matrix[10] - matrix[6] * matrix[9])
            - matrix[1] * (matrix[4] * matrix[10] - matrix[6] * matrix[8])
            + matrix[2] * (matrix[4] * matrix[9] - matrix[5] * matrix[8])
    }

    fn rest_points(&mut self, rows: &[Vec<Token>], fmt: &str) -> Result<(), FigureError> {
        let skin = self.skin.as_ref().ok_or(FigureError::InvalidRecipe)?;
        let fine = skin.fine;
        let weights = skin.weights.clone();
        let bkey_matrices = skin.bkey_matrices.clone();
        let decimals = fmt
            .get(1..)
            .and_then(|s| s.split(':').next())
            .ok_or(FigureError::InvalidRecipe)?
            .parse::<i32>()
            .map_err(|_| FigureError::InvalidRecipe)?;
        let divisor = 10f64.powi(decimals + fine);
        let fine_scale = 10f64.powi(fine);
        let mut rest = Vec::with_capacity(rows.len());
        for (index, row) in rows.iter().enumerate() {
            if row.len() != 3 {
                return Err(FigureError::InvalidRecipe);
            }
            let mut point = [0.0; 3];
            for axis in 0..3 {
                let residual = self.streams[0].int()?;
                point[axis] = (row[axis].signed() as f64 * fine_scale + residual as f64) / divisor;
            }
            let mut point = self.inverse_transform(point);
            if let Some(matrices) = &bkey_matrices {
                if let Some(blended) = Self::blend_matrix(
                    matrices,
                    weights.get(index).ok_or(FigureError::InvalidRecipe)?,
                ) {
                    if Self::determinant(&blended).abs() > 1e-12 {
                        point = Self::apply(&minv(&blended), point);
                    }
                }
            }
            rest.push(point);
        }
        self.skin.as_mut().unwrap().rest = rest;
        Ok(())
    }

    fn skinned_points(&self, key: &PoseKey) -> Result<Vec<[f64; 3]>, FigureError> {
        let matrices = self.skin_matrices(key)?;
        let skin = self.skin.as_ref().ok_or(FigureError::InvalidRecipe)?;
        if skin.rest.len() != skin.weights.len() {
            return Err(FigureError::InvalidRecipe);
        }
        skin.rest
            .iter()
            .zip(&skin.weights)
            .map(|(&point, profile)| {
                let point = Self::blend_matrix(&matrices, profile)
                    .map(|matrix| Self::apply(&matrix, point))
                    .unwrap_or(point);
                Ok(self.transform(point))
            })
            .collect()
    }

    fn cloud(&mut self, spec: &str) -> Result<Vec<Vec<f64>>, FigureError> {
        if let Some(value) = self.clouds.get(spec) {
            return Ok(value.clone());
        }
        let (kind, argument) = spec.split_once(':').ok_or(FigureError::InvalidRecipe)?;
        let cloud = match kind {
            "bind" => {
                let mut result = Vec::new();
                for idx in argument.split(',') {
                    let i = idx
                        .parse::<usize>()
                        .map_err(|_| FigureError::InvalidRecipe)?;
                    let source = self.sources.get(i).ok_or(FigureError::InvalidRecipe)?;
                    result.extend(source.points.iter().map(|&p| self.transform(p).to_vec()));
                }
                result
            }
            "raw" => {
                let i = argument
                    .parse::<usize>()
                    .map_err(|_| FigureError::InvalidRecipe)?;
                let source = self.sources.get(i).ok_or(FigureError::InvalidRecipe)?;
                source
                    .raw_points
                    .iter()
                    .map(|&p| self.transform(p).to_vec())
                    .collect()
            }
            "uv" | "uvf" => {
                let i = argument
                    .parse::<usize>()
                    .map_err(|_| FigureError::InvalidRecipe)?;
                let source = self.sources.get(i).ok_or(FigureError::InvalidRecipe)?;
                source
                    .uvs
                    .iter()
                    .map(|&[u, v]| vec![u, if kind == "uvf" { 1.0 - v } else { v }])
                    .collect()
            }
            "pose" => {
                let source_index = argument
                    .parse::<usize>()
                    .map_err(|_| FigureError::InvalidRecipe)?;
                let pose = value_array(self.cfg.get("cpose").ok_or(FigureError::InvalidRecipe)?)?;
                let overrides = pose
                    .iter()
                    .map(|item| {
                        let values = value_array(item)?;
                        if values.len() != 5 {
                            return Err(FigureError::InvalidRecipe);
                        }
                        Ok((
                            value_usize(&values[0])?,
                            value_usize(&values[1])? != 0,
                            [
                                values[2].as_f64().ok_or(FigureError::InvalidRecipe)?,
                                values[3].as_f64().ok_or(FigureError::InvalidRecipe)?,
                                values[4].as_f64().ok_or(FigureError::InvalidRecipe)?,
                            ],
                        ))
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                self.sources
                    .get(source_index)
                    .ok_or(FigureError::InvalidRecipe)?
                    .posed_points(&overrides)?
                    .iter()
                    .map(|&point| self.transform(point).to_vec())
                    .collect()
            }
            _ => {
                let rec = self
                    .files
                    .get(argument)
                    .ok_or(FigureError::MissingReference)?;
                match kind {
                    "v" => rec
                        .vertices
                        .iter()
                        .map(|row| row.iter().map(|t| t.as_f64()).collect())
                        .collect(),
                    "vt" => rec
                        .texcoords
                        .iter()
                        .map(|row| row.iter().map(|t| t.as_f64()).collect())
                        .collect(),
                    _ => return Err(FigureError::InvalidRecipe),
                }
            }
        };
        self.clouds.insert(spec.to_owned(), cloud.clone());
        Ok(cloud)
    }

    fn texture_string(&self, text: &str) -> Result<String, FigureError> {
        let mut out = String::with_capacity(text.len());
        let mut rest = text;
        while let Some(open) = rest.find('{') {
            out.push_str(&rest[..open]);
            let tail = &rest[open + 1..];
            let close = tail.find('}').ok_or(FigureError::InvalidRecipe)?;
            let binding = tail[..close].get(1..).ok_or(FigureError::InvalidRecipe)?;
            let (src, index) = binding.split_once('.').ok_or(FigureError::InvalidRecipe)?;
            let src = src
                .parse::<usize>()
                .map_err(|_| FigureError::InvalidRecipe)?;
            let index = index
                .parse::<usize>()
                .map_err(|_| FigureError::InvalidRecipe)?;
            out.push_str(
                self.sources
                    .get(src)
                    .and_then(|s| s.model.texture_names.get(index))
                    .ok_or(FigureError::MissingReference)?,
            );
            rest = &tail[close + 1..];
        }
        out.push_str(rest);
        Ok(out)
    }

    fn numeric_rows(
        &mut self,
        count: usize,
        pred: &Value,
        file: &mut FileRec,
        name: &str,
    ) -> Result<Vec<Vec<Token>>, FigureError> {
        let kind = value_str(pred.get("p").ok_or(FigureError::InvalidRecipe)?)?;
        let dim = pred.get("dim").and_then(Value::as_u64).unwrap_or(3) as usize;
        let fmt = value_str(pred.get("fmt").ok_or(FigureError::InvalidRecipe)?)?;
        let number_format = NumberFormat::parse(fmt)?;
        let stream = match kind {
            "lit" => 2,
            "pose" => 1,
            _ => 0,
        };
        let mut reader = self.streams[stream].clone();
        let mut values = Vec::with_capacity(count * dim);
        match kind {
            "anchor" => {
                let cloud_spec = value_str(pred.get("c").ok_or(FigureError::InvalidRecipe)?)?;
                let cloud = self.cloud(cloud_spec)?;
                let posed = pred.get("posed").and_then(Value::as_u64).unwrap_or(0) != 0;
                let posed_data = if posed {
                    self.skin
                        .as_ref()
                        .and_then(|skin| skin.bkey_matrices.as_ref().map(|m| (skin.source, m)))
                } else {
                    None
                };
                let source_index = cloud_spec
                    .strip_prefix("bind:")
                    .and_then(|s| s.split(',').next())
                    .and_then(|s| s.parse::<usize>().ok());
                let mut point = -1i64;
                for vertex in 0..count {
                    point += 1 + reader
                        .int()
                        .map_err(|_| FigureError::InvalidStreamAt("anchor delta"))?;
                    let point_index =
                        usize::try_from(point).map_err(|_| FigureError::InvalidStream)?;
                    let row = cloud
                        .get(point_index)
                        .ok_or(FigureError::InvalidAnchor(point_index, cloud.len()))?;
                    file.anchors.push(point as usize);
                    if let (Some((source, matrices)), Some(source_index)) =
                        (posed_data, source_index)
                    {
                        if source != source_index {
                            return Err(FigureError::InvalidRecipe);
                        }
                        let skin = self.skin.as_ref().ok_or(FigureError::InvalidRecipe)?;
                        let profile = skin.weights.get(vertex).ok_or(FigureError::InvalidRecipe)?;
                        let base = self.sources[source]
                            .points
                            .get(point_index)
                            .copied()
                            .ok_or(FigureError::InvalidStream)?;
                        let point = Self::blend_matrix(matrices, profile)
                            .map(|m| Self::apply(&m, base))
                            .unwrap_or(base);
                        values.extend(self.transform(point).iter().take(dim).copied());
                    } else {
                        values.extend(row.iter().take(dim).copied());
                    }
                    if row.len() < dim {
                        return Err(FigureError::InvalidRecipe);
                    }
                }
            }
            "copy" => {
                let rows =
                    self.cloud(value_str(pred.get("c").ok_or(FigureError::InvalidRecipe)?)?)?;
                if rows.len() < count {
                    return Err(FigureError::InvalidStream);
                }
                for row in rows.iter().take(count) {
                    if row.len() < dim {
                        return Err(FigureError::InvalidRecipe);
                    }
                    values.extend(row.iter().take(dim).copied());
                }
            }
            "lit" => values.resize(count * dim, 0.0),
            "pose" => {
                let key = parse_pose_key(pred.get("key").ok_or(FigureError::InvalidRecipe)?)?;
                let points = self.skinned_points(&key)?;
                if points.len() < count {
                    return Err(FigureError::InvalidStream);
                }
                for point in points.iter().take(count) {
                    values.extend(point.iter().take(dim).copied());
                }
            }
            _ => return Err(FigureError::InvalidRecipe),
        }
        let mut tokens = values
            .iter()
            .map(|&value| {
                decode_number(value, number_format, &mut reader)
                    .map_err(|_| FigureError::InvalidStreamAt("number residual"))
            })
            .collect::<Result<Vec<_>, _>>()?;
        let mut index = -1i64;
        for _ in 0..reader.unsigned()? {
            index += 1 + reader.unsigned()? as i64;
            let slot = usize::try_from(index).map_err(|_| FigureError::InvalidStream)?;
            let code = reader.unsigned()?;
            let extra = if code == 0 { 0 } else { reader.int()? };
            let base = *tokens.get(slot).ok_or(FigureError::InvalidStream)?;
            if code == 0 {
                tokens[slot].negative = !base.negative;
            } else {
                let exception = code - 2;
                let negative_zero = (exception & 1) != 0;
                let decimals = ((exception >> 1) as i32) - 1;
                let predicted = values[slot];
                let formatted = fixed_token(predicted, decimals.max(0) as usize)?;
                let value = formatted
                    .signed()
                    .checked_add(extra)
                    .ok_or(FigureError::InvalidStream)?;
                tokens[slot] = Token {
                    negative: value < 0 || (value == 0 && negative_zero),
                    magnitude: value.unsigned_abs(),
                    decimals,
                };
            }
        }
        // The native reader's streams are shared by all blocks of a given
        // type, so commit the advanced decoder cursor back to the figure.
        self.streams[stream] = reader;
        // Keep file name in the signature to make reference data ownership
        // explicit; anchor indices are stored on the output file record.
        let _ = name;
        let mut rows = Vec::with_capacity(count);
        for _ in 0..count {
            rows.push(tokens.drain(..dim).collect());
        }
        Ok(rows)
    }

    fn build_file(&mut self, file_value: &Value) -> Result<(String, Vec<u8>), FigureError> {
        let name = value_str(file_value.get("n").ok_or(FigureError::InvalidRecipe)?)?.to_owned();
        let mut record = FileRec::default();
        let mut output = Vec::<u8>::new();
        let program = value_array(file_value.get("prog").ok_or(FigureError::InvalidRecipe)?)?;
        for op in program {
            let op = value_array(op)?;
            let tag = value_str(op.first().ok_or(FigureError::InvalidRecipe)?)?;
            match tag {
                "s" => {
                    let idx = value_usize(op.get(1).ok_or(FigureError::InvalidRecipe)?)?;
                    let text =
                        value_array(self.cfg.get("strings").ok_or(FigureError::InvalidRecipe)?)?
                            .get(idx)
                            .ok_or(FigureError::InvalidRecipe)?;
                    output.extend_from_slice(self.texture_string(value_str(text)?)?.as_bytes());
                    output.push(b'\n');
                }
                "v" | "vt" => {
                    let count = value_usize(op.get(1).ok_or(FigureError::InvalidRecipe)?)?;
                    let pred = op.get(2).ok_or(FigureError::InvalidRecipe)?;
                    if tag == "v"
                        && pred.get("rest").and_then(Value::as_u64).unwrap_or(0) != 0
                        && self.skin.is_none()
                    {
                        self.setup_skin(
                            self.cfg.get("skin").ok_or(FigureError::InvalidRecipe)?,
                            count,
                        )?;
                    }
                    let rows =
                        self.numeric_rows(count, pred, &mut record, &name)
                            .map_err(|error| {
                                FigureError::OperationFailed(tag.to_owned(), Box::new(error))
                            })?;
                    for row in &rows {
                        output.extend_from_slice(tag.as_bytes());
                        for token in row {
                            output.push(b' ');
                            token.append_render(&mut output);
                        }
                        output.push(b'\n');
                    }
                    if tag == "v" && pred.get("rest").and_then(Value::as_u64).unwrap_or(0) != 0 {
                        self.rest_points(
                            &rows,
                            value_str(pred.get("fmt").ok_or(FigureError::InvalidRecipe)?)?,
                        )
                        .map_err(|error| {
                            FigureError::OperationFailed("rest-points".to_owned(), Box::new(error))
                        })?;
                    }
                    if tag == "v" {
                        record.vertices.extend(rows);
                    } else {
                        record.texcoords.extend(rows);
                    }
                }
                "f" => {
                    let count = value_usize(op.get(1).ok_or(FigureError::InvalidRecipe)?)?;
                    let spec = op.get(2).ok_or(FigureError::InvalidRecipe)?;
                    self.faces(count, spec, &record, &mut output)
                        .map_err(|error| {
                            FigureError::OperationFailed("faces".to_owned(), Box::new(error))
                        })?;
                }
                _ => return Err(FigureError::InvalidRecipe),
            }
        }
        self.files.insert(name.clone(), record);
        Ok((name, output))
    }

    fn faces(
        &mut self,
        count: usize,
        spec: &Value,
        record: &FileRec,
        output: &mut Vec<u8>,
    ) -> Result<(), FigureError> {
        let triangles = if let Some(t) = spec.get("t") {
            let source_index = value_usize(t)?;
            let source = self
                .sources
                .get(source_index)
                .ok_or(FigureError::InvalidRecipe)?;
            let raw = spec.get("tk").and_then(Value::as_str) == Some("raw");
            Some(if raw {
                &source.raw_triangles
            } else {
                &source.triangles
            })
        } else {
            None
        };
        let mut anchor_map = HashMap::<usize, usize>::new();
        if triangles.is_some() {
            for (index, &anchor) in record.anchors.iter().enumerate() {
                anchor_map.entry(anchor).or_insert(index + 1);
            }
        }
        let mut vertex = 0i64;
        let mut texcoord = 0i64;
        let mut triangle_index = -1i64;
        for _ in 0..count {
            let code = if triangles.is_some() {
                self.streams[3].unsigned()? as usize
            } else {
                0
            };
            let mut ordered = [0usize; 3];
            if code != 0 {
                triangle_index += 1 + self.streams[3].int()?;
                let tri = triangles
                    .and_then(|ts| ts.get(usize::try_from(triangle_index).ok()?))
                    .ok_or(FigureError::InvalidStream)?;
                let orders = [
                    [0, 1, 2],
                    [1, 2, 0],
                    [2, 0, 1],
                    [0, 2, 1],
                    [2, 1, 0],
                    [1, 0, 2],
                ];
                let order = orders.get(code - 1).ok_or(FigureError::InvalidStream)?;
                for k in 0..3 {
                    ordered[k] = *anchor_map
                        .get(&tri[order[k]])
                        .ok_or(FigureError::InvalidStream)?;
                }
            }
            output.extend_from_slice(b"f ");
            for ci in 0..3 {
                let position = if code != 0 {
                    vertex = ordered[ci] as i64;
                    vertex
                } else {
                    vertex += self.streams[3].int()?;
                    vertex
                };
                let uv = texcoord + 1 + self.streams[3].int()?;
                texcoord = texcoord.max(uv);
                append_i64(position, output);
                output.push(b'/');
                append_i64(uv, output);
                if ci != 2 {
                    output.push(b' ');
                }
            }
            output.push(b'\n');
        }
        Ok(())
    }

    fn build(mut self) -> Result<Vec<(String, Vec<u8>)>, FigureError> {
        let files = value_array(self.cfg.get("files").ok_or(FigureError::InvalidRecipe)?)?;
        let mut out = Vec::new();
        for file in files {
            let file_name = value_str(file.get("n").ok_or(FigureError::InvalidRecipe)?)?.to_owned();
            let (name, bytes) = self
                .build_file(file)
                .map_err(|error| FigureError::FileFailed(file_name, Box::new(error)))?;
            out.push((
                format!(
                    "assets/figures/{}/{}",
                    value_str(self.cfg.get("name").ok_or(FigureError::InvalidRecipe)?)?,
                    name
                ),
                bytes,
            ));
        }
        if let Some(keys) = self.cfg.get("keys").and_then(Value::as_array) {
            let count = self
                .skin
                .as_ref()
                .ok_or(FigureError::InvalidRecipe)?
                .rest
                .len();
            let fmt = value_str(
                self.cfg
                    .get("skin")
                    .and_then(|s| s.get("kfmt"))
                    .ok_or(FigureError::InvalidRecipe)?,
            )?
            .to_owned();
            for key in keys {
                let values = value_array(key)?;
                if values.len() < 4 {
                    return Err(FigureError::InvalidRecipe);
                }
                let filename = value_str(&values[0])?.to_owned();
                let mut pose_values = values[1..4].to_vec();
                if let Some(overrides) = values.get(4) {
                    pose_values.push(overrides.clone());
                }
                let pose_key = Value::Array(pose_values);
                let pred = serde_json::json!({ "p": "pose", "key": pose_key, "fmt": fmt });
                let mut record = self.files.get(&filename).cloned().unwrap_or_default();
                let rows = self.numeric_rows(count, &pred, &mut record, &filename)?;
                let mut bytes = Vec::new();
                for row in rows {
                    bytes.extend_from_slice(b"v");
                    for token in row {
                        bytes.push(b' ');
                        token.append_render(&mut bytes);
                    }
                    bytes.push(b'\n');
                }
                let figure = value_str(self.cfg.get("name").ok_or(FigureError::InvalidRecipe)?)?;
                out.push((format!("assets/figures/{figure}/{filename}"), bytes));
            }
        }
        Ok(out)
    }
}

fn decode_number(
    value: f64,
    fmt: NumberFormat,
    reader: &mut IntReader<'_>,
) -> Result<Token, FigureError> {
    let parsed = match fmt {
        NumberFormat::Quantized { digits, grid } => {
            let quantized = (value * grid + 0.5).floor() as i64 + reader.int()?;
            return fixed_token(quantized as f64 / grid, digits);
        }
        NumberFormat::Fixed(digits) => fixed_token(value, digits)?,
        NumberFormat::General(significant) => general_token(value, significant)?,
    };
    let residual = reader.int()?;
    if residual == 0 {
        return Ok(parsed);
    }
    let value = parsed
        .signed()
        .checked_add(residual)
        .ok_or(FigureError::InvalidStream)?;
    Ok(Token {
        negative: value < 0,
        magnitude: value.unsigned_abs(),
        decimals: parsed.decimals,
    })
}

/// All archive paths referenced by the embedded generic figure recipes.
pub fn source_archives() -> Result<Vec<String>, FigureError> {
    let bytes = asset_packs::get(PACK, "index.json").ok_or(FigureError::MissingRecipe)?;
    let configs: Vec<Value> =
        serde_json::from_slice(bytes).map_err(|_| FigureError::InvalidRecipe)?;
    let mut paths = std::collections::BTreeSet::new();
    for cfg in configs {
        for src in value_array(cfg.get("src").ok_or(FigureError::InvalidRecipe)?)? {
            let (path, _) = value_str(src)?
                .split_once(':')
                .ok_or(FigureError::InvalidRecipe)?;
            paths.insert(path.to_owned());
        }
        if let Some(path) = cfg
            .get("skin")
            .and_then(|s| s.get("arc"))
            .and_then(Value::as_str)
        {
            paths.insert(path.to_owned());
        }
    }
    Ok(paths.into_iter().collect())
}

/// Construct generic figure files from extracted source archives.
pub fn generate(
    archives: &BTreeMap<String, Vec<u8>>,
) -> Result<Vec<(String, Vec<u8>)>, FigureError> {
    let pack = asset_packs::EMBEDDED_PACKS
        .iter()
        .find(|p| p.name == PACK)
        .ok_or(FigureError::MissingRecipe)?;
    let index = asset_packs::get(PACK, "index.json").ok_or(FigureError::MissingRecipe)?;
    let configs: Vec<Value> =
        serde_json::from_slice(index).map_err(|_| FigureError::InvalidRecipe)?;
    let mut remaining_uses = HashMap::<SourceKey, usize>::new();
    for cfg in &configs {
        for key in source_keys(cfg)? {
            *remaining_uses.entry(key).or_default() += 1;
        }
    }
    let mut source_cache = HashMap::<SourceKey, Rc<Source>>::new();
    let get_stream = |stream: char, name: &str| -> &'static [u8] {
        let key = format!("{stream}/{name}");
        pack.entries
            .iter()
            .find(|(n, _, _)| *n == key)
            .and_then(|(_, offset, len)| pack.data.get(*offset..offset + len))
            .unwrap_or(&[])
    };
    let mut output = Vec::new();
    for cfg in &configs {
        let name = value_str(cfg.get("name").ok_or(FigureError::InvalidRecipe)?)?;
        let streams = ['m', 'k', 'u', 'f'].map(|tag| get_stream(tag, name));
        let generated = Figure::new(cfg, archives, streams, &mut source_cache, &remaining_uses)
            .and_then(Figure::build)
            .map_err(|error| FigureError::RecipeFailed(name.to_owned(), Box::new(error)))?;
        for key in source_keys(cfg)? {
            if let Some(remaining) = remaining_uses.get_mut(&key) {
                *remaining -= 1;
                if *remaining == 0 {
                    source_cache.remove(&key);
                }
            }
        }
        let mut files = generated;
        output.append(&mut files);
    }
    Ok(output)
}

type SourceKey = (String, String, bool);

fn source_keys(cfg: &Value) -> Result<Vec<SourceKey>, FigureError> {
    let sources = value_array(cfg.get("src").ok_or(FigureError::InvalidRecipe)?)?;
    let unhide = cfg.get("unhide").and_then(Value::as_array);
    sources
        .iter()
        .enumerate()
        .map(|(index, source)| {
            let (archive, model) = value_str(source)?
                .split_once(':')
                .ok_or(FigureError::InvalidRecipe)?;
            let unhide = unhide.is_some_and(|values| {
                values
                    .iter()
                    .any(|value| value.as_u64() == Some(index as u64))
            });
            Ok((archive.to_owned(), model.to_owned(), unhide))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{
        append_i64, fixed_token, general_token, general_token_formatted, parse_token, Token,
    };

    #[test]
    fn appends_tokens_without_changing_decimal_spelling() {
        let cases = [
            (
                Token {
                    negative: false,
                    magnitude: 123,
                    decimals: -1,
                },
                "123",
            ),
            (
                Token {
                    negative: false,
                    magnitude: 123,
                    decimals: 0,
                },
                "123.",
            ),
            (
                Token {
                    negative: false,
                    magnitude: 123,
                    decimals: 2,
                },
                "1.23",
            ),
            (
                Token {
                    negative: false,
                    magnitude: 23,
                    decimals: 2,
                },
                "0.23",
            ),
            (
                Token {
                    negative: false,
                    magnitude: 3,
                    decimals: 3,
                },
                "0.003",
            ),
            (
                Token {
                    negative: true,
                    magnitude: 0,
                    decimals: 1,
                },
                "-0.0",
            ),
        ];
        for (token, expected) in cases {
            let mut bytes = Vec::new();
            token.append_render(&mut bytes);
            assert_eq!(std::str::from_utf8(&bytes).unwrap(), expected);
        }
    }

    #[test]
    fn appends_signed_indices_including_i64_min() {
        for value in [0, 12, -34, i64::MIN] {
            let mut bytes = Vec::new();
            append_i64(value, &mut bytes);
            assert_eq!(std::str::from_utf8(&bytes).unwrap(), value.to_string());
        }
    }

    #[test]
    fn fixed_decimal_fast_path_matches_rust_formatting() {
        let mut state = 0x9e37_79b9u64;
        for digits in 0..=15 {
            for _ in 0..4000 {
                state ^= state << 13;
                state ^= state >> 7;
                state ^= state << 17;
                let value = ((state as i64) as f64) / 1_000_000_000_000_000.0;
                let expected = parse_token(&format!("{value:.digits$}")).unwrap();
                let actual = fixed_token(value, digits).unwrap();
                assert_eq!(actual.negative, expected.negative);
                assert_eq!(actual.magnitude, expected.magnitude);
                assert_eq!(actual.decimals, expected.decimals);
            }
        }
        for value in [0.5, -0.5, 1.005, 1.25, 2.675, -0.0, f64::MIN_POSITIVE] {
            for digits in 0..=15 {
                let expected = parse_token(&format!("{value:.digits$}")).unwrap();
                let actual = fixed_token(value, digits).unwrap();
                assert_eq!(actual.negative, expected.negative);
                assert_eq!(actual.magnitude, expected.magnitude);
                assert_eq!(actual.decimals, expected.decimals);
            }
        }
    }

    #[test]
    fn general_decimal_fast_path_matches_formatter() {
        let mut state = 0x243f_6a88_85a3_08d3u64;
        for significant in 1..=15 {
            for _ in 0..4000 {
                state ^= state << 13;
                state ^= state >> 7;
                state ^= state << 17;
                let value = ((state as i64) as f64) / 1_000_000_000_000_000.0;
                let expected = general_token_formatted(value, significant).unwrap();
                let actual = general_token(value, significant).unwrap();
                assert_eq!(actual.negative, expected.negative);
                assert_eq!(actual.magnitude, expected.magnitude);
                assert_eq!(actual.decimals, expected.decimals);
            }
        }
        for value in [
            0.0,
            -0.0,
            0.00009999999999999,
            9.9995,
            9_999.5,
            1.25,
            -123.456,
        ] {
            for significant in 1..=15 {
                let expected = general_token_formatted(value, significant).unwrap();
                let actual = general_token(value, significant).unwrap();
                assert_eq!(actual.negative, expected.negative);
                assert_eq!(actual.magnitude, expected.magnitude);
                assert_eq!(actual.decimals, expected.decimals);
            }
        }
    }
}

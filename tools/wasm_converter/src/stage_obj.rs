//! Interpreter for compact stage OBJ recipes.

use std::collections::{BTreeMap, HashMap};

use crate::{
    stage_galaxy::{self, StageObject},
    stage_geometry::GeometryPool,
    stage_recipe::{self, RecipeError, StageRecipe},
};

const K_TEXT: i64 = 0;
const K_V: i64 = 1;
const K_VT: i64 = 2;
const K_F: i64 = 3;
const MODE_BOX: i64 = 1;
const MODE_ORIG: i64 = 2;
const MODE_AFFINE: i64 = 3;
const GAME: f64 = 1000.0 / 256.0;
const TILE_SCALE: f64 = 1_000_000_000.0;
const AFFINE_SCALE: f64 = 10_000_000.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StageObjError {
    Recipe(RecipeError),
    InvalidStream,
    InvalidPoolReference,
    InvalidObjectReference,
    UnsupportedTextureRepeat,
}

impl From<RecipeError> for StageObjError {
    fn from(error: RecipeError) -> Self {
        Self::Recipe(error)
    }
}

type Point = [f64; 3];
type Uv = [f64; 2];
type Frame = ([[f64; 3]; 3], [f64; 3]);

#[derive(Clone)]
struct Group {
    mode: i64,
    frame: i64,
    tile: i64,
    convention: i64,
    texture_repeat: i64,
    affine: [i64; 8],
}

pub fn name_candidates(pool: &GeometryPool) -> Vec<String> {
    let mut candidates = Vec::with_capacity(pool.candidate_names.len() * 5);
    for name in &pool.candidate_names {
        candidates.push(format!("newmtl {name}"));
        candidates.push(format!("\tmap_Kd {name}.png"));
        candidates.push(format!("usemtl {name}"));
        candidates.push(format!("o {name}"));
        candidates.push(format!("o {}", name.to_lowercase()));
    }
    candidates
}

pub fn decode_file(
    recipe: &StageRecipe,
    file_name: &str,
    pool: &GeometryPool,
    objects: &[StageObject],
) -> Result<Vec<String>, StageObjError> {
    decode_file_with_repeats(recipe, file_name, pool, objects, |_| {
        Err(StageObjError::UnsupportedTextureRepeat)
    })
}

pub fn decode_file_with_repeats(
    recipe: &StageRecipe,
    file_name: &str,
    pool: &GeometryPool,
    objects: &[StageObject],
    repeats: impl FnMut(usize) -> Result<BTreeMap<String, f64>, StageObjError>,
) -> Result<Vec<String>, StageObjError> {
    let candidates = name_candidates(pool);
    let (line_count, mut streams) = recipe.file_streams(file_name)?;
    decode_streams(
        &mut streams,
        line_count,
        pool,
        objects,
        &candidates,
        repeats,
    )
}

fn decode_streams(
    streams: &mut stage_recipe::Streams<'_>,
    line_count: usize,
    pool: &GeometryPool,
    objects: &[StageObject],
    candidates: &[String],
    mut repeats: impl FnMut(usize) -> Result<BTreeMap<String, f64>, StageObjError>,
) -> Result<Vec<String>, StageObjError> {
    let mut kinds = Vec::with_capacity(line_count);
    while kinds.len() < line_count {
        let kind = streams.int("o.kind")?;
        let run =
            usize::try_from(streams.int("o.run")?).map_err(|_| StageObjError::InvalidStream)?;
        if run == 0 || kinds.len().saturating_add(run) > line_count {
            return Err(StageObjError::InvalidStream);
        }
        kinds.resize(kinds.len() + run, kind);
    }
    if kinds
        .iter()
        .any(|kind| ![K_TEXT, K_V, K_VT, K_F].contains(kind))
    {
        return Err(StageObjError::InvalidStream);
    }
    let texts = stage_recipe::decode_lines_from_streams(
        streams,
        kinds.iter().filter(|&&kind| kind == K_TEXT).count(),
        candidates,
    )?;

    let mut vertex_rows = Vec::new();
    let mut vertices = Vec::<Point>::new();
    let mut anchors = Vec::new();
    let mut anchor = 0i64;
    for _ in 0..kinds.iter().filter(|&&kind| kind == K_V).count() {
        anchor = anchor
            .checked_add(streams.int("v.anc")?)
            .ok_or(StageObjError::InvalidPoolReference)?;
        let anchor_index =
            usize::try_from(anchor).map_err(|_| StageObjError::InvalidPoolReference)?;
        let point = *pool
            .positions
            .get(anchor_index)
            .ok_or(StageObjError::InvalidPoolReference)?;
        anchors.push(anchor_index);
        let residual = [
            streams.int("v.res")?,
            streams.int("v.res")?,
            streams.int("v.res")?,
        ];
        let decimal_code = streams.int("v.dec")?;
        let decimals = [
            ((decimal_code >> 8) & 15) as u32,
            ((decimal_code >> 4) & 15) as u32,
            (decimal_code & 15) as u32,
        ];
        let negative_zero = streams.int("v.nz")?;
        let mut row = Vec::with_capacity(3);
        let mut quantized = [0.0; 3];
        for axis in 0..3 {
            let value = scaled(point[axis], decimals[axis])?
                .checked_add(residual[axis])
                .ok_or(StageObjError::InvalidStream)?;
            let negative = ((negative_zero >> axis) & 1 != 0) ^ (value == 0 && point[axis] < 0.0);
            row.push(format_number(value, decimals[axis], negative));
            quantized[axis] = value as f64 / 10f64.powi(decimals[axis] as i32);
        }
        vertex_rows.push(format!("v {}", row.join(" ")));
        let worst_residual = residual.iter().map(|value| value.abs()).max().unwrap_or(0);
        vertices.push(
            if worst_residual <= 1 && decimals.iter().min().copied().unwrap_or(0) >= 5 {
                point
            } else {
                quantized
            },
        );
    }

    let mut faces = Vec::<[(usize, usize); 3]>::new();
    let (mut vertex_index, mut uv_index) = (0i64, 0i64);
    for _ in 0..kinds.iter().filter(|&&kind| kind == K_F).count() {
        let mut face = [(0usize, 0usize); 3];
        for corner in &mut face {
            vertex_index = vertex_index
                .checked_add(streams.int("f.v")?)
                .ok_or(StageObjError::InvalidStream)?;
            uv_index = uv_index
                .checked_add(streams.int("f.t")?)
                .ok_or(StageObjError::InvalidStream)?;
            *corner = (
                usize::try_from(vertex_index).map_err(|_| StageObjError::InvalidStream)?,
                usize::try_from(uv_index).map_err(|_| StageObjError::InvalidStream)?,
            );
        }
        faces.push(face);
    }

    let group_count =
        usize::try_from(streams.int("g.n")?).map_err(|_| StageObjError::InvalidStream)?;
    let mut groups = Vec::with_capacity(group_count);
    for _ in 0..group_count {
        let mode = streams.int("g.mode")?;
        let mut group = Group {
            mode,
            frame: -1,
            tile: 0,
            convention: 0,
            texture_repeat: 0,
            affine: [0; 8],
        };
        if mode == MODE_BOX {
            group.frame = streams.int("g.frame")?;
            group.tile = streams.int("g.tile")?;
            group.convention = streams.int("g.conv")?;
            group.texture_repeat = streams.int("g.tsrc")?;
        }
        if mode == MODE_ORIG || mode == MODE_AFFINE {
            for parameter in &mut group.affine {
                *parameter = streams.int("g.aff")?;
            }
        }
        groups.push(group);
    }
    let face_groups = face_groups(&kinds, &texts, faces.len());
    let predicted = predict_uvs(
        streams,
        &faces,
        &face_groups,
        &groups,
        &vertices,
        &anchors,
        pool,
        objects,
        &mut repeats,
    )?;

    let mut uv_rows = Vec::new();
    let mut last = [0.0; 2];
    for index in 1..=kinds.iter().filter(|&&kind| kind == K_VT).count() {
        let uv = predicted.get(&index).copied().unwrap_or(last);
        last = uv;
        let residual = [streams.int("vt.res")?, streams.int("vt.res")?];
        let decimal_code = streams.int("vt.dec")?;
        let decimals = [
            ((decimal_code >> 4) & 15) as u32,
            (decimal_code & 15) as u32,
        ];
        let negative_zero = streams.int("vt.nz")?;
        let mut row = Vec::with_capacity(2);
        for axis in 0..2 {
            let value = scaled(uv[axis], decimals[axis])?
                .checked_add(residual[axis])
                .ok_or(StageObjError::InvalidStream)?;
            let negative = ((negative_zero >> axis) & 1 != 0) ^ (value == 0 && uv[axis] < 0.0);
            row.push(format_number(value, decimals[axis], negative));
        }
        uv_rows.push(format!("vt {}", row.join(" ")));
    }

    let mut text_index = 0;
    let mut vertex_row = 0;
    let mut uv_row = 0;
    let mut face_row = 0;
    let mut output = Vec::with_capacity(kinds.len());
    for kind in kinds {
        match kind {
            K_TEXT => {
                output.push(texts[text_index].clone());
                text_index += 1;
            }
            K_V => {
                output.push(vertex_rows[vertex_row].clone());
                vertex_row += 1;
            }
            K_VT => {
                output.push(uv_rows[uv_row].clone());
                uv_row += 1;
            }
            K_F => {
                let face = faces[face_row];
                output.push(format!(
                    "f {}/{} {}/{} {}/{}",
                    face[0].0, face[0].1, face[1].0, face[1].1, face[2].0, face[2].1
                ));
                face_row += 1;
            }
            _ => unreachable!(),
        }
    }
    Ok(output)
}

fn format_number(value: i64, decimals: u32, negative_zero: bool) -> String {
    let negative = value < 0 || (value == 0 && negative_zero);
    let mut digits = value.unsigned_abs().to_string();
    if decimals != 0 {
        let decimals = decimals as usize;
        if digits.len() <= decimals {
            digits.insert_str(0, &"0".repeat(decimals + 1 - digits.len()));
        }
        let split = digits.len() - decimals;
        digits.insert(split, '.');
    }
    if negative {
        digits.insert(0, '-');
    }
    digits
}

fn scaled(value: f64, decimals: u32) -> Result<i64, StageObjError> {
    let result = (value * 10f64.powi(decimals as i32)).round_ties_even();
    if !result.is_finite() || result < i64::MIN as f64 || result >= i64::MAX as f64 {
        return Err(StageObjError::InvalidStream);
    }
    Ok(result as i64)
}

fn face_groups(kinds: &[i64], texts: &[String], face_count: usize) -> Vec<usize> {
    let mut object = -1isize;
    let mut material = String::new();
    let mut groups = HashMap::<(isize, String), usize>::new();
    let mut result = Vec::with_capacity(face_count);
    let mut text_index = 0;
    for (line_index, kind) in kinds.iter().enumerate() {
        if *kind == K_TEXT {
            let text = &texts[text_index];
            text_index += 1;
            if text.starts_with("o ") {
                object = line_index as isize;
            } else if text.starts_with("usemtl ") {
                material.clone_from(text);
            }
        } else if *kind == K_F {
            let key = (object, material.clone());
            let next = groups.len();
            let group = *groups.entry(key).or_insert(next);
            result.push(group);
        }
    }
    result
}

fn frame_of(object: Option<&StageObject>) -> Frame {
    let Some(object) = object else {
        return (
            [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
            [0.0; 3],
        );
    };
    let matrix = stage_galaxy::trs([0.0; 3], object.world_rot);
    let g = [
        [-matrix[0][0], -matrix[0][1], -matrix[0][2]],
        [matrix[2][0], matrix[2][1], matrix[2][2]],
        [matrix[1][0], matrix[1][1], matrix[1][2]],
    ];
    let axes = [
        [g[0][0], g[1][0], g[2][0]],
        [-g[0][2], -g[1][2], -g[2][2]],
        [g[0][1], g[1][1], g[2][1]],
    ];
    let p = object.world_pos;
    (axes, [-p[0] / 256.0, p[2] / 256.0, p[1] / 256.0])
}

fn box_project(frame: Frame, points: [Point; 3]) -> ([Point; 3], usize, f64) {
    let (axes, location) = frame;
    let mut local = [[0.0; 3]; 3];
    for (index, [x, y, z]) in points.into_iter().enumerate() {
        let game = [
            -x * GAME - location[0],
            z * GAME - location[1],
            y * GAME - location[2],
        ];
        for axis in 0..3 {
            local[index][axis] =
                axes[axis][0] * game[0] + axes[axis][1] * game[1] + axes[axis][2] * game[2];
        }
    }
    let a = local[0];
    let b = local[1];
    let c = local[2];
    let e1 = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
    let e2 = [c[0] - a[0], c[1] - a[1], c[2] - a[2]];
    let normal = [
        -(e1[1] * e2[2] - e1[2] * e2[1]),
        -(e1[2] * e2[0] - e1[0] * e2[2]),
        -(e1[0] * e2[1] - e1[1] * e2[0]),
    ];
    let mut axis = 0;
    for candidate in 1..3 {
        if normal[candidate].abs() > normal[axis].abs() {
            axis = candidate;
        }
    }
    let sign = if normal[axis] > 0.0 { 1.0 } else { -1.0 };
    (local, axis, sign)
}

fn box_uv(point: Point, axis: usize, sign: f64, tile: f64) -> Uv {
    match axis {
        0 => [point[1] * sign / tile, point[2] / tile],
        1 => [-point[0] * sign / tile, point[2] / tile],
        _ => [point[0] / tile, point[1] * sign / tile],
    }
}

fn affine_uv(parameters: &[i64; 8], point: Point) -> Uv {
    let a = parameters.map(|value| value as f64 / AFFINE_SCALE);
    [
        a[0] * point[0] + a[1] * point[1] + a[2] * point[2] + a[3],
        a[4] * point[0] + a[5] * point[1] + a[6] * point[2] + a[7],
    ]
}

fn predict_uvs(
    streams: &mut stage_recipe::Streams<'_>,
    faces: &[[(usize, usize); 3]],
    face_groups: &[usize],
    groups: &[Group],
    vertices: &[Point],
    anchors: &[usize],
    pool: &GeometryPool,
    objects: &[StageObject],
    repeats: &mut impl FnMut(usize) -> Result<BTreeMap<String, f64>, StageObjError>,
) -> Result<BTreeMap<usize, Uv>, StageObjError> {
    let mut predicted = BTreeMap::new();
    let mut frames = HashMap::<i64, Frame>::new();
    for (face_index, face) in faces.iter().enumerate() {
        let group = groups
            .get(face_groups[face_index])
            .ok_or(StageObjError::InvalidStream)?;
        let points = [
            *vertices
                .get(face[0].0 - 1)
                .ok_or(StageObjError::InvalidPoolReference)?,
            *vertices
                .get(face[1].0 - 1)
                .ok_or(StageObjError::InvalidPoolReference)?,
            *vertices
                .get(face[2].0 - 1)
                .ok_or(StageObjError::InvalidPoolReference)?,
        ];
        let uvs = match group.mode {
            MODE_BOX => {
                let frame = *frames.entry(group.frame).or_insert_with(|| {
                    if group.frame < 0 {
                        frame_of(None)
                    } else {
                        frame_of(objects.get(group.frame as usize))
                    }
                });
                let (local, mut axis, mut sign) = box_project(frame, points);
                if group.convention == 1 && axis == 2 {
                    sign = 1.0;
                } else if group.convention == 2 {
                    sign = [1.0, -1.0, 1.0][axis];
                }
                let axis_override = streams.int("vt.ax")?;
                if axis_override != 0 {
                    let code = axis_override - 1;
                    axis = (code >> 1) as usize;
                    sign = if code & 1 != 0 { 1.0 } else { -1.0 };
                }
                let mut tile = group.tile as f64 / TILE_SCALE;
                if group.texture_repeat != 0 {
                    let object_index = usize::try_from(group.frame)
                        .map_err(|_| StageObjError::InvalidObjectReference)?;
                    let repeat = repeats(object_index)?;
                    let (texture, value) = repeat
                        .iter()
                        .nth(group.texture_repeat as usize - 1)
                        .ok_or(StageObjError::InvalidObjectReference)?;
                    let _ = texture;
                    tile = ((value * TILE_SCALE).round_ties_even() as i64 + group.tile) as f64
                        / TILE_SCALE;
                }
                local
                    .map(|point| Some(box_uv(point, axis, sign, tile)))
                    .into_iter()
                    .collect()
            }
            MODE_ORIG => {
                let mut result = Vec::with_capacity(3);
                for (corner, point) in face.iter().zip(points) {
                    if predicted.contains_key(&corner.1) {
                        result.push(None);
                    } else {
                        let anchor = *anchors
                            .get(corner.0 - 1)
                            .ok_or(StageObjError::InvalidPoolReference)?;
                        let mut options = pool
                            .uvs
                            .get(anchor)
                            .ok_or(StageObjError::InvalidPoolReference)?
                            .clone();
                        options.push(affine_uv(&group.affine, point));
                        let choice = usize::try_from(streams.int("vt.oc")?)
                            .map_err(|_| StageObjError::InvalidStream)?;
                        result.push(Some(
                            *options.get(choice).ok_or(StageObjError::InvalidStream)?,
                        ));
                    }
                }
                result
            }
            MODE_AFFINE => points
                .map(|point| Some(affine_uv(&group.affine, point)))
                .into_iter()
                .collect(),
            _ => vec![Some([0.0, 0.0]); 3],
        };
        for (corner, uv) in face.iter().zip(uvs) {
            if !predicted.contains_key(&corner.1) {
                if let Some(uv) = uv {
                    predicted.insert(corner.1, uv);
                }
            }
        }
    }
    Ok(predicted)
}

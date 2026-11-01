//! High-resolution vertex streams shared by source-backed texture bakes.
//!
//! This ports the J3D half of `textures_bake.build_high`: shape vertices are
//! expanded in native order, transformed by their draw matrices, and assigned
//! the material's selected UV set. Low-poly OBJ streams and material textures
//! are assembled by the caller.

use crate::textures_gx::TextureLevel;
use crate::{
    j3d_model::{minv, mmul, trs_euler, J3dModel},
    tev::TevModel,
};
use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq)]
pub struct HighMesh {
    pub positions: Vec<[f64; 3]>,
    pub uvs: Vec<[f64; 2]>,
    pub colors: Vec<[f64; 4]>,
    pub normals: Vec<[f64; 3]>,
    /// Original J3D draw-matrix index for each expanded vertex.
    pub draw_matrices: Vec<usize>,
    pub triangles: Vec<[u32; 3]>,
    /// One compact material index for each triangle.
    pub triangle_materials: Vec<u32>,
    /// Compact material index to source J3D material index.
    pub materials: Vec<usize>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct LowMesh {
    pub positions: Vec<[f64; 3]>,
    pub uvs: Vec<[f64; 2]>,
    pub faces: Vec<Vec<(usize, usize)>>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct BakedMaterial {
    pub texture: i32,
    pub wrap_s: i32,
    pub wrap_t: i32,
    /// 0 = opaque, 1 = alpha mask, 2 = blended.
    pub alpha_mode: i32,
    pub cutoff: f32,
    pub factor: [f32; 4],
    pub transform: Option<[f32; 4]>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BakeOptions {
    pub width: usize,
    pub height: usize,
    pub margin: i32,
    pub flags: i32,
    pub extent_factor: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GeometryError {
    InvalidMaterial,
    InvalidVertex,
    TooLarge,
    InvalidObj,
}

/// Return the native part-bake's low-mesh vertices in rest space.
///
/// Animated part recipes construct their high mesh in the requested pose, then
/// map the OBJ vertices back through the nearest posed draw matrix before
/// rasterizing against the rest high mesh. This mirrors
/// `textures_bake_parts.unpose()` including its 4-unit spatial grid and
/// one-cell neighbor search.
pub fn unpose_low_vertices(
    model: &J3dModel,
    high_pose: &HighMesh,
    pose: &[[f64; 12]],
    vertices: &mut [[f64; 3]],
) -> Result<(), GeometryError> {
    let mut grid = HashMap::<(i32, i32, i32), Vec<usize>>::new();
    for (index, position) in high_pose.positions.iter().enumerate() {
        let key = (
            (position[0] / 4.0).floor() as i32,
            (position[1] / 4.0).floor() as i32,
            (position[2] / 4.0).floor() as i32,
        );
        grid.entry(key).or_default().push(index);
    }

    let mut matrices = HashMap::<usize, [f64; 12]>::new();
    for point in vertices {
        let origin = (
            (point[0] / 4.0).floor() as i32,
            (point[1] / 4.0).floor() as i32,
            (point[2] / 4.0).floor() as i32,
        );
        let mut best: Option<(f64, usize)> = None;
        let mut radius = 0i32;
        loop {
            // The reference Python routine reuses `dx/dy/dz` for both the
            // neighborhood offsets and candidate deltas. Preserve that
            // shadowing behavior because it affects which cells later loop
            // iterations inspect for models with several candidate clusters.
            for outer_dx in -radius..=radius {
                let mut dx = outer_dx as f64;
                for outer_dy in -radius..=radius {
                    let mut dy = outer_dy as f64;
                    for outer_dz in -radius..=radius {
                        let mut dz = outer_dz as f64;
                        if dx.abs().max(dy.abs()).max(dz.abs()) != radius as f64 {
                            continue;
                        }
                        let offsets = [dx, dy, dz];
                        let key = offsets
                            .iter()
                            .map(|offset| {
                                if offset.is_finite() && offset.fract() == 0.0 {
                                    Some(offset.round() as i32)
                                } else {
                                    None
                                }
                            })
                            .collect::<Option<Vec<_>>>();
                        if let Some(offsets) = key {
                            let key = (
                                origin.0 + offsets[0],
                                origin.1 + offsets[1],
                                origin.2 + offsets[2],
                            );
                            for &index in grid.get(&key).into_iter().flatten() {
                                let candidate = high_pose.positions[index];
                                dx = candidate[0] - point[0];
                                dy = candidate[1] - point[1];
                                dz = candidate[2] - point[2];
                                let distance = dx * dx + dy * dy + dz * dz;
                                if best.is_none_or(|current| distance < current.0) {
                                    best = Some((distance, index));
                                }
                            }
                        }
                    }
                }
            }
            radius += 1;
            if (best.is_some() && radius > 1) || radius > 64 {
                break;
            }
        }
        let index = best.ok_or(GeometryError::InvalidVertex)?.1;
        let draw = *high_pose
            .draw_matrices
            .get(index)
            .ok_or(GeometryError::InvalidVertex)?;
        let delta = if let Some(matrix) = matrices.get(&draw) {
            *matrix
        } else {
            let rest = model
                .draw_matrix(draw)
                .ok_or(GeometryError::InvalidVertex)?;
            let animated = model
                .draw_matrix_with_pose(draw, pose)
                .ok_or(GeometryError::InvalidVertex)?;
            let matrix = mmul(&rest, &minv(&animated));
            matrices.insert(draw, matrix);
            matrix
        };
        *point = [
            delta[0] * point[0] + delta[1] * point[1] + delta[2] * point[2] + delta[3],
            delta[4] * point[0] + delta[5] * point[1] + delta[6] * point[2] + delta[7],
            delta[8] * point[0] + delta[9] * point[1] + delta[10] * point[2] + delta[11],
        ];
    }
    Ok(())
}

/// Parse the generated OBJ low mesh using the indexing and polygon fan order
/// consumed by `textures_bake.pack()`.
pub fn parse_obj(text: &[u8], selected_material: Option<&str>) -> Result<LowMesh, GeometryError> {
    let text = std::str::from_utf8(text).map_err(|_| GeometryError::InvalidObj)?;
    let mut positions = Vec::new();
    let mut uvs = Vec::new();
    let mut faces = Vec::new();
    let mut pending_faces = Vec::new();
    let mut current_material = None;
    for line in text.lines() {
        let mut words = line.split_whitespace();
        match words.next() {
            Some("v") => {
                let x = words
                    .next()
                    .and_then(|s| s.parse::<f64>().ok())
                    .ok_or(GeometryError::InvalidObj)?;
                let y = words
                    .next()
                    .and_then(|s| s.parse::<f64>().ok())
                    .ok_or(GeometryError::InvalidObj)?;
                let z = words
                    .next()
                    .and_then(|s| s.parse::<f64>().ok())
                    .ok_or(GeometryError::InvalidObj)?;
                positions.push([x / 0.004, y / 0.004, z / 0.004]);
            }
            Some("vt") => {
                let u = words
                    .next()
                    .and_then(|s| s.parse::<f64>().ok())
                    .ok_or(GeometryError::InvalidObj)?;
                let v = words
                    .next()
                    .and_then(|s| s.parse::<f64>().ok())
                    .ok_or(GeometryError::InvalidObj)?;
                uvs.push([u, v]);
            }
            Some("usemtl") => current_material = words.next(),
            Some("f")
                if selected_material.is_none_or(|selected| current_material == Some(selected)) =>
            {
                let mut face = Vec::new();
                for word in words {
                    let mut indices = word.split('/');
                    let position = parse_obj_raw_index(indices.next())?;
                    let uv = parse_obj_raw_index(indices.next())?;
                    face.push((position, uv));
                }
                if face.len() >= 3 {
                    pending_faces.push((face, positions.len(), uvs.len()));
                }
            }
            _ => {}
        }
    }
    for (face, position_count, uv_count) in pending_faces {
        faces.push(
            face.into_iter()
                .map(|(position, uv)| {
                    Ok((
                        resolve_obj_index(position, positions.len(), position_count)?,
                        resolve_obj_index(uv, uvs.len(), uv_count)?,
                    ))
                })
                .collect::<Result<Vec<_>, GeometryError>>()?,
        );
    }
    Ok(LowMesh {
        positions,
        uvs,
        faces,
    })
}

fn parse_obj_raw_index(value: Option<&str>) -> Result<isize, GeometryError> {
    let index = value
        .and_then(|s| s.parse::<isize>().ok())
        .ok_or(GeometryError::InvalidObj)?;
    if index == 0 {
        return Err(GeometryError::InvalidObj);
    }
    Ok(index)
}

fn resolve_obj_index(
    index: isize,
    final_length: usize,
    length_at_face: usize,
) -> Result<usize, GeometryError> {
    let resolved = if index > 0 {
        index - 1
    } else {
        length_at_face as isize + index
    };
    let resolved = usize::try_from(resolved).map_err(|_| GeometryError::InvalidObj)?;
    if resolved >= final_length {
        return Err(GeometryError::InvalidObj);
    }
    Ok(resolved)
}

/// Serialize the complete C rasterizer input. `aa` is the decoded sample
/// table: each entry is an (x, y) offset; an empty slice selects the native
/// single-sample center point.
pub fn serialize_bake_job(
    low: &LowMesh,
    high: &HighMesh,
    materials: &[BakedMaterial],
    textures: &[TextureLevel],
    options: BakeOptions,
    aa: &[[f32; 2]],
) -> Result<Vec<u8>, GeometryError> {
    let mut triangles = Vec::new();
    let mut loops = Vec::<(usize, [f64; 2])>::new();
    let mut polygons = Vec::<(usize, usize)>::new();
    let mut edge = HashMap::<(usize, usize), usize>::new();
    let mut adjacency = Vec::<i32>::new();

    for face in &low.faces {
        let start = loops.len();
        let count = face.len();
        polygons.push((start, count));
        for &(position, uv) in face {
            let uv = *low.uvs.get(uv).ok_or(GeometryError::InvalidObj)?;
            loops.push((position, uv));
            adjacency.push(-1);
        }
        for k in 0..count {
            let a = face[k].0;
            let b = face[(k + 1) % count].0;
            let key = if a < b { (a, b) } else { (b, a) };
            let current = start + k;
            if let Some(&other) = edge.get(&key) {
                adjacency[current] = i32::try_from(other).map_err(|_| GeometryError::TooLarge)?;
                adjacency[other] = i32::try_from(current).map_err(|_| GeometryError::TooLarge)?;
            } else {
                edge.insert(key, current);
            }
        }
        for k in 1..count - 1 {
            triangles.push(([start, start + k, start + k + 1], polygons.len() - 1));
        }
    }

    let mut bounds = [[f64::INFINITY, f64::NEG_INFINITY]; 3];
    for point in &low.positions {
        for axis in 0..3 {
            bounds[axis][0] = bounds[axis][0].min(point[axis]);
            bounds[axis][1] = bounds[axis][1].max(point[axis]);
        }
    }
    let extent = if low.positions.is_empty() {
        0.0
    } else {
        (0..3)
            .map(|axis| bounds[axis][1] - bounds[axis][0])
            .fold(0.0, f64::max)
            * options.extent_factor
    } as f32;

    let pixel_count = options
        .width
        .checked_mul(options.height)
        .ok_or(GeometryError::TooLarge)?;
    let table_samples = aa
        .len()
        .checked_div(pixel_count)
        .filter(|&n| n > 1 && n * pixel_count == aa.len());
    let use_aa_table = table_samples.is_some();
    let sample_count = table_samples.unwrap_or(1);
    let ns = i32::try_from(sample_count).map_err(|_| GeometryError::TooLarge)?;
    let nlow = i32::try_from(low.positions.len()).map_err(|_| GeometryError::TooLarge)?;
    let ntri = i32::try_from(triangles.len()).map_err(|_| GeometryError::TooLarge)?;
    let npoly = i32::try_from(polygons.len()).map_err(|_| GeometryError::TooLarge)?;
    let nloops = i32::try_from(loops.len()).map_err(|_| GeometryError::TooLarge)?;
    let nhigh = i32::try_from(high.positions.len()).map_err(|_| GeometryError::TooLarge)?;
    let nhtri = i32::try_from(high.triangles.len()).map_err(|_| GeometryError::TooLarge)?;
    let nmat = i32::try_from(materials.len()).map_err(|_| GeometryError::TooLarge)?;
    let ntex = i32::try_from(textures.len()).map_err(|_| GeometryError::TooLarge)?;
    if high.uvs.len() != high.positions.len()
        || high.colors.len() != high.positions.len()
        || high.normals.len() != high.positions.len()
        || high.triangle_materials.len() != high.triangles.len()
        || high.materials.len() != materials.len()
    {
        return Err(GeometryError::InvalidMaterial);
    }

    let mut out = Vec::new();
    out.extend_from_slice(b"TBK1");
    push_i32(
        &mut out,
        i32::try_from(options.width).map_err(|_| GeometryError::TooLarge)?,
    );
    push_i32(
        &mut out,
        i32::try_from(options.height).map_err(|_| GeometryError::TooLarge)?,
    );
    push_i32(&mut out, options.margin);
    push_i32(&mut out, options.flags | if use_aa_table { 32 } else { 0 });
    push_f32(&mut out, extent);
    push_i32(&mut out, ns);
    if use_aa_table {
        for sample in aa {
            push_f32(&mut out, sample[0]);
            push_f32(&mut out, sample[1]);
        }
    } else {
        push_f32(&mut out, 0.0);
        push_f32(&mut out, 0.0);
    }
    push_i32(&mut out, nlow);
    for point in &low.positions {
        for value in point {
            push_f32(&mut out, *value as f32);
        }
    }
    push_i32(&mut out, ntri);
    for (triangle, polygon) in &triangles {
        for &loop_index in triangle {
            push_i32(
                &mut out,
                i32::try_from(loops[loop_index].0).map_err(|_| GeometryError::TooLarge)?,
            );
        }
        for &loop_index in triangle {
            for value in loops[loop_index].1 {
                push_f32(&mut out, value as f32);
            }
        }
        push_i32(
            &mut out,
            i32::try_from(*polygon).map_err(|_| GeometryError::TooLarge)?,
        );
    }
    push_i32(&mut out, npoly);
    push_i32(&mut out, nloops);
    for &(start, count) in &polygons {
        push_i32(
            &mut out,
            i32::try_from(start).map_err(|_| GeometryError::TooLarge)?,
        );
        push_i32(
            &mut out,
            i32::try_from(count).map_err(|_| GeometryError::TooLarge)?,
        );
    }
    for (index, &(position, uv)) in loops.iter().enumerate() {
        push_i32(
            &mut out,
            i32::try_from(position).map_err(|_| GeometryError::TooLarge)?,
        );
        push_f32(&mut out, uv[0] as f32);
        push_f32(&mut out, uv[1] as f32);
        push_i32(&mut out, adjacency[index]);
    }
    push_i32(&mut out, nhigh);
    for point in &high.positions {
        for value in point {
            push_f32(&mut out, *value as f32);
        }
    }
    for uv in &high.uvs {
        for value in uv {
            push_f32(&mut out, *value as f32);
        }
    }
    for color in &high.colors {
        for value in color {
            push_f32(&mut out, *value as f32);
        }
    }
    for normal in &high.normals {
        for value in normal {
            push_f32(&mut out, *value as f32);
        }
    }
    push_i32(&mut out, nhtri);
    for (triangle, &material) in high.triangles.iter().zip(&high.triangle_materials) {
        for &index in triangle {
            push_i32(
                &mut out,
                i32::try_from(index).map_err(|_| GeometryError::TooLarge)?,
            );
        }
        push_i32(
            &mut out,
            i32::try_from(material).map_err(|_| GeometryError::TooLarge)?,
        );
    }
    push_i32(&mut out, nmat);
    for material in materials {
        push_i32(&mut out, material.texture);
        push_i32(&mut out, material.wrap_s);
        push_i32(&mut out, material.wrap_t);
        push_i32(&mut out, material.alpha_mode);
        push_i32(&mut out, i32::from(material.transform.is_some()));
        for value in material.factor {
            push_f32(&mut out, value);
        }
        push_f32(&mut out, material.cutoff);
        for value in material.transform.unwrap_or([0.0, 0.0, 1.0, 1.0]) {
            push_f32(&mut out, value);
        }
    }
    push_i32(&mut out, ntex);
    for texture in textures {
        push_i32(&mut out, texture.width as i32);
        push_i32(&mut out, texture.height as i32);
        let size = (texture.width as usize)
            .checked_mul(texture.height as usize)
            .and_then(|n| n.checked_mul(4))
            .ok_or(GeometryError::TooLarge)?;
        if texture.rgba.len() != size {
            return Err(GeometryError::InvalidMaterial);
        }
        out.extend_from_slice(&texture.rgba);
    }
    Ok(out)
}

fn push_i32(out: &mut Vec<u8>, value: i32) {
    out.extend_from_slice(&value.to_le_bytes());
}
fn push_f32(out: &mut Vec<u8>, value: f32) {
    out.extend_from_slice(&value.to_le_bytes());
}

fn transform_normal(matrix: &[f64; 12], normal: [f64; 3]) -> [f64; 3] {
    let a = &matrix[0..4];
    let b = &matrix[4..8];
    let c = &matrix[8..12];
    let cof = [
        [
            b[1] * c[2] - b[2] * c[1],
            b[2] * c[0] - b[0] * c[2],
            b[0] * c[1] - b[1] * c[0],
        ],
        [
            c[1] * a[2] - c[2] * a[1],
            c[2] * a[0] - c[0] * a[2],
            c[0] * a[1] - c[1] * a[0],
        ],
        [
            a[1] * b[2] - a[2] * b[1],
            a[2] * b[0] - a[0] * b[2],
            a[0] * b[1] - a[1] * b[0],
        ],
    ];
    let det = a[0] * cof[0][0] + a[1] * cof[0][1] + a[2] * cof[0][2];
    let sign = if det < 0.0 { -1.0 } else { 1.0 };
    [0, 1, 2].map(|row| {
        sign * (cof[row][0] * normal[0] + cof[row][1] * normal[1] + cof[row][2] * normal[2])
    })
}

/// Prepare the material-filtered high mesh used by the C rasterizer.
///
/// Shape and vertex traversal matches the native Python builder. Materials are
/// compacted in first-use order; degenerate triangles are omitted and triangle
/// winding is reversed to match `textures_bake.build_high`.
pub fn prepare_high_mesh(model: &J3dModel, tev: &TevModel) -> Result<HighMesh, GeometryError> {
    prepare_high_mesh_with_options(model, tev, false, &[])
}

/// Variant used by non-static bake recipes. `fix_unscaled_joints` applies the
/// native no-scale correction before the draw-matrix transform; excluded
/// material names are omitted exactly as in `textures_bake.build_high()`.
pub fn prepare_high_mesh_with_options(
    model: &J3dModel,
    tev: &TevModel,
    fix_unscaled_joints: bool,
    excluded_materials: &[String],
) -> Result<HighMesh, GeometryError> {
    prepare_high_mesh_with_pose_and_visibility(
        model,
        tev,
        fix_unscaled_joints,
        excluded_materials,
        None,
        None,
    )
}

/// Variant that omits shapes hidden by a BVA visibility animation.
pub fn prepare_high_mesh_with_visibility(
    model: &J3dModel,
    tev: &TevModel,
    fix_unscaled_joints: bool,
    excluded_materials: &[String],
    visible_shapes: Option<&[bool]>,
) -> Result<HighMesh, GeometryError> {
    prepare_high_mesh_with_pose_and_visibility(
        model,
        tev,
        fix_unscaled_joints,
        excluded_materials,
        None,
        visible_shapes,
    )
}

/// Variant that applies a fitted BCK pose and optional BVA shape visibility.
pub fn prepare_high_mesh_with_pose_and_visibility(
    model: &J3dModel,
    tev: &TevModel,
    fix_unscaled_joints: bool,
    excluded_materials: &[String],
    pose: Option<&[[f64; 12]]>,
    visible_shapes: Option<&[bool]>,
) -> Result<HighMesh, GeometryError> {
    let mut high = HighMesh {
        positions: Vec::new(),
        uvs: Vec::new(),
        colors: Vec::new(),
        normals: Vec::new(),
        draw_matrices: Vec::new(),
        triangles: Vec::new(),
        triangle_materials: Vec::new(),
        materials: Vec::new(),
    };
    let mut material_indices = HashMap::<usize, u32>::new();
    let joint_fixes = if fix_unscaled_joints {
        no_scale_fixes(model)
    } else {
        None
    };
    let normal_data = model
        .normals
        .get(&10)
        .filter(|values| !values.is_empty())
        .or_else(|| model.normals.get(&25).filter(|values| !values.is_empty()));
    let normal_attribute = if model
        .normals
        .get(&10)
        .is_some_and(|values| !values.is_empty())
    {
        10
    } else {
        25
    };

    for (shape_index, shape) in model.shapes.iter().enumerate() {
        if visible_shapes.is_some_and(|visible| !visible.get(shape_index).copied().unwrap_or(false))
        {
            continue;
        }
        let Some(material) = shape.material else {
            continue;
        };
        if material >= tev.material_count() || shape.triangles.is_empty() {
            continue;
        }
        if excluded_materials
            .iter()
            .any(|excluded| tev.material_name(material).ok() == Some(excluded.as_str()))
        {
            continue;
        }
        let compact_material = if let Some(&compact) = material_indices.get(&material) {
            compact
        } else {
            let compact =
                u32::try_from(high.materials.len()).map_err(|_| GeometryError::TooLarge)?;
            high.materials.push(material);
            material_indices.insert(material, compact);
            compact
        };
        let info = tev
            .bake_material_info(material)
            .map_err(|_| GeometryError::InvalidMaterial)?;
        let uv_attribute = (13..=20)
            .find(|&attribute| {
                shape
                    .vertices
                    .iter()
                    .all(|v| v.attributes.contains_key(&attribute))
                    && attribute == 13 + info.uv_src.max(0) as u32
            })
            .or_else(|| {
                (13..=20).find(|&attribute| {
                    shape
                        .vertices
                        .iter()
                        .all(|v| v.attributes.contains_key(&attribute))
                })
            });
        let has_color = !model.colors.is_empty()
            && shape
                .vertices
                .iter()
                .all(|v| v.attributes.contains_key(&11));
        let base = u32::try_from(high.positions.len()).map_err(|_| GeometryError::TooLarge)?;

        for vertex in &shape.vertices {
            high.draw_matrices.push(vertex.draw_matrix);
            let position_index = vertex
                .attributes
                .get(&9)
                .copied()
                .unwrap_or(0)
                .min(model.positions.len().saturating_sub(1));
            let position = model
                .positions
                .get(position_index)
                .copied()
                .ok_or(GeometryError::InvalidVertex)?;
            let mut matrix = if let Some(pose) = pose {
                model.draw_matrix_with_pose(vertex.draw_matrix, pose)
            } else {
                model.draw_matrix(vertex.draw_matrix)
            }
            .ok_or(GeometryError::InvalidVertex)?;
            if let Some(fixes) = &joint_fixes {
                let influences = model
                    .influences(vertex.draw_matrix)
                    .ok_or(GeometryError::InvalidVertex)?;
                let total = influences.iter().map(|(_, weight)| *weight).sum::<f64>();
                let total = if total == 0.0 { 1.0 } else { total };
                let mut correction = [0.0; 12];
                for (joint, weight) in influences {
                    let fix = fixes.get(joint).ok_or(GeometryError::InvalidVertex)?;
                    for k in 0..12 {
                        correction[k] += weight / total * fix[k];
                    }
                }
                matrix = mmul(&correction, &matrix);
            }
            let transformed = [
                matrix[0] * position[0]
                    + matrix[1] * position[1]
                    + matrix[2] * position[2]
                    + matrix[3],
                matrix[4] * position[0]
                    + matrix[5] * position[1]
                    + matrix[6] * position[2]
                    + matrix[7],
                matrix[8] * position[0]
                    + matrix[9] * position[1]
                    + matrix[10] * position[2]
                    + matrix[11],
            ];
            high.positions.push(transformed);

            let normal = if let (Some(normals), Some(&index)) =
                (normal_data, vertex.attributes.get(&normal_attribute))
            {
                transform_normal(&matrix, normals[index.min(normals.len().saturating_sub(1))])
            } else {
                [0.0; 3]
            };
            high.normals.push(normal);

            let uv = uv_attribute
                .and_then(|attribute| {
                    let index = *vertex.attributes.get(&attribute)?;
                    model
                        .tex_coords
                        .get((attribute - 13) as usize)?
                        .get(index)
                        .copied()
                })
                .unwrap_or([0.0, 1.0]);
            high.uvs.push(uv);

            let color = if has_color {
                let index = vertex.attributes[&11].min(model.colors.len() - 1);
                model.colors[index].map(|channel| channel as f64 / 255.0)
            } else {
                [1.0; 4]
            };
            high.colors.push(color);
        }

        for &[a, b, c] in &shape.triangles {
            if a == b || b == c || a == c {
                continue;
            }
            let a = base
                .checked_add(u32::try_from(a).map_err(|_| GeometryError::TooLarge)?)
                .ok_or(GeometryError::TooLarge)?;
            let b = base
                .checked_add(u32::try_from(b).map_err(|_| GeometryError::TooLarge)?)
                .ok_or(GeometryError::TooLarge)?;
            let c = base
                .checked_add(u32::try_from(c).map_err(|_| GeometryError::TooLarge)?)
                .ok_or(GeometryError::TooLarge)?;
            high.triangles.push([a, c, b]);
            high.triangle_materials.push(compact_material);
        }
    }
    Ok(high)
}

fn no_scale_fixes(model: &J3dModel) -> Option<Vec<[f64; 12]>> {
    if model
        .joints
        .iter()
        .all(|joint| joint.scale.iter().all(|scale| (scale - 1.0).abs() < 1e-9))
    {
        return None;
    }
    let count = model.joints.len();
    let mut with_scale = vec![[0.0; 12]; count];
    let mut without_scale = vec![[0.0; 12]; count];
    for &index in &model.order {
        let joint = &model.joints[index];
        let parent = model.parent[index];
        let mut scale = joint.scale;
        let inverse_parent = if model.matrix_calc == 2 && joint.scale_compensate {
            parent.map(|parent| {
                model.joints[parent]
                    .scale
                    .map(|value| if value != 0.0 { 1.0 / value } else { 0.0 })
            })
        } else {
            None
        };
        if let Some(parent) = parent.filter(|_| model.matrix_calc == 2 && joint.scale_compensate) {
            let parent_scale = model.joints[parent].scale;
            for axis in 0..3 {
                scale[axis] = if parent_scale[axis] != 0.0 {
                    scale[axis] / parent_scale[axis]
                } else {
                    scale[axis]
                };
            }
        }
        for value in &mut scale {
            if value.abs() <= 1e-6 {
                *value = 1.0;
            }
        }
        let local_scaled = trs_euler(joint.translation, joint.rotation, scale, inverse_parent);
        let local_unscaled = trs_euler(joint.translation, joint.rotation, [1.0; 3], None);
        with_scale[index] = parent
            .map(|parent| mmul(&with_scale[parent], &local_scaled))
            .unwrap_or(local_scaled);
        without_scale[index] = parent
            .map(|parent| mmul(&without_scale[parent], &local_unscaled))
            .unwrap_or(local_unscaled);
    }
    Some(
        (0..count)
            .map(|joint| mmul(&without_scale[joint], &minv(&with_scale[joint])))
            .collect(),
    )
}

/// Resolve the compact high-mesh materials and texture pixels. This mirrors
/// the material portion of `textures_bake.material()` for both raw `parts`
/// recipes and TEV-rendered `bake` recipes.
pub fn build_bake_materials(
    tev: &TevModel,
    high: &HighMesh,
    raw: bool,
    stage_limit: Option<usize>,
) -> Result<(Vec<BakedMaterial>, Vec<TextureLevel>), GeometryError> {
    let mut materials = Vec::with_capacity(high.materials.len());
    let mut textures = Vec::new();
    for &source_material in &high.materials {
        let info = tev
            .bake_material_info(source_material)
            .map_err(|_| GeometryError::InvalidMaterial)?;
        let source_texture = tev
            .bake_texture_index(source_material)
            .map_err(|_| GeometryError::InvalidMaterial)?;
        let (texture, factor, wraps) = if let Some(source_texture) = source_texture {
            let (width, height, rgba) = if raw {
                let level = tev
                    .texture_level(source_texture)
                    .map_err(|_| GeometryError::InvalidMaterial)?;
                (level.width, level.height, level.rgba)
            } else {
                let (width, height, rgba) = tev
                    .effective_rgba_with_stages(source_material, stage_limit)
                    .map_err(|_| GeometryError::InvalidMaterial)?;
                (
                    u16::try_from(width).map_err(|_| GeometryError::TooLarge)?,
                    u16::try_from(height).map_err(|_| GeometryError::TooLarge)?,
                    rgba,
                )
            };
            let (wrap_s, wrap_t) = tev
                .texture_wraps(source_texture)
                .map_err(|_| GeometryError::InvalidMaterial)?;
            let wraps = (wrap_mode(wrap_s), wrap_mode(wrap_t));
            let index = i32::try_from(textures.len()).map_err(|_| GeometryError::TooLarge)?;
            textures.push(TextureLevel {
                width,
                height,
                rgba,
            });
            (index, [1.0; 4], wraps)
        } else if raw {
            (
                -1,
                tev.material_color(source_material)
                    .map_err(|_| GeometryError::InvalidMaterial)?
                    .map(|v| v as f32),
                (1, 1),
            )
        } else {
            let (width, height, rgba) = tev
                .effective_rgba_with_stages(source_material, stage_limit)
                .map_err(|_| GeometryError::InvalidMaterial)?;
            let count = width.checked_mul(height).ok_or(GeometryError::TooLarge)? as f64;
            if count == 0.0 || rgba.len() != width * height * 4 {
                return Err(GeometryError::InvalidMaterial);
            }
            let mut factor = [0.0f32; 4];
            for pixel in rgba.chunks_exact(4) {
                for channel in 0..4 {
                    factor[channel] += pixel[channel] as f32 / (255.0 * count as f32);
                }
            }
            (-1, factor, (1, 1))
        };
        let transform = info.srt.as_ref().and_then(|srt| {
            let (sx, sy) = (srt.scale[0], srt.scale[1]);
            let (tx, ty) = (srt.translation[0], srt.translation[1]);
            ((sx, sy) != (1.0, 1.0) || tx != 0.0 || ty != 0.0 || srt.rotation != 0).then_some([
                (srt.center[0] * (1.0 - sx) + tx) as f32,
                (srt.center[1] * (1.0 - sy) + ty) as f32,
                sx as f32,
                sy as f32,
            ])
        });
        materials.push(BakedMaterial {
            texture,
            wrap_s: wraps.0,
            wrap_t: wraps.1,
            alpha_mode: info.alpha_mode as i32,
            cutoff: info.alpha_ref,
            factor,
            transform,
        });
    }
    Ok((materials, textures))
}

fn wrap_mode(mode: u8) -> i32 {
    if mode <= 2 {
        mode as i32
    } else {
        1
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn obj_faces_can_reference_later_vertex_and_uv_records() {
        let input =
            b"usemtl wall\nf 3/3 1/1 2/2\nv 0 0 0\nvt 0 0\nv 1 0 0\nvt 1 0\nv 0 1 0\nvt 0 1\n";
        let mesh = parse_obj(input, Some("wall")).unwrap();
        assert_eq!(mesh.positions.len(), 3);
        assert_eq!(mesh.uvs.len(), 3);
        assert_eq!(mesh.faces, vec![vec![(2, 2), (0, 0), (1, 1)]]);
    }
}

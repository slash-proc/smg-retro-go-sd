//! Geometry point extraction used by the native stage construction pipeline.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use crate::{
    asset_formats,
    j3d_model::J3dModel,
    stage_galaxy::{self, Matrix, StageGalaxy, StageObject},
    stage_recipe::StageSource,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StageGeometryError {
    InvalidModel,
    InvalidCollision,
    InvalidArchive,
    MissingArchive,
    MissingObject,
}

#[derive(Debug, Clone, PartialEq)]
pub struct GeometryPool {
    pub positions: Vec<[f64; 3]>,
    pub uvs: Vec<Vec<[f64; 2]>>,
    pub candidate_names: Vec<String>,
}

type Matrix4 = [[f64; 4]; 4];

fn placement_matrix(raw: &([f64; 3], [f64; 3], [f64; 3])) -> Matrix4 {
    let mut matrix = stage_galaxy::trs(raw.0, raw.1);
    for row in &mut matrix {
        for column in 0..3 {
            row[column] *= raw.2[column];
        }
    }
    [matrix[0], matrix[1], matrix[2], [0.0, 0.0, 0.0, 1.0]]
}

fn mat4_mul(a: Matrix4, b: Matrix4) -> Matrix4 {
    let mut out = [[0.0; 4]; 4];
    for row in 0..4 {
        for column in 0..4 {
            out[row][column] = a[row][0] * b[0][column]
                + a[row][1] * b[1][column]
                + a[row][2] * b[2][column]
                + a[row][3] * b[3][column];
        }
    }
    out
}

fn placement_world(object: &crate::stage_galaxy::StageObject, stage: &StageGalaxy) -> Matrix4 {
    let mut chain = vec![placement_matrix(&object.raw)];
    let mut placement_index = object.zone_placement;
    while let Some(index) = placement_index {
        let placement = &stage.placements[index];
        chain.push(placement_matrix(&placement.raw));
        placement_index = placement.parent;
    }
    let mut matrix = *chain.last().unwrap();
    for child in chain[..chain.len() - 1].iter().rev() {
        matrix = mat4_mul(matrix, *child);
    }
    matrix
}

/// Reconstruct the ordered point/UV pool consumed by a stage OBJ recipe.
/// Archives are fetched one at a time so callers can avoid retaining the full
/// set of ObjectData files while building a large stage.
pub fn build_pool(
    stage: &StageGalaxy,
    sources: &[StageSource],
    mut archive: impl FnMut(&str) -> Option<Vec<u8>>,
) -> Result<GeometryPool, StageGeometryError> {
    const OBJ_SCALE: f64 = 1000.0;
    let mut pool = GeometryPool {
        positions: Vec::new(),
        uvs: Vec::new(),
        candidate_names: stage
            .zone_list
            .iter()
            .cloned()
            .chain(stage.objects.iter().map(|object| object.name.clone()))
            .chain(sources.iter().map(|source| source.archive.clone()))
            .filter(|name| !name.is_empty())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect(),
    };
    for source in sources {
        let object = stage
            .objects
            .get(source.object_index)
            .ok_or(StageGeometryError::MissingObject)?;
        let bytes = archive(&source.archive).ok_or(StageGeometryError::MissingArchive)?;
        let files =
            asset_formats::rarc_listfiles_with_extensions(&bytes, &[".bdl", ".bmd", ".kcl"])
                .map_err(|_| StageGeometryError::InvalidArchive)?;
        let kinds = source.kinds.iter().map(String::as_str).collect::<Vec<_>>();
        let rotation: Matrix = stage_galaxy::trs([0.0; 3], object.world_rot);
        let world_scale = object.world_scale;
        let world_pos = object.world_pos;
        let world = [
            [
                rotation[0][0] * world_scale[0],
                rotation[0][1] * world_scale[1],
                rotation[0][2] * world_scale[2],
                world_pos[0],
            ],
            [
                rotation[1][0] * world_scale[0],
                rotation[1][1] * world_scale[1],
                rotation[1][2] * world_scale[2],
                world_pos[1],
            ],
            [
                rotation[2][0] * world_scale[0],
                rotation[2][1] * world_scale[1],
                rotation[2][2] * world_scale[2],
                world_pos[2],
            ],
        ];
        let collision_world = if kinds.contains(&"kclx") {
            Some(placement_world(object, stage))
        } else {
            None
        };
        for (path, data) in files {
            let lower = path.to_lowercase();
            if (lower.ends_with(".bdl") || lower.ends_with(".bmd")) && kinds.contains(&"mdl") {
                let model =
                    J3dModel::parse_stage(&data).map_err(|_| StageGeometryError::InvalidModel)?;
                pool.candidate_names
                    .extend(model.texture_names.iter().cloned());
                pool.candidate_names
                    .extend(model.material_names.iter().cloned());
                let model = model_points_from(&model)?;
                for (point, uvs) in model.positions.into_iter().zip(model.uvs) {
                    pool.positions.push([
                        (world[0][0] * point[0]
                            + world[0][1] * point[1]
                            + world[0][2] * point[2]
                            + world[0][3])
                            / OBJ_SCALE,
                        (world[1][0] * point[0]
                            + world[1][1] * point[1]
                            + world[1][2] * point[2]
                            + world[1][3])
                            / OBJ_SCALE,
                        (world[2][0] * point[0]
                            + world[2][1] * point[1]
                            + world[2][2] * point[2]
                            + world[2][3])
                            / OBJ_SCALE,
                    ]);
                    pool.uvs.push(uvs);
                }
            } else if lower.ends_with(".kcl") {
                if kinds.contains(&"kcl") || kinds.contains(&"kclx") {
                    let triangles = kcl_triangles(&data)?;
                    if kinds.contains(&"kclx") {
                        let matrix = collision_world.as_ref().unwrap();
                        for triangle in &triangles {
                            for point in triangle {
                                pool.positions.push([
                                    (matrix[0][0] * point[0]
                                        + matrix[0][1] * point[1]
                                        + matrix[0][2] * point[2]
                                        + matrix[0][3])
                                        / OBJ_SCALE,
                                    (matrix[1][0] * point[0]
                                        + matrix[1][1] * point[1]
                                        + matrix[1][2] * point[2]
                                        + matrix[1][3])
                                        / OBJ_SCALE,
                                    (matrix[2][0] * point[0]
                                        + matrix[2][1] * point[1]
                                        + matrix[2][2] * point[2]
                                        + matrix[2][3])
                                        / OBJ_SCALE,
                                ]);
                                pool.uvs.push(Vec::new());
                            }
                        }
                    }
                    if kinds.contains(&"kcl") {
                        for point in kcl_points(&triangles) {
                            pool.positions.push([
                                (world[0][0] * point[0]
                                    + world[0][1] * point[1]
                                    + world[0][2] * point[2]
                                    + world[0][3])
                                    / OBJ_SCALE,
                                (world[1][0] * point[0]
                                    + world[1][1] * point[1]
                                    + world[1][2] * point[2]
                                    + world[1][3])
                                    / OBJ_SCALE,
                                (world[2][0] * point[0]
                                    + world[2][1] * point[1]
                                    + world[2][2] * point[2]
                                    + world[2][3])
                                    / OBJ_SCALE,
                            ]);
                            pool.uvs.push(Vec::new());
                        }
                    }
                }
            }
        }
    }
    pool.candidate_names.sort();
    pool.candidate_names.dedup();
    Ok(pool)
}

/// Stage source points keyed by draw matrix and vertex position index, with
/// the material-selected UV values attached to each point.
#[derive(Debug, Clone, PartialEq)]
pub struct ModelPoints {
    pub positions: Vec<[f64; 3]>,
    pub uvs: Vec<Vec<[f64; 2]>>,
}

pub fn model_points(data: &[u8]) -> Result<ModelPoints, StageGeometryError> {
    let model = J3dModel::parse_stage(data).map_err(|_| StageGeometryError::InvalidModel)?;
    model_points_from(&model)
}

fn model_points_from(model: &J3dModel) -> Result<ModelPoints, StageGeometryError> {
    let mut point_indices = HashMap::<(usize, usize), usize>::new();
    let mut positions = Vec::new();
    let mut uvs: Vec<Vec<[f64; 2]>> = Vec::new();
    for shape in &model.shapes {
        let uv_source = shape
            .material
            .and_then(|material| model.material_uv_src.get(material))
            .copied()
            .unwrap_or(0);
        let source_coords = usize::try_from(uv_source)
            .ok()
            .and_then(|source| model.tex_coords.get(source));
        let uv_attribute = if uv_source >= 0 {
            Some(13 + uv_source as u32)
        } else {
            None
        };
        for vertex in &shape.vertices {
            let Some(&position_index) = vertex.attributes.get(&9) else {
                continue;
            };
            let key = (vertex.draw_matrix, position_index);
            let point_index = if let Some(&point_index) = point_indices.get(&key) {
                point_index
            } else {
                let position = model
                    .anchor_point(position_index, vertex.draw_matrix)
                    .ok_or(StageGeometryError::InvalidModel)?;
                let point_index = positions.len();
                point_indices.insert(key, point_index);
                positions.push(position);
                uvs.push(Vec::new());
                point_index
            };
            if let Some((coords, &uv_index)) = uv_attribute
                .and_then(|attribute| source_coords.zip(vertex.attributes.get(&attribute)))
            {
                if let Some(&uv) = coords.get(uv_index) {
                    let uv = [uv[0], 1.0 - uv[1]];
                    if !uvs[point_index].contains(&uv) {
                        uvs[point_index].push(uv);
                    }
                }
            }
        }
    }
    Ok(ModelPoints { positions, uvs })
}

/// Reconstruct the per-texture surface-to-UV area ratios used by the native
/// stage projector's `tsrc` mode.
pub fn texture_repeats(
    archive: &[u8],
    object: &StageObject,
) -> Result<BTreeMap<String, f64>, StageGeometryError> {
    let files = asset_formats::rarc_listfiles_with_extensions(archive, &[".bdl", ".bmd"])
        .map_err(|_| StageGeometryError::InvalidArchive)?;
    let rotation: Matrix = stage_galaxy::trs([0.0; 3], object.world_rot);
    let scale = object.world_scale;
    let transform = [
        [
            rotation[0][0] * scale[0] / 256.0,
            rotation[0][1] * scale[1] / 256.0,
            rotation[0][2] * scale[2] / 256.0,
        ],
        [
            rotation[1][0] * scale[0] / 256.0,
            rotation[1][1] * scale[1] / 256.0,
            rotation[1][2] * scale[2] / 256.0,
        ],
        [
            rotation[2][0] * scale[0] / 256.0,
            rotation[2][1] * scale[1] / 256.0,
            rotation[2][2] * scale[2] / 256.0,
        ],
    ];
    let mut areas = BTreeMap::<String, [f64; 2]>::new();
    for (_, bytes) in files {
        let model = J3dModel::parse_stage(&bytes).map_err(|_| StageGeometryError::InvalidModel)?;
        for shape in &model.shapes {
            let Some(material) = shape.material else {
                continue;
            };
            let Some(&uv_source) = model.material_uv_src.get(material) else {
                continue;
            };
            if uv_source < 0 {
                continue;
            }
            let Some(Some(texture)) = model.material_texture_names.get(material) else {
                continue;
            };
            let source = uv_source as usize;
            let Some(coords) = model.tex_coords.get(source) else {
                continue;
            };
            let attribute = 13 + source as u32;
            let total = areas.entry(texture.clone()).or_insert([0.0, 0.0]);
            for triangle in &shape.triangles {
                let mut positions = [[0.0; 3]; 3];
                let mut uvs = [[0.0; 2]; 3];
                let mut valid = true;
                for (corner, &vertex_index) in triangle.iter().enumerate() {
                    let Some(vertex) = shape.vertices.get(vertex_index) else {
                        valid = false;
                        break;
                    };
                    let (Some(&position_index), Some(&uv_index)) =
                        (vertex.attributes.get(&9), vertex.attributes.get(&attribute))
                    else {
                        valid = false;
                        break;
                    };
                    let Some(point) = model.anchor_point(position_index, vertex.draw_matrix) else {
                        valid = false;
                        break;
                    };
                    let Some(&uv) = coords.get(uv_index) else {
                        valid = false;
                        break;
                    };
                    for axis in 0..3 {
                        positions[corner][axis] = transform[axis][0] * point[0]
                            + transform[axis][1] * point[1]
                            + transform[axis][2] * point[2];
                    }
                    uvs[corner] = uv;
                }
                if !valid {
                    continue;
                }
                let edge1 = [
                    positions[1][0] - positions[0][0],
                    positions[1][1] - positions[0][1],
                    positions[1][2] - positions[0][2],
                ];
                let edge2 = [
                    positions[2][0] - positions[0][0],
                    positions[2][1] - positions[0][1],
                    positions[2][2] - positions[0][2],
                ];
                let cross = [
                    edge1[1] * edge2[2] - edge1[2] * edge2[1],
                    edge1[2] * edge2[0] - edge1[0] * edge2[2],
                    edge1[0] * edge2[1] - edge1[1] * edge2[0],
                ];
                let uv1 = [uvs[1][0] - uvs[0][0], uvs[1][1] - uvs[0][1]];
                let uv2 = [uvs[2][0] - uvs[0][0], uvs[2][1] - uvs[0][1]];
                let uv_area = (uv1[0] * uv2[1] - uv1[1] * uv2[0]).abs() / 2.0;
                if uv_area > 1e-9 {
                    total[0] += (cross[0] * cross[0] + cross[1] * cross[1] + cross[2] * cross[2])
                        .sqrt()
                        / 2.0;
                    total[1] += uv_area;
                }
            }
        }
    }
    Ok(areas
        .into_iter()
        .filter_map(|(texture, [area, uv_area])| {
            (uv_area > 0.0).then(|| (texture, 0.25f64.max((area / uv_area).sqrt())))
        })
        .collect())
}

fn be_u32(data: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_be_bytes(
        data.get(at..at.checked_add(4)?)?.try_into().ok()?,
    ))
}

fn be_f32(data: &[u8], at: usize) -> Option<f64> {
    Some(f32::from_bits(be_u32(data, at)?) as f64)
}

/// Reconstruct triangles from a Nintendo KCL collision mesh.
pub fn kcl_triangles(data: &[u8]) -> Result<Vec<[[f64; 3]; 3]>, StageGeometryError> {
    let data =
        asset_formats::yaz0_decompress(data).map_err(|_| StageGeometryError::InvalidCollision)?;
    let pos_offset = be_u32(&data, 0).ok_or(StageGeometryError::InvalidCollision)? as usize;
    let nrm_offset = be_u32(&data, 4).ok_or(StageGeometryError::InvalidCollision)? as usize;
    let prism_offset = be_u32(&data, 8).ok_or(StageGeometryError::InvalidCollision)? as usize;
    let octree_offset = be_u32(&data, 12).ok_or(StageGeometryError::InvalidCollision)? as usize;
    let prism_start = prism_offset
        .checked_add(0x10)
        .ok_or(StageGeometryError::InvalidCollision)?;
    let prism_bytes = octree_offset
        .checked_sub(prism_start)
        .ok_or(StageGeometryError::InvalidCollision)?;
    if prism_bytes % 0x10 != 0 {
        return Err(StageGeometryError::InvalidCollision);
    }
    let prism_count = prism_bytes / 0x10;
    let mut prisms = Vec::with_capacity(prism_count);
    let mut max_position = None;
    let mut max_normal = None;
    for index in 0..prism_count {
        let p = prism_start + index * 0x10;
        let height = be_f32(&data, p).ok_or(StageGeometryError::InvalidCollision)?;
        let position = be_u16(&data, p + 4).ok_or(StageGeometryError::InvalidCollision)? as usize;
        let face = be_u16(&data, p + 6).ok_or(StageGeometryError::InvalidCollision)? as usize;
        let edge0 = be_u16(&data, p + 8).ok_or(StageGeometryError::InvalidCollision)? as usize;
        let edge1 = be_u16(&data, p + 10).ok_or(StageGeometryError::InvalidCollision)? as usize;
        let edge2 = be_u16(&data, p + 12).ok_or(StageGeometryError::InvalidCollision)? as usize;
        max_position = Some(max_position.map_or(position, |value: usize| value.max(position)));
        max_normal = Some(
            max_normal.map_or(face.max(edge0).max(edge1).max(edge2), |value: usize| {
                value.max(face).max(edge0).max(edge1).max(edge2)
            }),
        );
        prisms.push((height, position, face, edge0, edge1, edge2));
    }
    if prisms.is_empty() {
        return Ok(Vec::new());
    }
    let position_count = nrm_offset
        .checked_sub(pos_offset)
        .filter(|bytes| bytes % 12 == 0)
        .map(|bytes| bytes / 12)
        .unwrap_or(0)
        .max(max_position.unwrap() + 1);
    let normal_count = prism_start
        .checked_sub(nrm_offset)
        .filter(|bytes| bytes % 12 == 0)
        .map(|bytes| bytes / 12)
        .unwrap_or(0)
        .max(max_normal.unwrap() + 1);
    // Reject sizes before allocation if the file's ranges are malformed.
    if pos_offset
        .checked_add(
            position_count
                .checked_mul(12)
                .ok_or(StageGeometryError::InvalidCollision)?,
        )
        .is_none_or(|end| end > data.len())
        || nrm_offset
            .checked_add(
                normal_count
                    .checked_mul(12)
                    .ok_or(StageGeometryError::InvalidCollision)?,
            )
            .is_none_or(|end| end > data.len())
    {
        return Err(StageGeometryError::InvalidCollision);
    }
    let positions = (0..position_count)
        .map(|index| {
            let p = pos_offset + index * 12;
            Ok([
                be_f32(&data, p).ok_or(StageGeometryError::InvalidCollision)?,
                be_f32(&data, p + 4).ok_or(StageGeometryError::InvalidCollision)?,
                be_f32(&data, p + 8).ok_or(StageGeometryError::InvalidCollision)?,
            ])
        })
        .collect::<Result<Vec<_>, StageGeometryError>>()?;
    let normals = (0..normal_count)
        .map(|index| {
            let p = nrm_offset + index * 12;
            Ok([
                be_f32(&data, p).ok_or(StageGeometryError::InvalidCollision)?,
                be_f32(&data, p + 4).ok_or(StageGeometryError::InvalidCollision)?,
                be_f32(&data, p + 8).ok_or(StageGeometryError::InvalidCollision)?,
            ])
        })
        .collect::<Result<Vec<_>, StageGeometryError>>()?;
    let mut triangles = Vec::with_capacity(prisms.len());
    for (height, point, face, edge0, edge1, edge2) in prisms {
        let a = *positions
            .get(point)
            .ok_or(StageGeometryError::InvalidCollision)?;
        let face = *normals
            .get(face)
            .ok_or(StageGeometryError::InvalidCollision)?;
        let e0 = *normals
            .get(edge0)
            .ok_or(StageGeometryError::InvalidCollision)?;
        let e1 = *normals
            .get(edge1)
            .ok_or(StageGeometryError::InvalidCollision)?;
        let e2 = *normals
            .get(edge2)
            .ok_or(StageGeometryError::InvalidCollision)?;
        let c1 = cross(face, e1);
        let c2 = cross(e0, face);
        let d1 = dot(c1, e2);
        let d2 = dot(c2, e2);
        if d1 == 0.0 || d2 == 0.0 {
            continue;
        }
        let f1 = height / d1;
        let f2 = height / d2;
        let b = [a[0] + c1[0] * f1, a[1] + c1[1] * f1, a[2] + c1[2] * f1];
        let c = [a[0] + c2[0] * f2, a[1] + c2[1] * f2, a[2] + c2[2] * f2];
        if b.iter().chain(&c).all(|value| value.is_finite()) {
            triangles.push([a, b, c]);
        }
    }
    Ok(triangles)
}

pub fn kcl_points(triangles: &[[[f64; 3]; 3]]) -> Vec<[f64; 3]> {
    let mut seen = HashMap::<[u64; 3], ()>::new();
    let mut points = Vec::new();
    for triangle in triangles {
        for point in triangle {
            let rounded = point.map(|value| stage_galaxy::round_to(value, 3));
            let key = rounded.map(|value| {
                if value == 0.0 {
                    0.0f64.to_bits()
                } else {
                    value.to_bits()
                }
            });
            if seen.insert(key, ()).is_none() {
                points.push(rounded);
            }
        }
    }
    points
}

fn be_u16(data: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_be_bytes(
        data.get(at..at.checked_add(2)?)?.try_into().ok()?,
    ))
}

fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

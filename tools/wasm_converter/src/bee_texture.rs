//! Bee Mario wing and antenna atlas generation ported from
//! `textures_bake_bee.parts_atlas`.

use crate::{
    geometry_bake::{
        build_bake_materials, prepare_high_mesh, BakeOptions, BakedMaterial, HighMesh, LowMesh,
    },
    j3d_model::J3dModel,
    tev::TevModel,
    textures_gx::{TextureError, TextureLevel},
};

const TEX: usize = 32;
#[derive(Clone)]
struct Triangle {
    positions: [[f64; 3]; 3],
    uvs: [[f64; 2]; 3],
    material: usize,
    mesh_triangle: usize,
}

#[derive(Clone)]
struct Material {
    name: String,
    texture: i32,
    wrap_s: i32,
    wrap_t: i32,
}

struct High {
    triangles: Vec<Triangle>,
    mesh: HighMesh,
    baked_materials: Vec<BakedMaterial>,
    materials: Vec<Material>,
    textures: Vec<TextureLevel>,
}

impl High {
    fn parse(data: &[u8]) -> Result<Self, TextureError> {
        let model = J3dModel::parse(data).map_err(|_| TextureError::InvalidHeader)?;
        let tev = TevModel::parse(data).map_err(|_| TextureError::InvalidHeader)?;
        let mesh = prepare_high_mesh(&model, &tev).map_err(|_| TextureError::InvalidHeader)?;
        let (baked_materials, textures) = build_bake_materials(&tev, &mesh, true, None)
            .map_err(|_| TextureError::InvalidHeader)?;
        let materials = baked_materials
            .iter()
            .enumerate()
            .map(|(compact, baked)| {
                let source = *mesh
                    .materials
                    .get(compact)
                    .ok_or(TextureError::InvalidHeader)?;
                Ok(Material {
                    name: tev
                        .material_name(source)
                        .map_err(|_| TextureError::InvalidHeader)?
                        .to_owned(),
                    texture: baked.texture,
                    wrap_s: baked.wrap_s,
                    wrap_t: baked.wrap_t,
                })
            })
            .collect::<Result<Vec<_>, TextureError>>()?;
        let mut triangles = Vec::with_capacity(mesh.triangles.len());
        for (index, indices) in mesh.triangles.iter().enumerate() {
            let material = *mesh
                .triangle_materials
                .get(index)
                .ok_or(TextureError::InvalidHeader)? as usize;
            let mut positions = [[0.0; 3]; 3];
            let mut uvs = [[0.0; 2]; 3];
            for (corner, &vertex) in indices.iter().enumerate() {
                let vertex = vertex as usize;
                positions[corner] = *mesh
                    .positions
                    .get(vertex)
                    .ok_or(TextureError::InvalidHeader)?;
                let uv = *mesh.uvs.get(vertex).ok_or(TextureError::InvalidHeader)?;
                uvs[corner] = [uv[0], 1.0 - uv[1]];
            }
            triangles.push(Triangle {
                positions,
                uvs,
                material,
                mesh_triangle: index,
            });
        }
        Ok(Self {
            triangles,
            mesh,
            baked_materials,
            materials,
            textures,
        })
    }
}

#[derive(Clone, Copy)]
struct Tile {
    x: usize,
    y: usize,
    width: usize,
    height: usize,
    normal: Option<[f64; 3]>,
    signs: [f64; 2],
    alpha_cut: f64,
    thicken: bool,
}

const PARTS: [(&str, &str, &str, Tile); 5] = [
    (
        "wingL",
        "wing",
        "WingMat2",
        Tile {
            x: 0,
            y: 0,
            width: 16,
            height: 16,
            normal: None,
            signs: [1.0, 1.0],
            alpha_cut: 0.5,
            thicken: false,
        },
    ),
    (
        "wingR",
        "wing",
        "WingMat2(2)",
        Tile {
            x: 16,
            y: 0,
            width: 16,
            height: 16,
            normal: None,
            signs: [1.0, 1.0],
            alpha_cut: 0.5,
            thicken: false,
        },
    ),
    (
        "wingfast",
        "wing",
        "WingMat",
        Tile {
            x: 0,
            y: 16,
            width: 20,
            height: 16,
            normal: None,
            signs: [1.0, 1.0],
            alpha_cut: 0.0,
            thicken: false,
        },
    ),
    (
        "antennaL",
        "body",
        "1",
        Tile {
            x: 20,
            y: 16,
            width: 6,
            height: 16,
            normal: Some([0.7071, 0.0, -0.7071]),
            signs: [1.0, 1.0],
            alpha_cut: 0.5,
            thicken: true,
        },
    ),
    (
        "antennaR",
        "body",
        "-1",
        Tile {
            x: 26,
            y: 16,
            width: 6,
            height: 16,
            normal: Some([-0.7071, 0.0, -0.7071]),
            signs: [1.0, 1.0],
            alpha_cut: 0.5,
            thicken: true,
        },
    ),
];

pub fn parts_atlas(
    wing_data: &[u8],
    body_data: &[u8],
    signs: &serde_json::Value,
) -> Result<Vec<u8>, TextureError> {
    let wing = High::parse(wing_data)?;
    let body = High::parse(body_data)?;
    let mut atlas = vec![0; TEX * TEX * 4];
    for (name, _, _, tile) in PARTS {
        let mut tile = tile;
        if let Some(values) = signs.get(name).and_then(|value| value.as_array()) {
            if values.len() != 2 {
                return Err(TextureError::InvalidHeader);
            }
            tile.signs = [
                values[0].as_f64().ok_or(TextureError::InvalidHeader)?,
                values[1].as_f64().ok_or(TextureError::InvalidHeader)?,
            ];
        }
        let (source, selected) = if name.starts_with("wing") {
            (&wing, PARTS.iter().find(|part| part.0 == name).unwrap().2)
        } else {
            (&body, PARTS.iter().find(|part| part.0 == name).unwrap().2)
        };
        let selected_triangles = source
            .triangles
            .iter()
            .filter(|triangle| {
                let material = &source.materials[triangle.material];
                if name.starts_with("wing") {
                    material.name == selected
                } else {
                    material.name == "Cap_v"
                        && (triangle.uvs[0][0] + triangle.uvs[1][0] + triangle.uvs[2][0]) / 3.0
                            < 0.45
                        && (triangle.positions[0][0]
                            + triangle.positions[1][0]
                            + triangle.positions[2][0])
                            * selected.parse::<f64>().unwrap_or(0.0)
                            > 0.0
                }
            })
            .cloned()
            .collect::<Vec<_>>();
        if selected_triangles.is_empty() {
            continue;
        }
        let pixels = part_tile(source, &selected_triangles, tile)?;
        for y in 0..tile.height {
            for x in 0..tile.width {
                let src = (y * tile.width + x) * 4;
                let dst = ((tile.y + y) * TEX + tile.x + x) * 4;
                atlas[dst..dst + 4].copy_from_slice(&pixels[src..src + 4]);
            }
        }
    }
    Ok(atlas)
}

/// Produce Mario's bee suit textures by transferring material colour from
/// the Bee Mario surface onto the corresponding pixels in the base Mario map.
pub fn suit_texture(
    bee_data: &[u8],
    mario_data: &[u8],
    obj: &[u8],
    material_name: &str,
    base: &[u8],
) -> Result<Vec<u8>, TextureError> {
    let bee = High::parse(bee_data)?;
    let mario = High::parse(mario_data)?;
    let low = crate::geometry_bake::parse_obj(obj, Some(material_name))
        .map_err(|_| TextureError::InvalidHeader)?;
    let (points, cells) = texture_transfer_samples(&low)?;
    let bee_body = bee
        .triangles
        .iter()
        .filter(|triangle| !is_antenna(&bee, triangle))
        .cloned()
        .collect::<Vec<_>>();
    let bee_suit = bee_body
        .iter()
        .filter(|triangle| {
            matches!(
                bee.materials[triangle.material].name.as_str(),
                "Body1_v" | "Cap_v" | "FurCollar_v"
            )
        })
        .cloned()
        .collect::<Vec<_>>();
    let (mario_colors, mario_hit, mario_suit) =
        transfer(&mario, &mario.triangles, &points, &cells, 25.0)?;
    let (_, _, bee_suit_coverage) = transfer(&bee, &bee_body, &points, &cells, 25.0)?;
    let (bee_colors, bee_hit, _) = transfer(&bee, &bee_suit, &points, &cells, 25.0)?;

    let pixel_count = TEX * TEX;
    if base.len() != pixel_count * 4 {
        return Err(TextureError::InvalidHeader);
    }
    let mut output = base.to_vec();
    let mut replace = vec![false; pixel_count];
    let mut force = vec![false; pixel_count];
    for pixel in 0..pixel_count {
        let offset = pixel * 4;
        let base_color = [
            base[offset] as f64 / 255.0,
            base[offset + 1] as f64 / 255.0,
            base[offset + 2] as f64 / 255.0,
        ];
        force[pixel] = suit_color(
            base_color,
            if material_name == "marioface" {
                "red"
            } else {
                "blue"
            },
        );
        let difference = (0..3)
            .map(|channel| (bee_colors[pixel][channel] - mario_colors[pixel][channel]).abs())
            .fold(0.0, f64::max)
            * 255.0;
        replace[pixel] = bee_hit[pixel]
            && mario_hit[pixel]
            && (mario_suit[pixel] >= 0.5 || bee_suit_coverage[pixel] >= 0.5 || force[pixel])
            && difference > 60.0;
        if replace[pixel] {
            for channel in 0..3 {
                output[offset + channel] = float_byte(bee_colors[pixel][channel]);
            }
        }
    }
    let replacement_pixels = replace
        .iter()
        .enumerate()
        .filter_map(|(index, &selected)| selected.then_some(index))
        .collect::<Vec<_>>();
    if !replacement_pixels.is_empty() {
        for pixel in 0..pixel_count {
            if !force[pixel] || replace[pixel] {
                continue;
            }
            let y = pixel / TEX;
            let x = pixel % TEX;
            let mut best = (u32::MAX, replacement_pixels[0]);
            for &candidate in &replacement_pixels {
                let cy = candidate / TEX;
                let cx = candidate % TEX;
                let distance =
                    (cy as i32 - y as i32).pow(2) as u32 + (cx as i32 - x as i32).pow(2) as u32;
                if distance < best.0 {
                    best = (distance, candidate);
                }
            }
            let source = best.1 * 4;
            let destination = pixel * 4;
            let color = [output[source], output[source + 1], output[source + 2]];
            output[destination..destination + 3].copy_from_slice(&color);
        }
    }
    Ok(output)
}

fn texture_transfer_samples(low: &LowMesh) -> Result<(Vec<[f64; 3]>, Vec<usize>), TextureError> {
    let mut points = Vec::new();
    let mut cells = Vec::new();
    let sample_size = TEX * 4;
    for face in &low.faces {
        for corner in 1..face.len().saturating_sub(1) {
            let vertices = [face[0], face[corner], face[corner + 1]];
            let positions = vertices
                .map(|(position, _)| low.positions.get(position).copied())
                .into_iter()
                .collect::<Option<Vec<_>>>()
                .ok_or(TextureError::InvalidHeader)?;
            let uvs = vertices
                .map(|(_, uv)| low.uvs.get(uv).copied())
                .into_iter()
                .collect::<Option<Vec<_>>>()
                .ok_or(TextureError::InvalidHeader)?;
            let denominator = (uvs[1][1] - uvs[2][1]) * (uvs[0][0] - uvs[2][0])
                + (uvs[2][0] - uvs[1][0]) * (uvs[0][1] - uvs[2][1]);
            if denominator.abs() < 1.0e-12 {
                continue;
            }
            let mut bounds = [[f64::INFINITY, f64::NEG_INFINITY]; 2];
            for uv in &uvs {
                for axis in 0..2 {
                    bounds[axis][0] = bounds[axis][0].min(uv[axis]);
                    bounds[axis][1] = bounds[axis][1].max(uv[axis]);
                }
            }
            let low_x = (bounds[0][0] * sample_size as f64).floor() as i64;
            let high_x = (bounds[0][1] * sample_size as f64).ceil() as i64;
            let low_y = (bounds[1][0] * sample_size as f64).floor() as i64;
            let high_y = (bounds[1][1] * sample_size as f64).ceil() as i64;
            for sy in low_y..=high_y {
                for sx in low_x..=high_x {
                    let u = (sx as f64 + 0.5) / sample_size as f64;
                    let v = (sy as f64 + 0.5) / sample_size as f64;
                    let l1 = ((uvs[1][1] - uvs[2][1]) * (u - uvs[2][0])
                        + (uvs[2][0] - uvs[1][0]) * (v - uvs[2][1]))
                        / denominator;
                    let l2 = ((uvs[2][1] - uvs[0][1]) * (u - uvs[2][0])
                        + (uvs[0][0] - uvs[2][0]) * (v - uvs[2][1]))
                        / denominator;
                    let l3 = 1.0 - l1 - l2;
                    if l1.min(l2).min(l3) < -1.0e-6 {
                        continue;
                    }
                    points.push(std::array::from_fn(|axis| {
                        positions[0][axis] * l1 + positions[1][axis] * l2 + positions[2][axis] * l3
                    }));
                    let tx = (u * TEX as f64).floor() as i64;
                    let ty = (v * TEX as f64).floor() as i64;
                    let tx = tx.rem_euclid(TEX as i64) as usize;
                    let ty = TEX - 1 - ty.rem_euclid(TEX as i64) as usize;
                    cells.push(ty * TEX + tx);
                }
            }
        }
    }
    Ok((points, cells))
}

fn transfer(
    high: &High,
    selected: &[Triangle],
    points: &[[f64; 3]],
    cells: &[usize],
    max_distance: f64,
) -> Result<(Vec<[f64; 4]>, Vec<bool>, Vec<f64>), TextureError> {
    if points.len() != cells.len() {
        return Err(TextureError::InvalidHeader);
    }
    let mut colors = vec![[0.0; 4]; TEX * TEX];
    let mut counts = vec![0usize; TEX * TEX];
    let mut suit_counts = vec![0usize; TEX * TEX];
    if selected.is_empty() || points.is_empty() {
        return Ok((colors, vec![false; TEX * TEX], vec![0.0; TEX * TEX]));
    }
    let mut mesh = high.mesh.clone();
    mesh.triangles = selected
        .iter()
        .map(|triangle| {
            high.mesh
                .triangles
                .get(triangle.mesh_triangle)
                .copied()
                .ok_or(TextureError::InvalidHeader)
        })
        .collect::<Result<Vec<_>, _>>()?;
    mesh.triangle_materials = selected
        .iter()
        .map(|triangle| triangle.material as u32)
        .collect();
    let mut max_extent = 0.0f64;
    for axis in 0..3 {
        let low = points
            .iter()
            .map(|point| point[axis])
            .fold(f64::INFINITY, f64::min);
        let high = points
            .iter()
            .map(|point| point[axis])
            .fold(f64::NEG_INFINITY, f64::max);
        max_extent = max_extent.max(high - low);
    }
    let extent_factor = if max_extent > 0.0 {
        max_distance / max_extent
    } else {
        0.0
    };
    let low = LowMesh {
        positions: points.to_vec(),
        uvs: Vec::new(),
        faces: Vec::new(),
    };
    let job = crate::geometry_bake::serialize_bake_job(
        &low,
        &mesh,
        &high.baked_materials,
        &high.textures,
        BakeOptions {
            width: 1,
            height: 1,
            margin: 0,
            flags: 67_108_864,
            extent_factor,
        },
        &[],
    )
    .map_err(|_| TextureError::InvalidHeader)?;
    let mapped = crate::textures_bake_core::run(&job).map_err(|_| TextureError::InvalidHeader)?;
    let record_bytes = points.len().checked_mul(16).ok_or(TextureError::TooLarge)?;
    if mapped.len() != record_bytes {
        return Err(TextureError::InvalidHeader);
    }
    for (index, record) in mapped.chunks_exact(16).enumerate() {
        let triangle_index = f32::from_le_bytes(record[..4].try_into().unwrap());
        if triangle_index < 0.0 {
            continue;
        }
        let triangle_index = triangle_index as usize;
        let triangle = selected
            .get(triangle_index)
            .ok_or(TextureError::InvalidHeader)?;
        let bary_b = f32::from_le_bytes(record[4..8].try_into().unwrap()) as f64;
        let bary_c = f32::from_le_bytes(record[8..12].try_into().unwrap()) as f64;
        let bary_a = 1.0 - bary_b - bary_c;
        let uv = [
            triangle.uvs[0][0] * bary_a + triangle.uvs[1][0] * bary_b + triangle.uvs[2][0] * bary_c,
            triangle.uvs[0][1] * bary_a + triangle.uvs[1][1] * bary_b + triangle.uvs[2][1] * bary_c,
        ];
        let Some(color) = sample(high, triangle.material, uv) else {
            continue;
        };
        let cell = *cells.get(index).ok_or(TextureError::InvalidHeader)?;
        for channel in 0..4 {
            colors[cell][channel] += color[channel];
        }
        counts[cell] += 1;
        if matches!(
            high.materials[triangle.material].name.as_str(),
            "Body1_v" | "Cap_v" | "FurCollar_v"
        ) {
            suit_counts[cell] += 1;
        }
    }
    let mut hit = vec![false; TEX * TEX];
    let mut suit = vec![0.0; TEX * TEX];
    for pixel in 0..TEX * TEX {
        if counts[pixel] > 0 {
            hit[pixel] = true;
            for channel in 0..4 {
                colors[pixel][channel] /= counts[pixel] as f64;
            }
            suit[pixel] = suit_counts[pixel] as f64 / counts[pixel] as f64;
        }
    }
    Ok((colors, hit, suit))
}

fn is_antenna(high: &High, triangle: &Triangle) -> bool {
    high.materials[triangle.material].name == "Cap_v"
        && (triangle.uvs[0][0] + triangle.uvs[1][0] + triangle.uvs[2][0]) / 3.0 < 0.45
}

fn suit_color(rgb: [f64; 3], color: &str) -> bool {
    let maximum = rgb[0].max(rgb[1]).max(rgb[2]);
    let minimum = rgb[0].min(rgb[1]).min(rgb[2]);
    let saturation = if maximum > 0.0 {
        (maximum - minimum) / maximum.max(1.0e-6)
    } else {
        0.0
    };
    if color == "red" {
        saturation > 0.5 && rgb[0] == maximum && rgb[0] > 0.35 && (rgb[1] - rgb[2]).abs() < 0.12
    } else {
        saturation > 0.4 && rgb[2] == maximum && rgb[2] > rgb[0] + 0.1
    }
}

fn float_byte(value: f64) -> u8 {
    ((value * 255.0 + 0.5).floor() as i32).clamp(0, 255) as u8
}

fn part_tile(high: &High, triangles: &[Triangle], tile: Tile) -> Result<Vec<u8>, TextureError> {
    let point_count = triangles
        .len()
        .checked_mul(3)
        .ok_or(TextureError::TooLarge)?;
    if point_count == 0 {
        return Ok(vec![0; tile.width * tile.height * 4]);
    }
    let mut center = [0.0; 3];
    for triangle in triangles {
        for point in triangle.positions {
            for axis in 0..3 {
                center[axis] += point[axis];
            }
        }
    }
    for value in &mut center {
        *value /= point_count as f64;
    }
    let mut covariance = [[0.0; 3]; 3];
    for triangle in triangles {
        for point in triangle.positions {
            let delta = [
                point[0] - center[0],
                point[1] - center[1],
                point[2] - center[2],
            ];
            for row in 0..3 {
                for col in 0..3 {
                    covariance[row][col] += delta[row] * delta[col];
                }
            }
        }
    }
    let eigenvectors = eigen3(covariance);
    let mut axis_x = eigenvectors[0].map(|value| value * tile.signs[0]);
    let normal = if let Some(normal) = tile.normal {
        normalize(normal)
    } else {
        eigenvectors[2].map(|value| value * tile.signs[1])
    };
    let projected = dot(axis_x, normal);
    for axis in 0..3 {
        axis_x[axis] -= normal[axis] * projected;
    }
    axis_x = normalize(axis_x);
    let axis_y = cross(normal, axis_x);
    let to2 = |point: [f64; 3]| -> [f64; 2] {
        let delta = [
            point[0] - center[0],
            point[1] - center[1],
            point[2] - center[2],
        ];
        [dot(delta, axis_x), dot(delta, axis_y)]
    };
    let projected_points = triangles
        .iter()
        .flat_map(|triangle| triangle.positions.map(to2))
        .collect::<Vec<_>>();
    let hull = convex_hull(&projected_points);
    let mut triangle = enclosing_triangle(&hull)?;
    let centroid = [
        (triangle[0][0] + triangle[1][0] + triangle[2][0]) / 3.0,
        (triangle[0][1] + triangle[1][1] + triangle[2][1]) / 3.0,
    ];
    for point in &mut triangle {
        point[0] = centroid[0] + (point[0] - centroid[0]) * 1.04;
        point[1] = centroid[1] + (point[1] - centroid[1]) * 1.04;
    }
    let min = [
        triangle.iter().map(|p| p[0]).fold(f64::INFINITY, f64::min),
        triangle.iter().map(|p| p[1]).fold(f64::INFINITY, f64::min),
    ];
    let max = [
        triangle
            .iter()
            .map(|p| p[0])
            .fold(f64::NEG_INFINITY, f64::max),
        triangle
            .iter()
            .map(|p| p[1])
            .fold(f64::NEG_INFINITY, f64::max),
    ];
    let sub = 6usize;
    let grid_width = tile.width * sub;
    let grid_height = tile.height * sub;
    let mut depth = vec![-1.0e18f64; grid_width * grid_height];
    let mut color = vec![[0.0; 4]; grid_width * grid_height];
    let x_scale = grid_width as f64 / (max[0] - min[0]);
    let y_scale = grid_height as f64 / (max[1] - min[1]);
    for triangle in triangles {
        let projected = triangle.positions.map(to2);
        let denominator = (projected[1][1] - projected[2][1]) * (projected[0][0] - projected[2][0])
            + (projected[2][0] - projected[1][0]) * (projected[0][1] - projected[2][1]);
        if denominator.abs() < 1.0e-12 {
            continue;
        }
        let normal_depth = triangle.positions.map(|point| dot(point, normal));
        let tri_min_x = projected
            .iter()
            .map(|point| point[0])
            .fold(f64::INFINITY, f64::min);
        let tri_max_x = projected
            .iter()
            .map(|point| point[0])
            .fold(f64::NEG_INFINITY, f64::max);
        let tri_min_y = projected
            .iter()
            .map(|point| point[1])
            .fold(f64::INFINITY, f64::min);
        let tri_max_y = projected
            .iter()
            .map(|point| point[1])
            .fold(f64::NEG_INFINITY, f64::max);
        // The native reference checks the whole supersample tile for every
        // triangle. Restricting to its projected bounds preserves the same
        // barycentric tests while avoiding billions of irrelevant pixels.
        let gx0 = ((((tri_min_x - min[0]) * x_scale - 0.5).floor() as isize - 1)
            .clamp(0, grid_width as isize - 1)) as usize;
        let gx1 = ((((tri_max_x - min[0]) * x_scale - 0.5).ceil() as isize + 1)
            .clamp(0, grid_width as isize - 1)) as usize;
        let gy0 = ((((max[1] - tri_max_y) * y_scale - 0.5).floor() as isize - 1)
            .clamp(0, grid_height as isize - 1)) as usize;
        let gy1 = ((((max[1] - tri_min_y) * y_scale - 0.5).ceil() as isize + 1)
            .clamp(0, grid_height as isize - 1)) as usize;
        for gy in gy0..=gy1 {
            let y = max[1] - (gy as f64 + 0.5) / grid_height as f64 * (max[1] - min[1]);
            for gx in gx0..=gx1 {
                let x = min[0] + (gx as f64 + 0.5) / grid_width as f64 * (max[0] - min[0]);
                let l0 = ((projected[1][1] - projected[2][1]) * (x - projected[2][0])
                    + (projected[2][0] - projected[1][0]) * (y - projected[2][1]))
                    / denominator;
                let l1 = ((projected[2][1] - projected[0][1]) * (x - projected[2][0])
                    + (projected[0][0] - projected[2][0]) * (y - projected[2][1]))
                    / denominator;
                let l2 = 1.0 - l0 - l1;
                if l0 < 0.0 || l1 < 0.0 || l2 < 0.0 {
                    continue;
                }
                let z = l0 * normal_depth[0] + l1 * normal_depth[1] + l2 * normal_depth[2];
                let index = gy * grid_width + gx;
                if z <= depth[index] {
                    continue;
                }
                let uv = [
                    triangle.uvs[0][0] * l0 + triangle.uvs[1][0] * l1 + triangle.uvs[2][0] * l2,
                    triangle.uvs[0][1] * l0 + triangle.uvs[1][1] * l1 + triangle.uvs[2][1] * l2,
                ];
                let rgba = sample(high, triangle.material, uv).unwrap_or([1.0; 4]);
                if tile.alpha_cut > 0.0 && rgba[3] < tile.alpha_cut {
                    continue;
                }
                color[index] = rgba;
                depth[index] = z;
            }
        }
    }
    let mut output = vec![0; tile.width * tile.height * 4];
    for y in 0..tile.height {
        for x in 0..tile.width {
            let mut sum = [0.0; 3];
            let mut count = 0usize;
            for sy in 0..sub {
                for sx in 0..sub {
                    let index = (y * sub + sy) * grid_width + x * sub + sx;
                    if depth[index] > -1.0e17 {
                        for channel in 0..3 {
                            sum[channel] += color[index][channel];
                        }
                        count += 1;
                    }
                }
            }
            let coverage = count as f64 / (sub * sub) as f64;
            let destination = (y * tile.width + x) * 4;
            if count > 0 {
                for channel in 0..3 {
                    output[destination + channel] = ((sum[channel] / count as f64 * 255.0 + 0.5)
                        .floor() as i32)
                        .clamp(0, 255) as u8;
                }
            }
            output[destination + 3] =
                u8::from(coverage >= 0.5 || (tile.thicken && count > 0)) * 255;
        }
    }
    Ok(output)
}

fn sample(high: &High, material_index: usize, uv: [f64; 2]) -> Option<[f64; 4]> {
    let material = high.materials.get(material_index)?;
    let texture_index = usize::try_from(material.texture).ok()?;
    let texture = high.textures.get(texture_index)?;
    let width = texture.width as usize;
    let height = texture.height as usize;
    if width == 0 || height == 0 {
        return None;
    }
    let wrap_mode = if material.wrap_s == material.wrap_t {
        material.wrap_s
    } else {
        0
    };
    let wrap = |value: f64| match wrap_mode {
        2 => {
            let value = value.rem_euclid(2.0);
            if value > 1.0 {
                2.0 - value
            } else {
                value
            }
        }
        0 => value.clamp(0.0, 1.0),
        _ => value.rem_euclid(1.0),
    };
    let x = ((wrap(uv[0]) * width as f64) as usize).min(width - 1);
    let y = ((wrap(uv[1]) * height as f64) as usize).min(height - 1);
    let offset = ((height - 1 - y) * width + x) * 4;
    let rgba = texture.rgba.get(offset..offset + 4)?;
    Some(std::array::from_fn(|channel| rgba[channel] as f64 / 255.0))
}

fn eigen3(matrix: [[f64; 3]; 3]) -> [[f64; 3]; 3] {
    let mut a = matrix;
    let mut vectors = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
    for _ in 0..100 {
        let off = a[0][1].powi(2) + a[0][2].powi(2) + a[1][2].powi(2);
        if off < 1.0e-30 {
            break;
        }
        for (p, q) in [(0, 1), (0, 2), (1, 2)] {
            if a[p][q].abs() < 1.0e-300 {
                continue;
            }
            let theta = (a[q][q] - a[p][p]) / (2.0 * a[p][q]);
            let t = theta.signum() / (theta.abs() + (theta * theta + 1.0).sqrt());
            let c = 1.0 / (t * t + 1.0).sqrt();
            let s = t * c;
            for row in &mut a {
                let ap = row[p];
                let aq = row[q];
                row[p] = c * ap - s * aq;
                row[q] = s * ap + c * aq;
            }
            for column in 0..3 {
                let ap = a[p][column];
                let aq = a[q][column];
                a[p][column] = c * ap - s * aq;
                a[q][column] = s * ap + c * aq;
            }
            for row in &mut vectors {
                let vp = row[p];
                let vq = row[q];
                row[p] = c * vp - s * vq;
                row[q] = s * vp + c * vq;
            }
        }
    }
    let mut order = [(a[0][0], 0usize), (a[1][1], 1), (a[2][2], 2)];
    order.sort_by(|left, right| right.0.total_cmp(&left.0));
    std::array::from_fn(|index| {
        let column = order[index].1;
        std::array::from_fn(|row| vectors[row][column])
    })
}

fn normalize(vector: [f64; 3]) -> [f64; 3] {
    let length = dot(vector, vector).sqrt();
    [vector[0] / length, vector[1] / length, vector[2] / length]
}

fn dot(left: [f64; 3], right: [f64; 3]) -> f64 {
    left[0] * right[0] + left[1] * right[1] + left[2] * right[2]
}

fn cross(left: [f64; 3], right: [f64; 3]) -> [f64; 3] {
    [
        left[1] * right[2] - left[2] * right[1],
        left[2] * right[0] - left[0] * right[2],
        left[0] * right[1] - left[1] * right[0],
    ]
}

fn round6(value: f64) -> f64 {
    let scaled = value * 1_000_000.0;
    let floor = scaled.floor();
    let fraction = scaled - floor;
    let rounded = if fraction > 0.5 || (fraction == 0.5 && floor as i64 % 2 != 0) {
        floor + 1.0
    } else {
        floor
    };
    rounded / 1_000_000.0
}

fn convex_hull(points: &[[f64; 2]]) -> Vec<[f64; 2]> {
    let mut sorted = points
        .iter()
        .map(|point| [round6(point[0]), round6(point[1])])
        .collect::<Vec<_>>();
    sorted.sort_by(|a, b| a[0].total_cmp(&b[0]).then(a[1].total_cmp(&b[1])));
    sorted.dedup_by(|a, b| a[0] == b[0] && a[1] == b[1]);
    if sorted.len() < 3 {
        return sorted;
    }
    let turn = |o: [f64; 2], a: [f64; 2], b: [f64; 2]| {
        (a[0] - o[0]) * (b[1] - o[1]) - (a[1] - o[1]) * (b[0] - o[0])
    };
    let mut lower = Vec::new();
    for &point in &sorted {
        while lower.len() >= 2 && turn(lower[lower.len() - 2], lower[lower.len() - 1], point) <= 0.0
        {
            lower.pop();
        }
        lower.push(point);
    }
    let mut upper = Vec::new();
    for &point in sorted.iter().rev() {
        while upper.len() >= 2 && turn(upper[upper.len() - 2], upper[upper.len() - 1], point) <= 0.0
        {
            upper.pop();
        }
        upper.push(point);
    }
    lower.pop();
    upper.pop();
    lower.extend(upper);
    lower
}

fn sincos(value: f64) -> (f64, f64) {
    const HALF_PI: f64 = std::f64::consts::FRAC_PI_2;
    let rounded_quadrants = (value / HALF_PI + 0.5).floor();
    let quadrant = rounded_quadrants as i32 & 3;
    let reduced = value - rounded_quadrants * HALF_PI;
    let square = reduced * reduced;
    let mut term = reduced;
    let mut sine = reduced;
    let mut k = 1;
    while k < 23 {
        term = -term * square / ((k + 1) * (k + 2)) as f64;
        sine += term;
        k += 2;
    }
    let mut term = 1.0;
    let mut cosine = 1.0;
    let mut k = 0;
    while k < 22 {
        term = -term * square / ((k + 1) * (k + 2)) as f64;
        cosine += term;
        k += 2;
    }
    match quadrant {
        0 => (sine, cosine),
        1 => (cosine, -sine),
        2 => (-sine, -cosine),
        _ => (-cosine, sine),
    }
}

fn enclosing_triangle(hull: &[[f64; 2]]) -> Result<[[f64; 2]; 3], TextureError> {
    if hull.len() < 3 {
        return Err(TextureError::InvalidHeader);
    }
    let mut normals = Vec::<[f64; 2]>::new();
    for index in 0..hull.len() {
        let edge = [
            hull[(index + 1) % hull.len()][0] - hull[index][0],
            hull[(index + 1) % hull.len()][1] - hull[index][1],
        ];
        let length = edge[0].hypot(edge[1]);
        if length > 1.0e-9 {
            normals.push([edge[1] / length, -edge[0] / length]);
        }
    }
    for index in 0..72 {
        let (sine, cosine) = sincos(index as f64 * std::f64::consts::PI / 36.0);
        normals.push([cosine, sine]);
    }
    let distance = normals
        .iter()
        .map(|normal| {
            hull.iter()
                .map(|point| point[0] * normal[0] + point[1] * normal[1])
                .fold(f64::NEG_INFINITY, f64::max)
        })
        .collect::<Vec<_>>();
    let solve = |a: usize, b: usize| -> [f64; 2] {
        let determinant = normals[a][0] * normals[b][1] - normals[a][1] * normals[b][0];
        [
            (distance[a] * normals[b][1] - normals[a][1] * distance[b]) / determinant,
            (normals[a][0] * distance[b] - distance[a] * normals[b][0]) / determinant,
        ]
    };
    let mut best = None;
    let mut best_area = 1.0e18;
    for i in 0..normals.len() {
        for j in i + 1..normals.len() {
            let cij = normals[i][0] * normals[j][1] - normals[i][1] * normals[j][0];
            if cij.abs() < 1.0e-6 {
                continue;
            }
            for k in j + 1..normals.len() {
                let cjk = normals[j][0] * normals[k][1] - normals[j][1] * normals[k][0];
                let cki = normals[k][0] * normals[i][1] - normals[k][1] * normals[i][0];
                let same_sign = (cij > 1.0e-6 && cjk > 1.0e-6 && cki > 1.0e-6)
                    || (cij < -1.0e-6 && cjk < -1.0e-6 && cki < -1.0e-6);
                if !same_sign {
                    continue;
                }
                let candidate = [solve(i, j), solve(j, k), solve(k, i)];
                let area = ((candidate[1][0] - candidate[0][0])
                    * (candidate[2][1] - candidate[0][1])
                    - (candidate[1][1] - candidate[0][1]) * (candidate[2][0] - candidate[0][0]))
                    .abs()
                    / 2.0;
                if area < best_area {
                    best = Some(candidate);
                    best_area = area;
                }
            }
        }
    }
    best.ok_or(TextureError::InvalidHeader)
}

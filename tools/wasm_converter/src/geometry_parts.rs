//! High-mesh part grouping used by part-ID texture recipes.
//!
//! This ports `textures_bake_parts.part_ids`; the returned IDs are appended to
//! the TBK1 job for the C rasterizer's `FLAG_PART_IDS` path.

use crate::geometry_bake::{GeometryError, HighMesh};
use crate::j3d_model::J3dModel;
use std::collections::{BTreeSet, HashMap};

#[derive(Debug)]
struct Part {
    root: usize,
    joint: usize,
    triangles: Vec<usize>,
}

pub fn part_ids(model: &J3dModel, high: &HighMesh, kmax: usize) -> Result<Vec<i32>, GeometryError> {
    let triangle_count = high.triangles.len();
    if triangle_count == 0 {
        return Ok(Vec::new());
    }
    if high.draw_matrices.len() != high.positions.len() || kmax == 0 {
        return Err(GeometryError::InvalidVertex);
    }

    let mut dominant = Vec::with_capacity(high.draw_matrices.len());
    for &draw in &high.draw_matrices {
        let influences = model.influences(draw).ok_or(GeometryError::InvalidVertex)?;
        let best = influences
            .into_iter()
            .max_by(|&(joint_a, weight_a), &(joint_b, weight_b)| {
                weight_a
                    .total_cmp(&weight_b)
                    .then_with(|| joint_b.cmp(&joint_a))
            })
            .ok_or(GeometryError::InvalidVertex)?;
        dominant.push(best.0);
    }

    let used = high
        .triangles
        .iter()
        .flatten()
        .copied()
        .collect::<BTreeSet<_>>();
    let mut lo = [f64::INFINITY; 3];
    let mut hi = [f64::NEG_INFINITY; 3];
    for &index in &used {
        let point = *high
            .positions
            .get(index as usize)
            .ok_or(GeometryError::InvalidVertex)?;
        for axis in 0..3 {
            lo[axis] = lo[axis].min(point[axis]);
            hi[axis] = hi[axis].max(point[axis]);
        }
    }
    let diagonal = ((0..3)
        .map(|axis| (hi[axis] - lo[axis]).powi(2))
        .sum::<f64>())
    .sqrt();
    let epsilon = (diagonal * 1e-4).max(1e-3);

    let mut welded_keys = HashMap::<[i64; 3], usize>::new();
    let mut welded = Vec::with_capacity(high.positions.len());
    for point in &high.positions {
        let key = [
            round_half_even(point[0] / epsilon)?,
            round_half_even(point[1] / epsilon)?,
            round_half_even(point[2] / epsilon)?,
        ];
        let next = welded_keys.len();
        welded.push(*welded_keys.entry(key).or_insert(next));
    }

    let mut parent = (0..welded_keys.len()).collect::<Vec<_>>();
    let mut areas = Vec::with_capacity(triangle_count);
    let mut centers = Vec::with_capacity(triangle_count);
    for &[a, b, c] in &high.triangles {
        let wa = *welded.get(a as usize).ok_or(GeometryError::InvalidVertex)?;
        let wb = *welded.get(b as usize).ok_or(GeometryError::InvalidVertex)?;
        let wc = *welded.get(c as usize).ok_or(GeometryError::InvalidVertex)?;
        let ra = root(&mut parent, wa);
        let rb = root(&mut parent, wb);
        parent[rb] = ra;
        let rc = root(&mut parent, wc);
        let component_root = root(&mut parent, ra);
        parent[rc] = component_root;

        let pa = high.positions[a as usize];
        let pb = high.positions[b as usize];
        let pc = high.positions[c as usize];
        let u = [pb[0] - pa[0], pb[1] - pa[1], pb[2] - pa[2]];
        let v = [pc[0] - pa[0], pc[1] - pa[1], pc[2] - pa[2]];
        let cross = [
            u[1] * v[2] - u[2] * v[1],
            u[2] * v[0] - u[0] * v[2],
            u[0] * v[1] - u[1] * v[0],
        ];
        areas.push(0.5 * (cross[0] * cross[0] + cross[1] * cross[1] + cross[2] * cross[2]).sqrt());
        centers.push([
            (pa[0] + pb[0] + pc[0]) / 3.0,
            (pa[1] + pb[1] + pc[1]) / 3.0,
            (pa[2] + pb[2] + pc[2]) / 3.0,
        ]);
    }

    let mut parts = Vec::<Part>::new();
    for (triangle_index, triangle) in high.triangles.iter().enumerate() {
        let [a, b, c] = *triangle;
        let joints = [
            dominant[a as usize],
            dominant[b as usize],
            dominant[c as usize],
        ];
        let mut joint = joints[0];
        let mut count = 0;
        for candidate in joints {
            let n = joints.iter().filter(|&&value| value == candidate).count();
            if n > count || (n == count && candidate < joint) {
                joint = candidate;
                count = n;
            }
        }
        let component = root(&mut parent, welded[a as usize]);
        if let Some(part) = parts
            .iter_mut()
            .find(|part| part.root == component && part.joint == joint)
        {
            part.triangles.push(triangle_index);
        } else {
            parts.push(Part {
                root: component,
                joint,
                triangles: vec![triangle_index],
            });
        }
    }

    let minimum_area = areas.iter().sum::<f64>() * 0.002;
    loop {
        let mut order = (0..parts.len()).collect::<Vec<_>>();
        order.sort_by(|&a, &b| {
            part_area(&parts[a], &areas).total_cmp(&part_area(&parts[b], &areas))
        });
        let mut merge = None;
        for index in order {
            if parts.len() <= 1 {
                break;
            }
            let area = part_area(&parts[index], &areas);
            if area >= minimum_area && parts[index].triangles.len() >= 4 {
                continue;
            }
            let mine = vertices(&parts[index], &high.triangles, &welded);
            let mut best = None;
            let mut greatest_overlap = 0usize;
            for (other_index, other) in parts.iter().enumerate() {
                if other_index == index || other.root != parts[index].root {
                    continue;
                }
                let other_vertices = vertices(other, &high.triangles, &welded);
                let overlap = mine.intersection(&other_vertices).count();
                if overlap > greatest_overlap {
                    best = Some(other_index);
                    greatest_overlap = overlap;
                }
            }
            if best.is_none() || greatest_overlap == 0 {
                if area >= minimum_area * 0.25 {
                    continue;
                }
                let center = part_center(&parts[index], &centers);
                best = parts
                    .iter()
                    .enumerate()
                    .filter(|(i, _)| *i != index)
                    .min_by(|(_, a), (_, b)| {
                        distance(part_center(a, &centers), center)
                            .total_cmp(&distance(part_center(b, &centers), center))
                    })
                    .map(|(i, _)| i);
            }
            if let Some(best) = best {
                merge = Some((index, best));
                break;
            }
        }
        let Some((from, to)) = merge else { break };
        let moved = parts.remove(from);
        parts[if to > from { to - 1 } else { to }]
            .triangles
            .extend(moved.triangles);
    }

    while parts.len() > kmax {
        let stats = parts
            .iter()
            .map(|part| part_bounds(part, &high.triangles, &high.positions))
            .collect::<Vec<_>>();
        let mut best = (f64::INFINITY, 0usize, 1usize);
        for i in 0..parts.len() {
            for j in i + 1..parts.len() {
                let mut score = distance(stats[i].0, stats[j].0) + 0.5 * (stats[i].1 + stats[j].1);
                if parts[i].joint != parts[j].joint {
                    score *= 2.0;
                }
                if score < best.0 {
                    best = (score, i, j);
                }
            }
        }
        let extra = parts.remove(best.2).triangles;
        parts[best.1].triangles.extend(extra);
    }

    let mut ids = vec![0i32; triangle_count];
    for (index, part) in parts.iter().enumerate() {
        let id = i32::try_from(index).map_err(|_| GeometryError::TooLarge)?;
        for &triangle in &part.triangles {
            ids[triangle] = id;
        }
    }
    Ok(ids)
}

fn round_half_even(value: f64) -> Result<i64, GeometryError> {
    if !value.is_finite() || value.abs() > i64::MAX as f64 {
        return Err(GeometryError::TooLarge);
    }
    let floor = value.floor();
    let fraction = value - floor;
    let rounded = if fraction < 0.5 {
        floor
    } else if fraction > 0.5 {
        floor + 1.0
    } else if (floor as i64) & 1 == 0 {
        floor
    } else {
        floor + 1.0
    };
    Ok(rounded as i64)
}

fn root(parent: &mut [usize], mut index: usize) -> usize {
    while parent[index] != index {
        parent[index] = parent[parent[index]];
        index = parent[index];
    }
    index
}

fn part_area(part: &Part, areas: &[f64]) -> f64 {
    part.triangles.iter().map(|&i| areas[i]).sum()
}

fn vertices(part: &Part, triangles: &[[u32; 3]], welded: &[usize]) -> BTreeSet<usize> {
    part.triangles
        .iter()
        .flat_map(|&i| triangles[i].iter().map(|&v| welded[v as usize]))
        .collect()
}

fn part_center(part: &Part, centers: &[[f64; 3]]) -> [f64; 3] {
    let mut center = [0.0; 3];
    for &triangle in &part.triangles {
        for axis in 0..3 {
            center[axis] += centers[triangle][axis];
        }
    }
    for value in &mut center {
        *value /= part.triangles.len() as f64;
    }
    center
}

fn distance(a: [f64; 3], b: [f64; 3]) -> f64 {
    ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)).sqrt()
}

fn part_bounds(part: &Part, triangles: &[[u32; 3]], positions: &[[f64; 3]]) -> ([f64; 3], f64) {
    let mut mean = [0.0; 3];
    let mut count = 0usize;
    let mut lo = [f64::INFINITY; 3];
    let mut hi = [f64::NEG_INFINITY; 3];
    for &triangle in &part.triangles {
        for &vertex in &triangles[triangle] {
            let point = positions[vertex as usize];
            count += 1;
            for axis in 0..3 {
                mean[axis] += point[axis];
                lo[axis] = lo[axis].min(point[axis]);
                hi[axis] = hi[axis].max(point[axis]);
            }
        }
    }
    for value in &mut mean {
        *value /= count as f64;
    }
    (mean, distance(hi, lo))
}

#[cfg(test)]
mod tests {
    use super::round_half_even;

    #[test]
    fn weld_rounding_matches_python_half_even() {
        assert_eq!(round_half_even(2.5), Ok(2));
        assert_eq!(round_half_even(3.5), Ok(4));
        assert_eq!(round_half_even(-2.5), Ok(-2));
        assert_eq!(round_half_even(-3.5), Ok(-4));
    }
}

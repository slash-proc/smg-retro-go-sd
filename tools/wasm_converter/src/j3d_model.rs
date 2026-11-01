//! Structural J3D model reader used by the Mario figure construction stage.

use std::collections::BTreeMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum J3dError {
    Truncated,
    InvalidHeader,
    InvalidSection,
    UnsupportedVertexType,
}

#[derive(Clone, Debug)]
pub struct Joint {
    pub scale_compensate: bool,
    pub scale: [f64; 3],
    pub rotation_raw: [i16; 3],
    pub rotation: [f64; 3],
    pub translation: [f64; 3],
}

#[derive(Clone, Debug)]
pub struct ShapeVertex {
    pub draw_matrix: usize,
    pub attributes: BTreeMap<u32, usize>,
}

#[derive(Clone, Debug)]
pub struct Shape {
    pub material: Option<usize>,
    pub vertices: Vec<ShapeVertex>,
    pub triangles: Vec<[usize; 3]>,
}

#[derive(Clone, Debug)]
pub struct J3dModel {
    pub matrix_calc: u16,
    stage_math: bool,
    pub sections: BTreeMap<[u8; 4], (usize, usize)>,
    pub names: Vec<String>,
    pub joints: Vec<Joint>,
    pub parent: Vec<Option<usize>>,
    pub children: Vec<Vec<usize>>,
    pub order: Vec<usize>,
    pub bind_j3d: Vec<[f64; 12]>,
    pub bind: Vec<[f64; 12]>,
    pub bind_inv: Vec<[f64; 12]>,
    pub rest_local: Vec<[f64; 12]>,
    pub draw_matrices: Vec<(bool, usize)>,
    pub envelopes: Vec<Vec<(usize, f64)>>,
    pub envelope_inverse_bind: Vec<Option<[f64; 12]>>,
    pub positions: Vec<[f64; 3]>,
    pub normals: BTreeMap<u32, Vec<[f64; 3]>>,
    pub colors: Vec<[u8; 4]>,
    pub uvs: Vec<[f64; 2]>,
    pub tex_coords: Vec<Vec<[f64; 2]>>,
    pub texture_names: Vec<String>,
    pub material_names: Vec<String>,
    pub material_texture_names: Vec<Option<String>>,
    /// Texture-coordinate array selected by each material (GX matrix source).
    pub material_uv_src: Vec<i8>,
    pub shape_material: Vec<Option<usize>>,
    pub shapes: Vec<Shape>,
}

const HALF_PI: f64 = 1.5707963267948966;

fn sin_poly(x: f64) -> f64 {
    let z = x * x;
    let mut sum = 0.0;
    let mut term = x;
    let mut k = 1;
    while k < 23 {
        sum += term;
        term = -term * z / ((k + 1) * (k + 2)) as f64;
        k += 2;
    }
    sum
}

fn cos_poly(x: f64) -> f64 {
    let z = x * x;
    let mut sum = 0.0;
    let mut term = 1.0;
    let mut k = 0;
    while k < 22 {
        sum += term;
        term = -term * z / ((k + 1) * (k + 2)) as f64;
        k += 2;
    }
    sum
}

pub(crate) fn sincos(x: f64) -> (f64, f64) {
    if x == 0.0 {
        return (0.0, 1.0);
    }
    // Preserve the operation order used by the native figure constructor;
    // exact point deduplication makes even a one-ULP difference observable.
    let k = (x / HALF_PI + 0.5).floor();
    let r = x - k * HALF_PI;
    let (s, c) = (sin_poly(r), cos_poly(r));
    match (k as i64) & 3 {
        0 => (s, c),
        1 => (c, -s),
        2 => (-s, -c),
        _ => (-c, s),
    }
}

pub(crate) fn mmul(a: &[f64; 12], b: &[f64; 12]) -> [f64; 12] {
    [
        a[0] * b[0] + a[1] * b[4] + a[2] * b[8],
        a[0] * b[1] + a[1] * b[5] + a[2] * b[9],
        a[0] * b[2] + a[1] * b[6] + a[2] * b[10],
        a[0] * b[3] + a[1] * b[7] + a[2] * b[11] + a[3],
        a[4] * b[0] + a[5] * b[4] + a[6] * b[8],
        a[4] * b[1] + a[5] * b[5] + a[6] * b[9],
        a[4] * b[2] + a[5] * b[6] + a[6] * b[10],
        a[4] * b[3] + a[5] * b[7] + a[6] * b[11] + a[7],
        a[8] * b[0] + a[9] * b[4] + a[10] * b[8],
        a[8] * b[1] + a[9] * b[5] + a[10] * b[9],
        a[8] * b[2] + a[9] * b[6] + a[10] * b[10],
        a[8] * b[3] + a[9] * b[7] + a[10] * b[11] + a[11],
    ]
}

pub(crate) fn minv(a: &[f64; 12]) -> [f64; 12] {
    let c00 = a[5] * a[10] - a[6] * a[9];
    let c01 = a[2] * a[9] - a[1] * a[10];
    let c02 = a[1] * a[6] - a[2] * a[5];
    let c10 = a[6] * a[8] - a[4] * a[10];
    let c11 = a[0] * a[10] - a[2] * a[8];
    let c12 = a[2] * a[4] - a[0] * a[6];
    let c20 = a[4] * a[9] - a[5] * a[8];
    let c21 = a[1] * a[8] - a[0] * a[9];
    let c22 = a[0] * a[5] - a[1] * a[4];
    let determinant = a[0] * c00 + a[1] * c10 + a[2] * c20;
    // Match the native figure builder's singular-matrix convention: use a
    // unit denominator while preserving the cofactors.
    let d = if determinant == 0.0 { 1.0 } else { determinant };
    let r = [
        c00 / d,
        c01 / d,
        c02 / d,
        c10 / d,
        c11 / d,
        c12 / d,
        c20 / d,
        c21 / d,
        c22 / d,
    ];
    [
        r[0],
        r[1],
        r[2],
        -(r[0] * a[3] + r[1] * a[7] + r[2] * a[11]),
        r[3],
        r[4],
        r[5],
        -(r[3] * a[3] + r[4] * a[7] + r[5] * a[11]),
        r[6],
        r[7],
        r[8],
        -(r[6] * a[3] + r[7] * a[7] + r[8] * a[11]),
    ]
}

pub(crate) fn trs_euler(
    t: [f64; 3],
    rot: [f64; 3],
    s: [f64; 3],
    inv_parent_scale: Option<[f64; 3]>,
) -> [f64; 12] {
    trs_euler_with(t, rot, s, inv_parent_scale, sincos)
}

fn trs_euler_stage(
    t: [f64; 3],
    rot: [f64; 3],
    s: [f64; 3],
    inv_parent_scale: Option<[f64; 3]>,
) -> [f64; 12] {
    trs_euler_with(t, rot, s, inv_parent_scale, |angle| {
        (
            crate::stage_galaxy::fd_sin(angle),
            crate::stage_galaxy::fd_cos(angle),
        )
    })
}

fn trs_euler_with(
    t: [f64; 3],
    rot: [f64; 3],
    s: [f64; 3],
    inv_parent_scale: Option<[f64; 3]>,
    trig: impl Fn(f64) -> (f64, f64),
) -> [f64; 12] {
    let (sx, cx) = trig(rot[0]);
    let (sy, cy) = trig(rot[1]);
    let (sz, cz) = trig(rot[2]);
    let e = [
        cz * cy,
        sx * cz * sy - cx * sz,
        cx * cz * sy + sx * sz,
        sz * cy,
        sx * sz * sy + cx * cz,
        cx * sz * sy - sx * cz,
        -sy,
        cy * sx,
        cy * cx,
    ];
    let mut m = [
        e[0] * s[0],
        e[1] * s[1],
        e[2] * s[2],
        t[0],
        e[3] * s[0],
        e[4] * s[1],
        e[5] * s[2],
        t[1],
        e[6] * s[0],
        e[7] * s[1],
        e[8] * s[2],
        t[2],
    ];
    if let Some(inv) = inv_parent_scale {
        for r in 0..3 {
            for c in 0..3 {
                m[4 * r + c] *= inv[r];
            }
        }
    }
    m
}

pub(crate) fn trs_quat(t: [f64; 3], rot: [f64; 3], s: [f64; 3]) -> [f64; 12] {
    let (sx, cx) = sincos(rot[0] * 0.5);
    let (sy, cy) = sincos(rot[1] * 0.5);
    let (sz, cz) = sincos(rot[2] * 0.5);
    let w = cz * cy * cx + sz * sy * sx;
    let x = cz * cy * sx - sz * sy * cx;
    let y = cz * sy * cx + sz * cy * sx;
    let z = sz * cy * cx - cz * sy * sx;
    let r = [
        1.0 - 2.0 * (y * y + z * z),
        2.0 * (x * y - z * w),
        2.0 * (x * z + y * w),
        2.0 * (x * y + z * w),
        1.0 - 2.0 * (x * x + z * z),
        2.0 * (y * z - x * w),
        2.0 * (x * z - y * w),
        2.0 * (y * z + x * w),
        1.0 - 2.0 * (x * x + y * y),
    ];
    [
        r[0] * s[0],
        r[1] * s[1],
        r[2] * s[2],
        t[0],
        r[3] * s[0],
        r[4] * s[1],
        r[5] * s[2],
        t[1],
        r[6] * s[0],
        r[7] * s[1],
        r[8] * s[2],
        t[2],
    ]
}

fn apply(a: &[f64; 12], p: [f64; 3]) -> [f64; 3] {
    [
        a[0] * p[0] + a[1] * p[1] + a[2] * p[2] + a[3],
        a[4] * p[0] + a[5] * p[1] + a[6] * p[2] + a[7],
        a[8] * p[0] + a[9] * p[1] + a[10] * p[2] + a[11],
    ]
}

fn bytes(data: &[u8], at: usize, size: usize) -> Result<&[u8], J3dError> {
    data.get(at..at.checked_add(size).ok_or(J3dError::Truncated)?)
        .ok_or(J3dError::Truncated)
}

fn u8_at(data: &[u8], at: usize) -> Result<u8, J3dError> {
    Ok(*bytes(data, at, 1)?.first().unwrap())
}
fn u16_at(data: &[u8], at: usize) -> Result<u16, J3dError> {
    Ok(u16::from_be_bytes(bytes(data, at, 2)?.try_into().unwrap()))
}
fn i16_at(data: &[u8], at: usize) -> Result<i16, J3dError> {
    Ok(i16::from_be_bytes(bytes(data, at, 2)?.try_into().unwrap()))
}
fn u32_at(data: &[u8], at: usize) -> Result<u32, J3dError> {
    Ok(u32::from_be_bytes(bytes(data, at, 4)?.try_into().unwrap()))
}
fn f32_at(data: &[u8], at: usize) -> Result<f64, J3dError> {
    Ok(f32::from_bits(u32_at(data, at)?) as f64)
}
fn add(a: usize, b: usize) -> Result<usize, J3dError> {
    a.checked_add(b).ok_or(J3dError::Truncated)
}

impl J3dModel {
    /// Restore models whose all-zero joint scales deliberately hide their mesh.
    pub fn unhide_zero_scale(&mut self) {
        let mut changed = false;
        for joint in &mut self.joints {
            if joint.scale.iter().all(|value| value.abs() < 1e-6) {
                joint.scale = [1.0; 3];
                changed = true;
            }
        }
        if changed {
            self.build_bind_transforms();
        }
    }

    pub fn parse(data: &[u8]) -> Result<Self, J3dError> {
        Self::parse_with_stage_math(data, false)
    }

    /// Parse a model with the fdlibm trigonometry used by the native stage
    /// asset constructor when creating its static bind pose.
    pub(crate) fn parse_stage(data: &[u8]) -> Result<Self, J3dError> {
        Self::parse_with_stage_math(data, true)
    }

    fn parse_with_stage_math(data: &[u8], stage_math: bool) -> Result<Self, J3dError> {
        if data.len() < 0x20 || data.get(..4) != Some(b"J3D2") && data.get(..4) != Some(b"J3D1") {
            return Err(J3dError::InvalidHeader);
        }
        let section_count = u32_at(data, 0x0c)? as usize;
        let mut sections = BTreeMap::new();
        let mut offset = 0x20usize;
        for _ in 0..section_count {
            let signature: [u8; 4] = bytes(data, offset, 4)?.try_into().unwrap();
            let size = u32_at(data, offset + 4)? as usize;
            if size < 8 || add(offset, size)? > data.len() {
                return Err(J3dError::InvalidSection);
            }
            sections.insert(signature, (offset, size));
            offset += size;
        }
        let mut model = Self {
            matrix_calc: 0,
            stage_math,
            sections,
            names: Vec::new(),
            joints: Vec::new(),
            parent: Vec::new(),
            children: Vec::new(),
            order: Vec::new(),
            bind_j3d: Vec::new(),
            bind: Vec::new(),
            bind_inv: Vec::new(),
            rest_local: Vec::new(),
            draw_matrices: Vec::new(),
            envelopes: Vec::new(),
            envelope_inverse_bind: Vec::new(),
            positions: Vec::new(),
            normals: BTreeMap::new(),
            colors: Vec::new(),
            uvs: Vec::new(),
            tex_coords: vec![Vec::new(); 8],
            texture_names: Vec::new(),
            material_names: Vec::new(),
            material_texture_names: Vec::new(),
            material_uv_src: Vec::new(),
            shape_material: Vec::new(),
            shapes: Vec::new(),
        };
        model.read_inf1(data)?;
        model.read_jnt1(data)?;
        model.read_drw1(data)?;
        model.read_evp1(data)?;
        model.read_vtx1(data)?;
        model.read_tex1(data)?;
        model.read_mat3_uv_sources(data)?;
        model.read_shp1(data)?;
        model.build_bind_transforms();
        Ok(model)
    }

    fn build_bind_transforms(&mut self) {
        let n = self.joints.len();
        let roots = (0..n)
            .filter(|&j| self.parent[j].is_none())
            .collect::<Vec<_>>();
        let mut stack = roots.into_iter().rev().collect::<Vec<_>>();
        self.order.clear();
        while let Some(joint) = stack.pop() {
            self.order.push(joint);
            stack.extend(self.children[joint].iter().rev().copied());
        }
        let seen = self
            .order
            .iter()
            .copied()
            .collect::<std::collections::BTreeSet<_>>();
        self.order.extend((0..n).filter(|j| !seen.contains(j)));

        self.bind_j3d = vec![[0.0; 12]; n];
        self.bind = vec![[0.0; 12]; n];
        for &j in &self.order {
            let joint = &self.joints[j];
            let parent = self.parent[j];
            let parent_scale = parent.map(|p| self.joints[p].scale);
            let inv_parent_scale = if self.matrix_calc == 2 && joint.scale_compensate {
                parent_scale.map(|scale| scale.map(|v| if v != 0.0 { 1.0 / v } else { 0.0 }))
            } else {
                None
            };
            let local_j3d = (if self.stage_math {
                trs_euler_stage
            } else {
                trs_euler
            })(
                joint.translation,
                joint.rotation,
                joint.scale,
                inv_parent_scale,
            );
            self.bind_j3d[j] = parent
                .map(|p| mmul(&self.bind_j3d[p], &local_j3d))
                .unwrap_or(local_j3d);

            let mut gltf_scale = joint.scale;
            for value in &mut gltf_scale {
                if value.abs() <= 1e-6 {
                    *value = 1.0;
                }
            }
            let local = trs_quat(joint.translation, joint.rotation, gltf_scale);
            self.bind[j] = parent.map(|p| mmul(&self.bind[p], &local)).unwrap_or(local);
        }
        self.bind_inv = self.bind.iter().map(minv).collect();
        self.rest_local = (0..n)
            .map(|j| match self.parent[j] {
                Some(p) => mmul(&self.bind_inv[p], &self.bind[j]),
                None => self.bind[j],
            })
            .collect();
    }

    pub fn draw_matrix(&self, draw_index: usize) -> Option<[f64; 12]> {
        let &(weighted, index) = self.draw_matrices.get(draw_index)?;
        if !weighted {
            return self.bind_j3d.get(index).copied();
        }
        let envelope = self.envelopes.get(index)?;
        let mut out = [0.0; 12];
        for &(joint, weight) in envelope {
            let bind = self.bind_j3d.get(joint)?;
            let inverse = self
                .envelope_inverse_bind
                .get(joint)
                .and_then(|m| *m)
                .unwrap_or_else(|| minv(bind));
            let matrix = mmul(bind, &inverse);
            for k in 0..12 {
                out[k] += weight * matrix[k];
            }
        }
        Some(out)
    }

    /// Resolve one shape draw matrix against fitted animation joint matrices.
    pub fn draw_matrix_with_pose(
        &self,
        draw_index: usize,
        pose: &[[f64; 12]],
    ) -> Option<[f64; 12]> {
        let &(weighted, index) = self.draw_matrices.get(draw_index)?;
        if !weighted {
            return pose.get(index).copied();
        }
        let envelope = self.envelopes.get(index)?;
        let mut out = [0.0; 12];
        for &(joint, weight) in envelope {
            let joint_pose = pose.get(joint)?;
            let bind = self.bind_j3d.get(joint)?;
            let inverse = self
                .envelope_inverse_bind
                .get(joint)
                .and_then(|m| *m)
                .unwrap_or_else(|| minv(bind));
            let matrix = mmul(joint_pose, &inverse);
            for k in 0..12 {
                out[k] += weight * matrix[k];
            }
        }
        Some(out)
    }

    pub fn anchor_point(&self, position_index: usize, draw_index: usize) -> Option<[f64; 3]> {
        let point = *self.positions.get(position_index)?;
        Some(apply(&self.draw_matrix(draw_index)?, point))
    }

    pub fn influences(&self, draw_index: usize) -> Option<Vec<(usize, f64)>> {
        let &(weighted, index) = self.draw_matrices.get(draw_index)?;
        if weighted {
            self.envelopes.get(index).cloned()
        } else {
            self.joints.get(index)?;
            Some(vec![(index, 1.0)])
        }
    }

    fn section(&self, name: &[u8; 4]) -> Result<(usize, usize), J3dError> {
        self.sections
            .get(name)
            .copied()
            .ok_or(J3dError::InvalidSection)
    }

    fn read_inf1(&mut self, data: &[u8]) -> Result<(), J3dError> {
        let (section, _) = self.section(b"INF1")?;
        self.matrix_calc = u16_at(data, section + 8)? & 0x0f;
        let mut cursor = add(section, u32_at(data, section + 0x14)? as usize)?;
        let mut stack: Vec<Option<(u16, u16)>> = Vec::new();
        let mut last = None;
        let mut joint_parent = BTreeMap::<usize, Option<usize>>::new();
        let mut shape_material = BTreeMap::<usize, usize>::new();
        loop {
            let kind = u16_at(data, cursor)?;
            let value = u16_at(data, cursor + 2)?;
            cursor += 4;
            if kind == 0 {
                break;
            }
            match kind {
                1 => stack.push(last),
                2 => {
                    stack.pop();
                }
                0x10 | 0x11 | 0x12 => {
                    if kind == 0x10 {
                        let parent = stack
                            .iter()
                            .rev()
                            .flatten()
                            .find(|(parent_kind, _)| *parent_kind == 0x10)
                            .map(|(_, parent)| *parent as usize);
                        joint_parent.insert(value as usize, parent);
                    } else if kind == 0x12 {
                        let material = stack
                            .iter()
                            .rev()
                            .flatten()
                            .find(|(parent_kind, _)| *parent_kind == 0x11)
                            .map(|(_, parent)| *parent)
                            .or_else(|| {
                                last.filter(|(parent_kind, _)| *parent_kind == 0x11)
                                    .map(|(_, parent)| parent)
                            });
                        if let Some(material) = material {
                            shape_material.insert(value as usize, material as usize);
                        }
                    }
                    last = Some((kind, value));
                }
                _ => {}
            }
        }
        // Temporarily store the sparse hierarchy in parent/children; JNT1
        // supplies the final skeleton size and validates node indices.
        let count = joint_parent.keys().copied().max().map_or(0, |x| x + 1);
        self.parent = vec![None; count];
        for (joint, parent) in joint_parent {
            self.parent[joint] = parent;
        }
        let count = shape_material.keys().max().map_or(0, |shape| shape + 1);
        self.shape_material = vec![None; count];
        for (shape, material) in shape_material {
            self.shape_material[shape] = Some(material);
        }
        Ok(())
    }

    fn read_jnt1(&mut self, data: &[u8]) -> Result<(), J3dError> {
        let (section, _) = self.section(b"JNT1")?;
        let count = u16_at(data, section + 8)? as usize;
        let data_base = add(section, u32_at(data, section + 0x0c)? as usize)?;
        let remap_base = add(section, u32_at(data, section + 0x10)? as usize)?;
        let name_base = add(section, u32_at(data, section + 0x14)? as usize)?;
        let name_count = u16_at(data, name_base)? as usize;
        let mut names = Vec::with_capacity(name_count);
        for i in 0..name_count {
            let relative = u16_at(data, name_base + 6 + i * 4)? as usize;
            let start = add(name_base, relative)?;
            let tail = data.get(start..).ok_or(J3dError::Truncated)?;
            let len = tail
                .iter()
                .position(|&b| b == 0)
                .ok_or(J3dError::Truncated)?;
            names.push(String::from_utf8_lossy(&tail[..len]).into_owned());
        }
        let mut joints = Vec::with_capacity(count);
        for i in 0..count {
            let remapped = u16_at(data, remap_base + i * 2)? as usize;
            let base = add(
                data_base,
                remapped.checked_mul(0x40).ok_or(J3dError::Truncated)?,
            )?;
            let scale_compensate = u8_at(data, base + 2)? == 1;
            let scale = [
                f32_at(data, base + 4)?,
                f32_at(data, base + 8)?,
                f32_at(data, base + 12)?,
            ];
            let raw = [
                i16_at(data, base + 0x10)?,
                i16_at(data, base + 0x12)?,
                i16_at(data, base + 0x14)?,
            ];
            let rotation = raw.map(|r| r as f64 * core::f64::consts::PI / 32768.0);
            let translation = [
                f32_at(data, base + 0x18)?,
                f32_at(data, base + 0x1c)?,
                f32_at(data, base + 0x20)?,
            ];
            joints.push(Joint {
                scale_compensate,
                scale,
                rotation_raw: raw,
                rotation,
                translation,
            });
        }
        if self.parent.len() > count {
            return Err(J3dError::InvalidSection);
        }
        self.parent.resize(count, None);
        self.children = vec![Vec::new(); count];
        for (joint, parent) in self.parent.iter().copied().enumerate() {
            if let Some(parent) = parent {
                if joint >= count || parent >= count {
                    return Err(J3dError::InvalidSection);
                }
                self.children[parent].push(joint);
            }
        }
        self.names = names;
        self.joints = joints;
        Ok(())
    }

    fn read_drw1(&mut self, data: &[u8]) -> Result<(), J3dError> {
        let Some(&(section, _)) = self.sections.get(b"DRW1") else {
            return Ok(());
        };
        let count = u16_at(data, section + 8)? as usize;
        let weight_base = add(section, u32_at(data, section + 0x0c)? as usize)?;
        let index_base = add(section, u32_at(data, section + 0x10)? as usize)?;
        self.draw_matrices = (0..count)
            .map(|i| {
                Ok((
                    u8_at(data, weight_base + i)? != 0,
                    u16_at(data, index_base + i * 2)? as usize,
                ))
            })
            .collect::<Result<_, J3dError>>()?;
        Ok(())
    }

    fn read_evp1(&mut self, data: &[u8]) -> Result<(), J3dError> {
        let Some(&(section, section_size)) = self.sections.get(b"EVP1") else {
            return Ok(());
        };
        let count = u16_at(data, section + 8)? as usize;
        let counts = add(section, u32_at(data, section + 0x0c)? as usize)?;
        let indices = add(section, u32_at(data, section + 0x10)? as usize)?;
        let weights = add(section, u32_at(data, section + 0x14)? as usize)?;
        let matrix_rel = u32_at(data, section + 0x18)? as usize;
        let mut item = 0usize;
        self.envelopes = Vec::with_capacity(count);
        for e in 0..count {
            let n = u8_at(data, counts + e)? as usize;
            let mut envelope = Vec::with_capacity(n);
            for k in 0..n {
                envelope.push((
                    u16_at(data, indices + (item + k) * 2)? as usize,
                    f32_at(data, weights + (item + k) * 4)?,
                ));
            }
            item += n;
            self.envelopes.push(envelope);
        }
        let max_joint = self
            .envelopes
            .iter()
            .flat_map(|e| e.iter().map(|(j, _)| *j))
            .max();
        self.envelope_inverse_bind = vec![None; max_joint.map_or(0, |j| j + 1)];
        if matrix_rel != 0 {
            for (joint, slot) in self.envelope_inverse_bind.iter_mut().enumerate() {
                let at = add(
                    section,
                    add(
                        matrix_rel,
                        joint.checked_mul(48).ok_or(J3dError::Truncated)?,
                    )?,
                )?;
                if at
                    .checked_add(48)
                    .is_none_or(|end| end > section + section_size)
                {
                    break;
                }
                let mut matrix = [0.0; 12];
                for (i, value) in matrix.iter_mut().enumerate() {
                    *value = f32_at(data, at + i * 4)?;
                }
                *slot = Some(matrix);
            }
        }
        Ok(())
    }

    fn read_vtx1(&mut self, data: &[u8]) -> Result<(), J3dError> {
        let (section, section_size) = self.section(b"VTX1")?;
        let formats = add(section, u32_at(data, section + 8)? as usize)?;
        let offsets = (0..13)
            .map(|i| u32_at(data, section + 0x0c + i * 4).map(|v| v as usize))
            .collect::<Result<Vec<_>, _>>()?;
        let mut declarations = BTreeMap::new();
        let mut p = formats;
        loop {
            let attr = u32_at(data, p)?;
            let components = u32_at(data, p + 4)?;
            let kind = u32_at(data, p + 8)?;
            let fraction = u8_at(data, p + 12)?;
            p = add(p, 16)?;
            if attr == 0xff {
                break;
            }
            declarations.insert(attr, (components, kind, fraction));
        }
        let mut sorted_offsets = offsets
            .iter()
            .copied()
            .filter(|&o| o != 0)
            .collect::<Vec<_>>();
        sorted_offsets.sort_unstable();
        for (&attribute, &(components, kind, fraction)) in &declarations {
            let slot = match attribute {
                9 => 0,
                10 => 1,
                25 => 2,
                11 => 3,
                12 => 4,
                13..=20 => 5 + (attribute - 13) as usize,
                _ => continue,
            };
            let start = offsets[slot];
            if start == 0 {
                continue;
            }
            let end = sorted_offsets
                .iter()
                .copied()
                .find(|&offset| offset > start)
                .unwrap_or(section_size);
            let raw = bytes(
                data,
                section + start,
                end.checked_sub(start).ok_or(J3dError::InvalidSection)?,
            )?;

            if attribute == 11 {
                let stride = match kind {
                    0 | 3 => 2,
                    1 | 4 => 3,
                    2 | 5 => 4,
                    _ => return Err(J3dError::UnsupportedVertexType),
                };
                for pixel in raw.chunks_exact(stride) {
                    let color = match kind {
                        0 => {
                            let value = u16::from_be_bytes([pixel[0], pixel[1]]);
                            [
                                (((value >> 11) & 31) * 255 / 31) as u8,
                                (((value >> 5) & 63) * 255 / 63) as u8,
                                ((value & 31) * 255 / 31) as u8,
                                255,
                            ]
                        }
                        1 => [pixel[0], pixel[1], pixel[2], 255],
                        2 => [pixel[0], pixel[1], pixel[2], pixel[3]],
                        3 => {
                            let value = u16::from_be_bytes([pixel[0], pixel[1]]);
                            [
                                (((value >> 12) & 15) * 17) as u8,
                                (((value >> 8) & 15) * 17) as u8,
                                (((value >> 4) & 15) * 17) as u8,
                                ((value & 15) * 17) as u8,
                            ]
                        }
                        4 => {
                            let value = ((pixel[0] as u32) << 16)
                                | ((pixel[1] as u32) << 8)
                                | pixel[2] as u32;
                            [
                                (((value >> 18) & 63) * 255 / 63) as u8,
                                (((value >> 12) & 63) * 255 / 63) as u8,
                                (((value >> 6) & 63) * 255 / 63) as u8,
                                ((value & 63) * 255 / 63) as u8,
                            ]
                        }
                        _ => [pixel[0], pixel[1], pixel[2], pixel[3]],
                    };
                    self.colors.push(color);
                }
                continue;
            }

            let component_count = match attribute {
                9 => {
                    if components == 1 {
                        3
                    } else {
                        2
                    }
                }
                10 | 25 => {
                    if components == 1 || attribute == 25 {
                        9
                    } else {
                        3
                    }
                }
                13..=20 => {
                    if components == 1 {
                        2
                    } else {
                        1
                    }
                }
                _ => continue,
            };
            let component_size = match kind {
                0 | 1 => 1,
                2 | 3 => 2,
                4 => 4,
                _ => return Err(J3dError::UnsupportedVertexType),
            };
            let stride = component_size * component_count;
            let divisor = if kind == 4 {
                1.0
            } else {
                2.0f64.powi(fraction as i32)
            };
            for vertex in raw.chunks_exact(stride) {
                let mut values = [0.0; 3];
                for component in 0..component_count.min(3) {
                    let at = component * component_size;
                    values[component] = match kind {
                        0 => u8_at(vertex, at)? as f64,
                        1 => (u8_at(vertex, at)? as i8) as f64,
                        2 => u16_at(vertex, at)? as f64,
                        3 => i16_at(vertex, at)? as f64,
                        4 => f32_at(vertex, at)?,
                        _ => return Err(J3dError::UnsupportedVertexType),
                    } / divisor;
                }
                match attribute {
                    9 => self.positions.push(values),
                    10 | 25 => self.normals.entry(attribute).or_default().push(values),
                    13..=20 => {
                        let uv = [values[0], values[1]];
                        self.tex_coords[(attribute - 13) as usize].push(uv);
                        if attribute == 13 {
                            self.uvs.push(uv);
                        }
                    }
                    _ => {}
                }
            }
        }
        Ok(())
    }

    fn read_tex1(&mut self, data: &[u8]) -> Result<(), J3dError> {
        let Some(&(section, section_size)) = self.sections.get(b"TEX1") else {
            return Ok(());
        };
        let relative = u32_at(data, section + 0x10)? as usize;
        if relative == 0 {
            return Ok(());
        }
        let table = add(section, relative)?;
        if table >= section + section_size {
            return Err(J3dError::InvalidSection);
        }
        let count = u16_at(data, table)? as usize;
        self.texture_names.reserve(count);
        for i in 0..count {
            let relative = u16_at(data, table + 6 + i * 4)? as usize;
            let start = add(table, relative)?;
            let tail = data
                .get(start..section + section_size)
                .ok_or(J3dError::Truncated)?;
            let len = tail
                .iter()
                .position(|&b| b == 0)
                .ok_or(J3dError::Truncated)?;
            let (name, _, _) = encoding_rs::SHIFT_JIS.decode(&tail[..len]);
            self.texture_names.push(name.into_owned());
        }
        Ok(())
    }

    fn read_mat3_uv_sources(&mut self, data: &[u8]) -> Result<(), J3dError> {
        let Some(&(section, section_size)) = self
            .sections
            .get(b"MAT3")
            .or_else(|| self.sections.get(b"MAT2"))
        else {
            return Ok(());
        };
        let count = u16_at(data, section + 8)? as usize;
        let tables = (0..30)
            .map(|index| {
                let offset = u32_at(data, section + 0x0c + index * 4)? as usize;
                if offset == 0 {
                    Ok(None)
                } else {
                    let address = add(section, offset)?;
                    if address >= section + section_size {
                        return Err(J3dError::InvalidSection);
                    }
                    Ok(Some(address))
                }
            })
            .collect::<Result<Vec<_>, J3dError>>()?;
        let init = tables[0].ok_or(J3dError::InvalidSection)?;
        let remap = tables[1].ok_or(J3dError::InvalidSection)?;
        let texgen = tables[11];
        let texremap = tables[15];
        let tevorder = tables[16];
        if let Some(name_base) = tables[2] {
            let name_count = u16_at(data, name_base)? as usize;
            self.material_names.reserve(name_count);
            for index in 0..name_count {
                let relative = u16_at(data, name_base + 6 + index * 4)? as usize;
                let start = add(name_base, relative)?;
                let tail = data
                    .get(start..section + section_size)
                    .ok_or(J3dError::Truncated)?;
                let length = tail
                    .iter()
                    .position(|&byte| byte == 0)
                    .ok_or(J3dError::Truncated)?;
                self.material_names.push(
                    encoding_rs::SHIFT_JIS
                        .decode_without_bom_handling(&tail[..length])
                        .0
                        .into_owned(),
                );
            }
        }
        self.material_uv_src.reserve(count);
        self.material_texture_names.reserve(count);
        for material in 0..count {
            let record = add(
                init,
                (u16_at(data, remap + material * 2)? as usize)
                    .checked_mul(0x14c)
                    .ok_or(J3dError::Truncated)?,
            )?;
            let mut textures = [None; 8];
            for (slot, texture) in textures.iter_mut().enumerate() {
                let remapped = u16_at(data, record + 0x84 + slot * 2)?;
                if remapped != u16::MAX {
                    if let Some(base) = texremap {
                        *texture = Some(u16_at(data, base + remapped as usize * 2)? as usize);
                    }
                }
            }
            let mut used = [None; 8];
            if let Some(base) = tevorder {
                for order in 0..16 {
                    let index = u16_at(data, record + 0xbc + order * 2)?;
                    if index == u16::MAX {
                        continue;
                    }
                    let item = add(base, index as usize * 4)?;
                    let coord = u8_at(data, item)?;
                    let slot = u8_at(data, item + 1)? as usize;
                    if slot < 8 && textures[slot].is_some() && used[slot].is_none() {
                        used[slot] = Some(coord);
                    }
                }
            }
            let first_slot = used
                .iter()
                .position(Option::is_some)
                .or_else(|| textures.iter().position(Option::is_some));
            let coord = first_slot
                .and_then(|slot| used[slot].map(usize::from))
                .or(first_slot);
            let texture_name = first_slot
                .and_then(|slot| textures[slot])
                .and_then(|texture| self.texture_names.get(texture))
                .cloned();
            let mut source = 0i8;
            if let (Some(coord), Some(base)) = (coord.filter(|&coord| coord < 8), texgen) {
                let index = u16_at(data, record + 0x28 + coord * 2)?;
                if index != u16::MAX {
                    let source_index = u8_at(data, base + index as usize * 4 + 1)?;
                    source = if (4..=11).contains(&source_index) {
                        (source_index - 4) as i8
                    } else {
                        -1
                    };
                }
            }
            self.material_uv_src.push(source);
            self.material_texture_names.push(texture_name);
        }
        Ok(())
    }

    fn read_shp1(&mut self, data: &[u8]) -> Result<(), J3dError> {
        let (section, _) = self.section(b"SHP1")?;
        let count = u16_at(data, section + 8)? as usize;
        let init = add(section, u32_at(data, section + 0x0c)? as usize)?;
        let remap = add(section, u32_at(data, section + 0x10)? as usize)?;
        let desc_base = add(section, u32_at(data, section + 0x18)? as usize)?;
        let matrix_table = add(section, u32_at(data, section + 0x1c)? as usize)?;
        let display_lists = add(section, u32_at(data, section + 0x20)? as usize)?;
        let matrix_init = add(section, u32_at(data, section + 0x24)? as usize)?;
        let draw_init = add(section, u32_at(data, section + 0x28)? as usize)?;
        self.shapes.reserve(count);

        for shape_index in 0..count {
            let shape = add(
                init,
                (u16_at(data, remap + shape_index * 2)? as usize) * 0x28,
            )?;
            let packet_count = u16_at(data, shape + 2)? as usize;
            let descriptor_offset = u16_at(data, shape + 4)? as usize;
            let first_matrix = u16_at(data, shape + 6)? as usize;
            let first_draw = u16_at(data, shape + 8)? as usize;

            let mut descriptor = Vec::new();
            let mut cursor = add(desc_base, descriptor_offset)?;
            loop {
                let attribute = u32_at(data, cursor)?;
                let kind = u32_at(data, cursor + 4)?;
                cursor = add(cursor, 8)?;
                if attribute == 0xff {
                    break;
                }
                descriptor.push((attribute, kind));
            }

            let mut vertices = Vec::new();
            let mut triangles = Vec::new();
            let mut slots = [u16::MAX; 10];
            for packet in 0..packet_count {
                let matrix = add(matrix_init, (first_matrix + packet) * 8)?;
                let use_matrix = u16_at(data, matrix)? as usize;
                let matrix_count = u16_at(data, matrix + 2)? as usize;
                let first_slot = u32_at(data, matrix + 4)? as usize;
                for slot in 0..matrix_count {
                    let value = u16_at(data, matrix_table + (first_slot + slot) * 2)?;
                    if slot < slots.len() && value != u16::MAX {
                        slots[slot] = value;
                    }
                }
                let default_matrix = if slots[0] != u16::MAX {
                    slots[0] as usize
                } else {
                    use_matrix
                };
                let draw = add(draw_init, (first_draw + packet) * 8)?;
                let list_size = u32_at(data, draw)? as usize;
                let list_offset = u32_at(data, draw + 4)? as usize;
                let list_start = add(display_lists, list_offset)?;
                let list_end = add(list_start, list_size)?;
                if list_end > data.len() {
                    return Err(J3dError::InvalidSection);
                }
                parse_display_list(
                    data,
                    list_start,
                    list_end,
                    &descriptor,
                    &slots,
                    default_matrix,
                    &mut vertices,
                    &mut triangles,
                )?;
            }
            self.shapes.push(Shape {
                material: self.shape_material.get(shape_index).copied().flatten(),
                vertices,
                triangles,
            });
        }
        Ok(())
    }
}

fn parse_display_list(
    data: &[u8],
    mut cursor: usize,
    end: usize,
    descriptor: &[(u32, u32)],
    slots: &[u16; 10],
    default_matrix: usize,
    vertices: &mut Vec<ShapeVertex>,
    triangles: &mut Vec<[usize; 3]>,
) -> Result<(), J3dError> {
    while cursor < end {
        let opcode = u8_at(data, cursor)?;
        cursor += 1;
        if opcode == 0 {
            continue;
        }
        let primitive = opcode & 0xf8;
        if !matches!(primitive, 0x80 | 0x90 | 0x98 | 0xa0 | 0xa8 | 0xb0 | 0xb8) {
            break;
        }
        let count = u16_at(data, cursor)? as usize;
        cursor += 2;
        let first_vertex = vertices.len();
        for _ in 0..count {
            let mut draw_matrix = default_matrix;
            let mut attributes = BTreeMap::new();
            for &(attribute, kind) in descriptor {
                let value = match kind {
                    1 | 2 => {
                        let result = u8_at(data, cursor)? as usize;
                        cursor += 1;
                        Some(result)
                    }
                    3 => {
                        let result = u16_at(data, cursor)? as usize;
                        cursor += 2;
                        Some(result)
                    }
                    _ => None,
                };
                let Some(value) = value else { continue };
                if attribute == 0 {
                    let slot = value / 3;
                    draw_matrix = slots
                        .get(slot)
                        .copied()
                        .filter(|&v| v != u16::MAX)
                        .map_or(default_matrix, |v| v as usize);
                } else if attribute > 8 {
                    attributes.insert(attribute, value);
                }
            }
            vertices.push(ShapeVertex {
                draw_matrix,
                attributes,
            });
        }
        match primitive {
            0x80 => {
                for i in (0..count.saturating_sub(3)).step_by(4) {
                    triangles.push([first_vertex + i, first_vertex + i + 1, first_vertex + i + 2]);
                    triangles.push([first_vertex + i, first_vertex + i + 2, first_vertex + i + 3]);
                }
            }
            0x90 => {
                for i in (0..count.saturating_sub(2)).step_by(3) {
                    triangles.push([first_vertex + i, first_vertex + i + 1, first_vertex + i + 2]);
                }
            }
            0x98 => {
                for i in 0..count.saturating_sub(2) {
                    triangles.push(if i & 1 == 0 {
                        [first_vertex + i, first_vertex + i + 1, first_vertex + i + 2]
                    } else {
                        [first_vertex + i + 1, first_vertex + i, first_vertex + i + 2]
                    });
                }
            }
            0xa0 => {
                for i in 1..count.saturating_sub(1) {
                    triangles.push([first_vertex, first_vertex + i, first_vertex + i + 1]);
                }
            }
            _ => {}
        }
        if cursor > end {
            return Err(J3dError::InvalidSection);
        }
    }
    Ok(())
}

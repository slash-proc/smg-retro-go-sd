//! J3D BCK animation sampling and skeleton pose/deformation.

use crate::j3d_model::{minv, mmul, sincos, trs_euler, trs_quat, J3dModel};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnimationError {
    Truncated,
    MissingAnimationSection,
    InvalidAnimationSection,
}

/// Sample material texture-number overrides from a J3D BTP animation.
/// Texture numbers refer to the model's TEX1 texture table.
pub fn btp_textures(
    data: &[u8],
    frame: usize,
) -> Result<Vec<(String, usize, usize)>, AnimationError> {
    let mut section = 0x20usize;
    let count = u32_at(data, 0x0c)? as usize;
    let mut track = None;
    for _ in 0..count {
        let size = u32_at(data, section + 4)? as usize;
        if size < 8 || add(section, size)? > data.len() {
            return Err(AnimationError::InvalidAnimationSection);
        }
        if bytes(data, section, 4)? == b"TPT1" {
            track = Some((section, size));
            break;
        }
        section = add(section, size)?;
    }
    let (o, section_size) = track.ok_or(AnimationError::MissingAnimationSection)?;
    let section_end = add(o, section_size)?;
    let entries_offset = u32_at(data, o + 0x10)? as usize;
    let indices_offset = u32_at(data, o + 0x14)? as usize;
    let names_offset = u32_at(data, o + 0x1c)? as usize;
    let entry_count = u16_at(data, o + 0x0c)? as usize;
    let entries = add(o, entries_offset)?;
    let names = add(o, names_offset)?;
    let index_base = add(o, indices_offset)?;
    let entries_end = add(
        entries,
        entry_count
            .checked_mul(8)
            .ok_or(AnimationError::Truncated)?,
    )?;
    if entries_end > section_end || add(names, 2)? > section_end {
        return Err(AnimationError::InvalidAnimationSection);
    }

    let name_count = u16_at(data, names)? as usize;
    let name_records_end = add(
        names,
        6usize
            .checked_add(name_count.checked_mul(4).ok_or(AnimationError::Truncated)?)
            .ok_or(AnimationError::Truncated)?,
    )?;
    if name_records_end > section_end || name_count < entry_count {
        return Err(AnimationError::InvalidAnimationSection);
    }
    let mut decoded_names = Vec::with_capacity(name_count);
    for index in 0..name_count {
        let relative = u16_at(data, names + 6 + index * 4)? as usize;
        let start = add(names, relative)?;
        let tail = data
            .get(start..section_end)
            .ok_or(AnimationError::Truncated)?;
        let end = tail
            .iter()
            .position(|&byte| byte == 0)
            .ok_or(AnimationError::InvalidAnimationSection)?;
        decoded_names.push(
            encoding_rs::SHIFT_JIS
                .decode_without_bom_handling(&tail[..end])
                .0
                .into_owned(),
        );
    }

    let mut result = Vec::with_capacity(entry_count);
    for index in 0..entry_count {
        let entry = entries + index * 8;
        let key_count = u16_at(data, entry)? as usize;
        let first = u16_at(data, entry + 2)? as usize;
        let slot = data[entry + 4] as usize;
        if key_count == 0 {
            return Err(AnimationError::InvalidAnimationSection);
        }
        let selected = first
            .checked_add(frame.min(key_count - 1))
            .ok_or(AnimationError::Truncated)?;
        let texture = u16_at(
            data,
            index_base
                .checked_add(selected.checked_mul(2).ok_or(AnimationError::Truncated)?)
                .ok_or(AnimationError::Truncated)?,
        )? as usize;
        result.push((decoded_names[index].clone(), slot, texture));
    }
    Ok(result)
}

/// Sample animated material color registers from a J3D BRK animation.
/// The returned register values retain their signed 1/255 scale, matching the
/// native texture baker's TEV register override path.
pub fn brk_registers(
    data: &[u8],
    frame: f64,
) -> Result<BTreeMap<String, BTreeMap<usize, [i16; 4]>>, AnimationError> {
    let mut section = 0x20usize;
    let count = u32_at(data, 0x0c)? as usize;
    let mut track = None;
    for _ in 0..count {
        let size = u32_at(data, section + 4)? as usize;
        if size < 8 || add(section, size)? > data.len() {
            return Err(AnimationError::InvalidAnimationSection);
        }
        if bytes(data, section, 4)? == b"TRK1" {
            track = Some((section, size));
            break;
        }
        section = add(section, size)?;
    }
    let (o, section_size) = track.ok_or(AnimationError::MissingAnimationSection)?;
    let nreg = u16_at(data, o + 0x0c)? as usize;
    let counts = (0..8)
        .map(|i| u16_at(data, o + 0x10 + i * 2).map(|v| v as usize))
        .collect::<Result<Vec<_>, _>>()?;
    let offsets = (0..14)
        .map(|i| u32_at(data, o + 0x20 + i * 4).map(|v| v as usize))
        .collect::<Result<Vec<_>, _>>()?;
    let section_end = add(o, section_size)?;
    let mut values = Vec::<Vec<i16>>::with_capacity(4);
    for channel in 0..4 {
        let start = add(o, offsets[6 + channel])?;
        let byte_len = counts[channel]
            .checked_mul(2)
            .ok_or(AnimationError::Truncated)?;
        if add(start, byte_len)? > section_end {
            return Err(AnimationError::InvalidAnimationSection);
        }
        values.push(
            (0..counts[channel])
                .map(|i| i16_at(data, start + i * 2))
                .collect::<Result<Vec<_>, _>>()?,
        );
    }
    let names = if nreg == 0 {
        Vec::new()
    } else {
        let base = add(o, offsets[4])?;
        let table_count = u16_at(data, base)? as usize;
        let mut names = Vec::with_capacity(table_count);
        for index in 0..table_count {
            let relative = u16_at(data, base + 6 + index * 4)? as usize;
            let start = add(base, relative)?;
            let tail = data
                .get(start..section_end)
                .ok_or(AnimationError::Truncated)?;
            let end = tail
                .iter()
                .position(|&byte| byte == 0)
                .ok_or(AnimationError::InvalidAnimationSection)?;
            names.push(
                encoding_rs::SHIFT_JIS
                    .decode_without_bom_handling(&tail[..end])
                    .0
                    .into_owned(),
            );
        }
        names
    };

    let entries = add(o, offsets[0])?;
    if add(
        entries,
        nreg.checked_mul(28).ok_or(AnimationError::Truncated)?,
    )? > section_end
    {
        return Err(AnimationError::InvalidAnimationSection);
    }
    let mut result = BTreeMap::<String, BTreeMap<usize, [i16; 4]>>::new();
    for index in 0..nreg {
        let entry = entries + index * 28;
        let mut rgba = [0i16; 4];
        for channel in 0..4 {
            let count = u16_at(data, entry + channel * 6)? as usize;
            let first = u16_at(data, entry + channel * 6 + 2)? as usize;
            let tangent = u16_at(data, entry + channel * 6 + 4)?;
            rgba[channel] = sample_color_track(&values[channel], count, first, tangent, frame)?;
        }
        let name = names
            .get(index)
            .cloned()
            .unwrap_or_else(|| index.to_string());
        result
            .entry(name)
            .or_default()
            .insert(data[entry + 24] as usize, rgba);
    }
    Ok(result)
}

fn sample_color_track(
    values: &[i16],
    count: usize,
    first: usize,
    tangent: u16,
    frame: f64,
) -> Result<i16, AnimationError> {
    if count == 0 {
        return Ok(0);
    }
    if count == 1 {
        return values
            .get(first)
            .copied()
            .ok_or(AnimationError::InvalidAnimationSection);
    }
    let stride = if tangent == 0 { 3 } else { 4 };
    let key = |index: usize| -> Result<(f64, f64, f64, f64), AnimationError> {
        let at = first
            .checked_add(index.checked_mul(stride).ok_or(AnimationError::Truncated)?)
            .ok_or(AnimationError::Truncated)?;
        let time = *values
            .get(at)
            .ok_or(AnimationError::InvalidAnimationSection)? as f64;
        let value = *values
            .get(at + 1)
            .ok_or(AnimationError::InvalidAnimationSection)? as f64;
        if stride == 3 {
            let slope = *values
                .get(at + 2)
                .ok_or(AnimationError::InvalidAnimationSection)? as f64;
            Ok((time, value, slope, slope))
        } else {
            let incoming = *values
                .get(at + 2)
                .ok_or(AnimationError::InvalidAnimationSection)? as f64;
            let outgoing = *values
                .get(at + 3)
                .ok_or(AnimationError::InvalidAnimationSection)? as f64;
            Ok((time, value, incoming, outgoing))
        }
    };
    let first_key = key(0)?;
    let last_key = key(count - 1)?;
    let sampled = if frame <= first_key.0 {
        first_key.1
    } else if frame >= last_key.0 {
        last_key.1
    } else {
        let mut output = last_key.1;
        for index in 0..count - 1 {
            let a = key(index)?;
            let b = key(index + 1)?;
            if a.0 <= frame && frame <= b.0 {
                let duration = b.0 - a.0;
                let u = if duration == 0.0 {
                    0.0
                } else {
                    (frame - a.0) / duration
                };
                let u2 = u * u;
                let u3 = u2 * u;
                output = (2.0 * u3 - 3.0 * u2 + 1.0) * a.1
                    + (u3 - 2.0 * u2 + u) * duration * a.3
                    + (-2.0 * u3 + 3.0 * u2) * b.1
                    + (u3 - u2) * duration * b.2;
                break;
            }
        }
        output
    };
    let floor = sampled.floor();
    let fraction = sampled - floor;
    let rounded = if fraction > 0.5 || (fraction == 0.5 && (floor as i64) % 2 != 0) {
        floor + 1.0
    } else {
        floor
    };
    Ok(rounded.clamp(-1024.0, 1023.0) as i16)
}

#[derive(Clone, Copy, Debug)]
struct Key {
    frame: f64,
    value: f64,
    incoming: f64,
    outgoing: f64,
}

#[derive(Clone, Debug)]
struct Track {
    keys: Option<Vec<Key>>,
    constant: f64,
}

impl Track {
    fn sample(&self, frame: f64) -> f64 {
        let Some(keys) = &self.keys else {
            return self.constant;
        };
        if frame <= keys[0].frame {
            return keys[0].value;
        }
        if frame >= keys[keys.len() - 1].frame {
            return keys[keys.len() - 1].value;
        }
        // Recipe conversion samples thousands of animation frames across every
        // joint. A linear scan here made long BCK tracks quadratic in practice.
        // Find the same last key at or before `frame` with an upper-bound search.
        let i = keys.partition_point(|key| key.frame <= frame) - 1;
        let a = keys[i];
        let b = keys[i + 1];
        let duration = b.frame - a.frame;
        if duration <= 0.0 {
            return b.value;
        }
        let t = (frame - a.frame) / duration;
        let t2 = t * t;
        let t3 = t2 * t;
        a.value * (2.0 * t3 - 3.0 * t2 + 1.0)
            + b.value * (-2.0 * t3 + 3.0 * t2)
            + a.outgoing * duration * (t3 - 2.0 * t2 + t)
            + b.incoming * duration * (t3 - t2)
    }
}

#[derive(Clone, Debug)]
struct JointTracks {
    scale: [Track; 3],
    rotation: [Track; 3],
    translation: [Track; 3],
}

#[derive(Clone, Debug)]
pub struct Bck {
    pub loop_mode: u8,
    pub frame_count: u16,
    pub joint_count: usize,
    tracks: Vec<JointTracks>,
}

#[derive(Clone, Copy, Debug)]
pub struct RawJoint {
    pub scale: [f64; 3],
    pub rotation: [f64; 3],
    pub translation: [f64; 3],
}

#[derive(Clone, Copy, Debug)]
pub struct Correction {
    pub before_local: [f64; 12],
    pub before_parent: [f64; 12],
    pub after_world: [f64; 12],
}

fn bytes(data: &[u8], at: usize, len: usize) -> Result<&[u8], AnimationError> {
    data.get(at..at.checked_add(len).ok_or(AnimationError::Truncated)?)
        .ok_or(AnimationError::Truncated)
}
fn u8_at(data: &[u8], at: usize) -> Result<u8, AnimationError> {
    Ok(bytes(data, at, 1)?[0])
}
fn u16_at(data: &[u8], at: usize) -> Result<u16, AnimationError> {
    Ok(u16::from_be_bytes(bytes(data, at, 2)?.try_into().unwrap()))
}
fn i16_at(data: &[u8], at: usize) -> Result<i16, AnimationError> {
    Ok(i16::from_be_bytes(bytes(data, at, 2)?.try_into().unwrap()))
}
fn u32_at(data: &[u8], at: usize) -> Result<u32, AnimationError> {
    Ok(u32::from_be_bytes(bytes(data, at, 4)?.try_into().unwrap()))
}
fn f32_at(data: &[u8], at: usize) -> Result<f64, AnimationError> {
    Ok(f32::from_bits(u32_at(data, at)?) as f64)
}
fn add(a: usize, b: usize) -> Result<usize, AnimationError> {
    a.checked_add(b).ok_or(AnimationError::Truncated)
}

impl Bck {
    pub fn parse(data: &[u8]) -> Result<Self, AnimationError> {
        let mut section = 0x20usize;
        let mut ank = None;
        while section
            .checked_add(8)
            .filter(|&end| end <= data.len())
            .is_some()
        {
            let size = u32_at(data, section + 4)? as usize;
            if size < 8 || add(section, size)? > data.len() {
                return Err(AnimationError::InvalidAnimationSection);
            }
            if bytes(data, section, 4)? == b"ANK1" {
                ank = Some((section, size));
                break;
            }
            section += size;
        }
        let (o, section_size) = ank.ok_or(AnimationError::MissingAnimationSection)?;
        let loop_mode = u8_at(data, o + 8)?;
        let rotation_shift = u8_at(data, o + 9)?;
        let frame_count = u16_at(data, o + 0x0a)?;
        let joint_count = u16_at(data, o + 0x0c)? as usize;
        let scale_count = u16_at(data, o + 0x0e)? as usize;
        let rotation_count = u16_at(data, o + 0x10)? as usize;
        let translation_count = u16_at(data, o + 0x12)? as usize;
        let joint_base = add(o, u32_at(data, o + 0x14)? as usize)?;
        let scale_base = add(o, u32_at(data, o + 0x18)? as usize)?;
        let rotation_base = add(o, u32_at(data, o + 0x1c)? as usize)?;
        let translation_base = add(o, u32_at(data, o + 0x20)? as usize)?;
        let scale_end = add(
            scale_base,
            scale_count
                .checked_mul(4)
                .ok_or(AnimationError::Truncated)?,
        )?;
        let rotation_end = add(
            rotation_base,
            rotation_count
                .checked_mul(2)
                .ok_or(AnimationError::Truncated)?,
        )?;
        let translation_end = add(
            translation_base,
            translation_count
                .checked_mul(4)
                .ok_or(AnimationError::Truncated)?,
        )?;
        let joint_end = add(
            joint_base,
            joint_count
                .checked_mul(54)
                .ok_or(AnimationError::Truncated)?,
        )?;
        let section_end = add(o, section_size)?;
        if [scale_end, rotation_end, translation_end, joint_end]
            .into_iter()
            .any(|end| end > section_end)
        {
            return Err(AnimationError::InvalidAnimationSection);
        }
        let scales = (0..scale_count)
            .map(|i| f32_at(data, scale_base + i * 4))
            .collect::<Result<Vec<_>, _>>()?;
        let rotations = (0..rotation_count)
            .map(|i| i16_at(data, rotation_base + i * 2).map(|x| x as f64))
            .collect::<Result<Vec<_>, _>>()?;
        let translations = (0..translation_count)
            .map(|i| f32_at(data, translation_base + i * 4))
            .collect::<Result<Vec<_>, _>>()?;
        let rotation_scale = ((1u64 << rotation_shift) as f64) * core::f64::consts::PI / 32768.0;
        let mut tracks = Vec::with_capacity(joint_count);
        for joint in 0..joint_count {
            let mut components: [[Track; 3]; 3] = std::array::from_fn(|_| {
                std::array::from_fn(|_| Track {
                    keys: None,
                    constant: 0.0,
                })
            });
            for (component, (base, data_len, scale, default)) in [
                (scale_base, scale_count, 1.0, 1.0),
                (rotation_base, rotation_count, rotation_scale, 0.0),
                (translation_base, translation_count, 1.0, 0.0),
            ]
            .into_iter()
            .enumerate()
            {
                let values: &[f64] = match component {
                    0 => &scales,
                    1 => &rotations,
                    _ => &translations,
                };
                let _ = (base, data_len);
                for axis in 0..3 {
                    let channel = joint_base + joint * 54 + axis * 18 + component * 6;
                    let count = u16_at(data, channel)? as usize;
                    let index = u16_at(data, channel + 2)? as usize;
                    let mode = u16_at(data, channel + 4)?;
                    let track = if count == 0 {
                        Track {
                            keys: None,
                            constant: default,
                        }
                    } else if count == 1 {
                        let value = *values
                            .get(index)
                            .ok_or(AnimationError::InvalidAnimationSection)?;
                        Track {
                            keys: None,
                            constant: value * scale,
                        }
                    } else {
                        let stride = if mode == 1 { 4 } else { 3 };
                        let end = index
                            .checked_add(
                                count.checked_mul(stride).ok_or(AnimationError::Truncated)?,
                            )
                            .ok_or(AnimationError::Truncated)?;
                        if end > values.len() {
                            return Err(AnimationError::InvalidAnimationSection);
                        }
                        let mut keys = Vec::with_capacity(count);
                        for k in 0..count {
                            let at = index + k * stride;
                            let out_tangent = if stride == 4 {
                                values[at + 3]
                            } else {
                                values[at + 2]
                            };
                            keys.push(Key {
                                frame: values[at],
                                value: values[at + 1] * scale,
                                incoming: values[at + 2] * scale,
                                outgoing: out_tangent * scale,
                            });
                        }
                        Track {
                            keys: Some(keys),
                            constant: 0.0,
                        }
                    };
                    components[component][axis] = track;
                }
            }
            tracks.push(JointTracks {
                scale: components[0].clone(),
                rotation: components[1].clone(),
                translation: components[2].clone(),
            });
        }
        Ok(Self {
            loop_mode,
            frame_count,
            joint_count,
            tracks,
        })
    }

    pub fn raw(&self, model: &J3dModel, frame: f64) -> Vec<RawJoint> {
        model
            .joints
            .iter()
            .enumerate()
            .map(|(joint, rest)| {
                if joint < self.joint_count {
                    let tracks = &self.tracks[joint];
                    RawJoint {
                        scale: std::array::from_fn(|axis| tracks.scale[axis].sample(frame)),
                        rotation: std::array::from_fn(|axis| tracks.rotation[axis].sample(frame)),
                        translation: std::array::from_fn(|axis| {
                            tracks.translation[axis].sample(frame)
                        }),
                    }
                } else {
                    RawJoint {
                        scale: rest.scale,
                        rotation: rest.rotation,
                        translation: rest.translation,
                    }
                }
            })
            .collect()
    }
}

/// Compose sampled joints with the requested J3D scale-compensation behavior.
pub fn world_matrices(model: &J3dModel, raw: &[RawJoint], ssc: bool) -> Vec<[f64; 12]> {
    let mut world = vec![[0.0; 12]; model.joints.len()];
    for &joint in &model.order {
        let parent = model.parent[joint];
        let inv_parent_scale =
            if ssc && model.matrix_calc == 2 && model.joints[joint].scale_compensate {
                parent.map(|p| raw[p].scale.map(|v| if v != 0.0 { 1.0 / v } else { 0.0 }))
            } else {
                None
            };
        let pose = raw[joint];
        let local = trs_euler(
            pose.translation,
            pose.rotation,
            pose.scale,
            inv_parent_scale,
        );
        world[joint] = parent.map(|p| mmul(&world[p], &local)).unwrap_or(local);
    }
    world
}

/// Reconstruct the Maya fitted-world matrices used by figure recipes.
pub fn fitted_world(model: &J3dModel, raw: &[RawJoint]) -> Vec<[f64; 12]> {
    fitted_world_overrides(model, raw, &[])
}

/// Build fitted-world matrices with recipe-specific joint translation overrides.
pub fn fitted_world_overrides(
    model: &J3dModel,
    raw: &[RawJoint],
    overrides: &[(usize, bool, [f64; 3])],
) -> Vec<[f64; 12]> {
    let rest = if overrides.iter().any(|(_, use_rest, _)| *use_rest) {
        let joints = model
            .joints
            .iter()
            .map(|joint| RawJoint {
                scale: joint.scale,
                rotation: joint.rotation,
                translation: joint.translation,
            })
            .collect::<Vec<_>>();
        Some(world_matrices(model, &joints, true))
    } else {
        None
    };
    let mut world_j3d = vec![[0.0; 12]; model.joints.len()];
    let mut world = vec![[0.0; 12]; model.joints.len()];
    for &joint in &model.order {
        let parent = model.parent[joint];
        let parent_scale = parent.map(|p| raw[p].scale);
        let inv_parent_scale = if model.matrix_calc == 2 && model.joints[joint].scale_compensate {
            parent_scale.map(|s| s.map(|v| if v != 0.0 { 1.0 / v } else { 0.0 }))
        } else {
            None
        };
        let pose = raw[joint];
        let local_j3d = trs_euler(
            pose.translation,
            pose.rotation,
            pose.scale,
            inv_parent_scale,
        );
        world_j3d[joint] = parent
            .map(|p| mmul(&world_j3d[p], &local_j3d))
            .unwrap_or(local_j3d);
        let scaled = parent_scale.is_some_and(|s| s.iter().any(|v| (v - 1.0).abs() > 1e-6));
        let needs_compensation =
            model.matrix_calc == 2 && model.joints[joint].scale_compensate && scaled;
        let parent_is_clean = parent.is_none_or(|p| close(&world[p], &world_j3d[p]));
        let local = if !needs_compensation && parent_is_clean {
            local_j3d
        } else {
            let p = parent.expect("a root transform cannot require parent compensation");
            let relative = mmul(&minv(&world[p]), &world_j3d[joint]);
            let (rotation, scale) = polar_rs(&relative);
            [
                rotation[0] * scale[0],
                rotation[1] * scale[1],
                rotation[2] * scale[2],
                relative[3],
                rotation[3] * scale[0],
                rotation[4] * scale[1],
                rotation[5] * scale[2],
                relative[7],
                rotation[6] * scale[0],
                rotation[7] * scale[1],
                rotation[8] * scale[2],
                relative[11],
            ]
        };
        world[joint] = parent.map(|p| mmul(&world[p], &local)).unwrap_or(local);
        if let Some((_, use_rest, offset)) =
            overrides.iter().find(|(target, _, _)| *target == joint)
        {
            let mut matrix = if *use_rest {
                rest.as_ref().unwrap()[joint]
            } else {
                world[joint]
            };
            matrix[3] += offset[0];
            matrix[7] += offset[1];
            matrix[11] += offset[2];
            world[joint] = matrix;
            world_j3d[joint] = matrix;
        }
    }
    world
}

/// Build Maya-aware local joint transforms from the sampled animation.
pub fn pose(model: &J3dModel, raw: &[RawJoint]) -> Vec<[f64; 12]> {
    let n = model.joints.len();
    let mut world_j3d = vec![[0.0; 12]; n];
    let mut world = vec![[0.0; 12]; n];
    let mut local = vec![[0.0; 12]; n];
    for &joint in &model.order {
        let parent = model.parent[joint];
        let parent_scale = parent.map(|p| raw[p].scale);
        let inv_parent_scale = if model.matrix_calc == 2 && model.joints[joint].scale_compensate {
            parent_scale.map(|s| s.map(|v| if v != 0.0 { 1.0 / v } else { 0.0 }))
        } else {
            None
        };
        let r = raw[joint];
        let local_j3d = trs_euler(r.translation, r.rotation, r.scale, inv_parent_scale);
        world_j3d[joint] = parent
            .map(|p| mmul(&world_j3d[p], &local_j3d))
            .unwrap_or(local_j3d);
        let scaled = parent_scale.is_some_and(|s| s.iter().any(|v| (v - 1.0).abs() > 1e-6));
        let needs_compensation =
            model.matrix_calc == 2 && model.joints[joint].scale_compensate && scaled;
        let parent_is_clean = parent.is_none_or(|p| close(&world[p], &world_j3d[p]));
        let loc = if !needs_compensation && parent_is_clean {
            trs_quat(r.translation, r.rotation, r.scale)
        } else {
            let p = parent.expect("a root transform cannot require parent compensation");
            let relative = mmul(&minv(&world[p]), &world_j3d[joint]);
            let (rotation, scale) = polar_rs(&relative);
            [
                rotation[0] * scale[0],
                rotation[1] * scale[1],
                rotation[2] * scale[2],
                relative[3],
                rotation[3] * scale[0],
                rotation[4] * scale[1],
                rotation[5] * scale[2],
                relative[7],
                rotation[6] * scale[0],
                rotation[7] * scale[1],
                rotation[8] * scale[2],
                relative[11],
            ]
        };
        local[joint] = loc;
        world[joint] = parent.map(|p| mmul(&world[p], &loc)).unwrap_or(loc);
    }
    local
}

pub fn deform(
    model: &J3dModel,
    local: &[[f64; 12]],
    corrections: &BTreeMap<usize, Correction>,
) -> Vec<[f64; 12]> {
    let n = model.joints.len();
    let mut world = vec![[0.0; 12]; n];
    let mut out = vec![[0.0; 12]; n];
    for &joint in &model.order {
        let parent = model.parent[joint];
        let mut loc = local[joint];
        if let Some(correction) = corrections.get(&joint) {
            loc = mmul(&correction.before_local, &loc);
        }
        if let Some(p) = parent {
            if let Some(correction) = corrections.get(&p) {
                loc = mmul(&correction.before_parent, &loc);
            }
            world[joint] = mmul(&world[p], &loc);
        } else {
            world[joint] = loc;
        }
    }
    for joint in 0..n {
        out[joint] = if let Some(correction) = corrections.get(&joint) {
            mmul(&world[joint], &correction.after_world)
        } else {
            mmul(&world[joint], &model.bind_inv[joint])
        };
    }
    out
}

pub fn correction(model: &J3dModel, joint: usize, matrix: [f64; 12]) -> Option<Correction> {
    let rest = *model.rest_local.get(joint)?;
    let inverse = minv(&matrix);
    Some(Correction {
        before_local: mmul(&mmul(&rest, &matrix), &minv(&rest)),
        before_parent: inverse,
        after_world: mmul(&inverse, &model.bind_inv[joint]),
    })
}

fn close(a: &[f64; 12], b: &[f64; 12]) -> bool {
    (0..12).all(|i| (a[i] - b[i]).abs() <= 1e-5 + 1e-5 * b[i].abs())
}

fn polar_rs(matrix: &[f64; 12]) -> ([f64; 9], [f64; 3]) {
    let a = [
        matrix[0], matrix[1], matrix[2], matrix[4], matrix[5], matrix[6], matrix[8], matrix[9],
        matrix[10],
    ];
    let mut x = a;
    for _ in 0..40 {
        let d = x[0] * (x[4] * x[8] - x[5] * x[7]) - x[1] * (x[3] * x[8] - x[5] * x[6])
            + x[2] * (x[3] * x[7] - x[4] * x[6]);
        if d == 0.0 {
            return (
                [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0],
                [a[0], a[4], a[8]],
            );
        }
        let it = [
            (x[4] * x[8] - x[5] * x[7]) / d,
            (x[5] * x[6] - x[3] * x[8]) / d,
            (x[3] * x[7] - x[4] * x[6]) / d,
            (x[2] * x[7] - x[1] * x[8]) / d,
            (x[0] * x[8] - x[2] * x[6]) / d,
            (x[1] * x[6] - x[0] * x[7]) / d,
            (x[1] * x[5] - x[2] * x[4]) / d,
            (x[2] * x[3] - x[0] * x[5]) / d,
            (x[0] * x[4] - x[1] * x[3]) / d,
        ];
        x = std::array::from_fn(|i| 0.5 * (x[i] + it[i]));
    }
    let scale = [
        x[0] * a[0] + x[3] * a[3] + x[6] * a[6],
        x[1] * a[1] + x[4] * a[4] + x[7] * a[7],
        x[2] * a[2] + x[5] * a[5] + x[8] * a[8],
    ];
    (x, scale)
}

/// Read the per-shape visibility channels from a BVA file.
pub fn bva_visibility(data: &[u8]) -> Result<Vec<Vec<u8>>, AnimationError> {
    let o = 0x20usize;
    let count = u16_at(data, o + 0x0c)? as usize;
    let shape_offset = u32_at(data, o + 0x10)? as usize;
    let value_offset = u32_at(data, o + 0x14)? as usize;
    let mut shapes = Vec::with_capacity(count);
    for shape in 0..count {
        let frames = u16_at(data, o + shape_offset + shape * 4)? as usize;
        let first = u16_at(data, o + shape_offset + shape * 4 + 2)? as usize;
        let start = add(o + value_offset, first)?;
        shapes.push(bytes(data, start, frames)?.to_vec());
    }
    Ok(shapes)
}

/// Construct a correction from an axis-angle rotation vector and translation.
pub fn rotation_vector_matrix(rotation: [f64; 3], translation: [f64; 3]) -> [f64; 12] {
    let theta2 = rotation[0] * rotation[0] + rotation[1] * rotation[1] + rotation[2] * rotation[2];
    if theta2 == 0.0 {
        return [
            1.0,
            0.0,
            0.0,
            translation[0],
            0.0,
            1.0,
            0.0,
            translation[1],
            0.0,
            0.0,
            1.0,
            translation[2],
        ];
    }
    let theta = theta2.sqrt();
    let (s, c) = sincos(theta);
    let (x, y, z) = (
        rotation[0] / theta,
        rotation[1] / theta,
        rotation[2] / theta,
    );
    let a = 1.0 - c;
    [
        c + a * x * x,
        a * x * y - s * z,
        a * x * z + s * y,
        translation[0],
        a * x * y + s * z,
        c + a * y * y,
        a * y * z - s * x,
        translation[1],
        a * x * z - s * y,
        a * y * z + s * x,
        c + a * z * z,
        translation[2],
    ]
}

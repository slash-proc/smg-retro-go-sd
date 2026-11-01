//! J3D material TEV evaluation used by texture construction recipes.
//!
//! This mirrors the source sampling and material combiner behavior in
//! `tools/asset_construction/textures_tev.py`. It currently exposes the
//! per-material effective RGBA path; image-recipe dispatch is in textures_gx.

use crate::j3d_model::J3dModel;
use crate::textures_gx::{bti_levels, j3d_texture_headers, TextureLevel};
use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TevError {
    InvalidModel,
    Truncated,
    InvalidMaterial,
    Texture,
}

#[derive(Clone)]
struct Stage {
    color: [u8; 4],
    alpha: [u8; 4],
    color_op: u8,
    alpha_op: u8,
    color_bias: u8,
    color_scale: u8,
    color_clamp: u8,
    color_reg: u8,
    alpha_bias: u8,
    alpha_scale: u8,
    alpha_clamp: u8,
    alpha_reg: u8,
    index: usize,
}

#[derive(Clone)]
struct Material {
    name: String,
    color: [f64; 4],
    ras_from_vertex: bool,
    konst: [[f64; 4]; 4],
    color_konst: [u8; 16],
    alpha_konst: [u8; 16],
    order: [Option<[u8; 3]>; 16],
    regs: [[f64; 4]; 4],
    stages: Vec<Stage>,
    swap_tables: [[u8; 4]; 4],
    swap: [(u8, u8); 16],
    textures: [Option<usize>; 8],
    first_texmap: Option<usize>,
    uv_src: i8,
}

#[derive(Clone, Debug, PartialEq)]
pub struct TextureSrt {
    pub center: [f64; 3],
    pub scale: [f64; 2],
    pub rotation: i16,
    pub translation: [f64; 2],
}

#[derive(Clone, Debug, PartialEq)]
pub struct BakeMaterialInfo {
    pub first: Option<usize>,
    pub uv_src: i8,
    pub srt: Option<TextureSrt>,
    /// 0 = opaque, 1 = alpha mask, 2 = blended.
    pub alpha_mode: u8,
    pub alpha_ref: f32,
}

#[derive(Clone)]
pub struct TevModel {
    materials: Vec<Material>,
    bake_info: Vec<BakeMaterialInfo>,
    textures: Vec<TextureLevel>,
    texture_wraps: Vec<(u8, u8)>,
    texture_names: Vec<String>,
    shape_materials: Vec<Option<usize>>,
    shape_color_indices: Vec<Vec<usize>>,
    colors: Vec<[f64; 4]>,
}

fn bytes(data: &[u8], at: usize, size: usize) -> Result<&[u8], TevError> {
    data.get(at..at.checked_add(size).ok_or(TevError::Truncated)?)
        .ok_or(TevError::Truncated)
}
fn u8_at(data: &[u8], at: usize) -> Result<u8, TevError> {
    Ok(bytes(data, at, 1)?[0])
}
fn u16_at(data: &[u8], at: usize) -> Result<u16, TevError> {
    Ok(u16::from_be_bytes(bytes(data, at, 2)?.try_into().unwrap()))
}
fn i16_at(data: &[u8], at: usize) -> Result<i16, TevError> {
    Ok(i16::from_be_bytes(bytes(data, at, 2)?.try_into().unwrap()))
}
fn u32_at(data: &[u8], at: usize) -> Result<u32, TevError> {
    Ok(u32::from_be_bytes(bytes(data, at, 4)?.try_into().unwrap()))
}
fn f32_at(data: &[u8], at: usize) -> Result<f64, TevError> {
    Ok(f32::from_bits(u32_at(data, at)?) as f64)
}
fn table_offset(data: &[u8], section: usize, index: usize) -> Result<Option<usize>, TevError> {
    let relative = u32_at(data, section + 0x0c + index * 4)? as usize;
    if relative == 0 {
        Ok(None)
    } else {
        Ok(Some(
            section.checked_add(relative).ok_or(TevError::Truncated)?,
        ))
    }
}

impl TevModel {
    pub fn parse(data: &[u8]) -> Result<Self, TevError> {
        let model = J3dModel::parse(data).map_err(|_| TevError::InvalidModel)?;
        let mut textures = Vec::new();
        let texture_headers = j3d_texture_headers(data).map_err(|_| TevError::Texture)?;
        let mut texture_wraps = Vec::with_capacity(texture_headers.len());
        for &header in &texture_headers {
            textures.push(
                bti_levels(data, header, false)
                    .map_err(|_| TevError::Texture)?
                    .into_iter()
                    .next()
                    .ok_or(TevError::Texture)?,
            );
            texture_wraps.push((u8_at(data, header + 6)?, u8_at(data, header + 7)?));
        }
        let mut result = Self {
            materials: Vec::new(),
            bake_info: parse_bake_material_info(data, &model)?,
            textures,
            texture_wraps,
            texture_names: model.texture_names.clone(),
            shape_materials: model.shapes.iter().map(|shape| shape.material).collect(),
            shape_color_indices: model
                .shapes
                .iter()
                .map(|shape| {
                    shape
                        .vertices
                        .iter()
                        .filter_map(|v| v.attributes.get(&11).copied())
                        .collect()
                })
                .collect(),
            colors: parse_vertex_colors(data, model.sections.get(b"VTX1").copied())?,
        };
        let (section, _) = model
            .sections
            .get(b"MAT3")
            .or_else(|| model.sections.get(b"MAT2"))
            .copied()
            .ok_or(TevError::InvalidMaterial)?;
        let tables = (0..30)
            .map(|i| table_offset(data, section, i))
            .collect::<Result<Vec<_>, _>>()?;
        let count = u16_at(data, section + 8)? as usize;
        let init = tables[0].ok_or(TevError::InvalidMaterial)?;
        let remap = tables[1].ok_or(TevError::InvalidMaterial)?;
        let names = tables[2]
            .map(|base| material_names(data, base))
            .transpose()?
            .unwrap_or_default();
        for i in 0..count {
            let p = init
                .checked_add(u16_at(data, remap + i * 2)? as usize * 0x14c)
                .ok_or(TevError::Truncated)?;
            result.materials.push(parse_material(
                data,
                p,
                &tables,
                names.get(i).cloned().unwrap_or_else(|| format!("mat{i}")),
            )?);
        }
        Ok(result)
    }

    pub fn material_count(&self) -> usize {
        self.materials.len()
    }

    pub fn material_name(&self, index: usize) -> Result<&str, TevError> {
        self.materials
            .get(index)
            .map(|material| material.name.as_str())
            .ok_or(TevError::InvalidMaterial)
    }

    pub fn bake_material_info(&self, index: usize) -> Result<&BakeMaterialInfo, TevError> {
        self.bake_info.get(index).ok_or(TevError::InvalidMaterial)
    }

    pub fn bake_texture_index(&self, index: usize) -> Result<Option<usize>, TevError> {
        let material = self.materials.get(index).ok_or(TevError::InvalidMaterial)?;
        let Some(slot) = self.bake_material_info(index)?.first else {
            return Ok(None);
        };
        Ok(material.textures.get(slot).copied().flatten())
    }

    pub fn texture_wraps(&self, index: usize) -> Result<(u8, u8), TevError> {
        self.texture_wraps
            .get(index)
            .copied()
            .ok_or(TevError::Texture)
    }

    pub fn material_color(&self, index: usize) -> Result<[f64; 4], TevError> {
        self.materials
            .get(index)
            .map(|material| material.color)
            .ok_or(TevError::InvalidMaterial)
    }

    /// Return the first two TEV color registers used by the Freezeflame wall
    /// texture reconstruction recipe.
    pub fn ffwall_registers(&self, index: usize) -> Result<([f64; 3], [f64; 3]), TevError> {
        let material = self.materials.get(index).ok_or(TevError::InvalidMaterial)?;
        Ok((
            material.regs[0][..3].try_into().unwrap(),
            material.regs[1][..3].try_into().unwrap(),
        ))
    }

    /// Apply the named color-register values sampled from a BRK animation.
    pub fn apply_brk_registers(&mut self, registers: &BTreeMap<String, BTreeMap<usize, [i16; 4]>>) {
        for material in &mut self.materials {
            let Some(overrides) = registers.get(&material.name) else {
                continue;
            };
            for (&index, value) in overrides {
                if let Some(register) = material.regs.get_mut(index) {
                    *register = value.map(|channel| channel as f64 / 255.0);
                }
            }
        }
    }

    /// Apply the named material texture-slot changes sampled from a BTP.
    pub fn apply_btp_textures(
        &mut self,
        changes: &[(String, usize, usize)],
    ) -> Result<(), TevError> {
        for (name, slot, texture) in changes {
            let Some(material) = self
                .materials
                .iter_mut()
                .find(|material| material.name == *name)
            else {
                continue;
            };
            if let Some(material_slot) = material.textures.get_mut(*slot) {
                if *texture >= self.textures.len() {
                    return Err(TevError::Texture);
                }
                *material_slot = Some(*texture);
            }
        }
        Ok(())
    }

    /// Override one named material's RGBA color from a DOL color table.
    pub fn apply_material_color(&mut self, name: &str, rgba: [u8; 4]) {
        if let Some(material) = self
            .materials
            .iter_mut()
            .find(|material| material.name == name)
        {
            material.color = rgba.map(|channel| channel as f64 / 255.0);
        }
    }

    pub fn material_index(&self, name: &str) -> Result<usize, TevError> {
        self.materials
            .iter()
            .position(|material| material.name == name)
            .ok_or(TevError::InvalidMaterial)
    }

    pub fn material_texture(&self, name: &str) -> Result<(usize, usize, Vec<u8>), TevError> {
        let material = self
            .materials
            .get(self.material_index(name)?)
            .ok_or(TevError::InvalidMaterial)?;
        let slot = material.first_texmap.ok_or(TevError::Texture)?;
        let index = material
            .textures
            .get(slot)
            .and_then(|index| *index)
            .ok_or(TevError::Texture)?;
        let texture = self.textures.get(index).ok_or(TevError::Texture)?;
        Ok((
            texture.width as usize,
            texture.height as usize,
            texture.rgba.clone(),
        ))
    }

    pub fn texture_level(&self, index: usize) -> Result<TextureLevel, TevError> {
        self.textures.get(index).cloned().ok_or(TevError::Texture)
    }

    pub fn texture_named(&self, name: &str) -> Result<TextureLevel, TevError> {
        let index = self
            .texture_names
            .iter()
            .position(|texture| texture == name)
            .ok_or(TevError::Texture)?;
        self.texture_level(index)
    }

    pub fn ice_parameters(&self, name: &str) -> Result<([f64; 3], [f64; 3]), TevError> {
        let material = self
            .materials
            .get(self.material_index(name)?)
            .ok_or(TevError::InvalidMaterial)?;
        Ok((
            material.color[..3].try_into().unwrap(),
            material.konst[0][..3].try_into().unwrap(),
        ))
    }

    pub fn power_star_parameters(
        &self,
        name: &str,
    ) -> Result<([f64; 4], [f64; 4], [f64; 4]), TevError> {
        let material = self
            .materials
            .get(self.material_index(name)?)
            .ok_or(TevError::InvalidMaterial)?;
        Ok((material.regs[0], material.regs[1], material.color))
    }

    pub fn effective_rgba(
        &self,
        material_index: usize,
    ) -> Result<(usize, usize, Vec<u8>), TevError> {
        self.effective_rgba_with_stages(material_index, None)
    }

    pub fn effective_rgba_with_stages(
        &self,
        material_index: usize,
        stage_limit: Option<usize>,
    ) -> Result<(usize, usize, Vec<u8>), TevError> {
        let (width, height, pixels) =
            self.effective_pixels_with_stages(material_index, stage_limit)?;
        let mut rgba = Vec::with_capacity(width * height * 4);
        for pixel in pixels {
            for channel in pixel {
                rgba.push(round_even(channel * 255.0));
            }
        }
        Ok((width, height, rgba))
    }

    /// Return the effective TEV image as unquantized, normalized channel values.
    /// Texture-delta recipes use these values before the final byte rounding.
    pub fn effective_pixels_with_stages(
        &self,
        material_index: usize,
        stage_limit: Option<usize>,
    ) -> Result<(usize, usize, Vec<[f64; 4]>), TevError> {
        let m = self
            .materials
            .get(material_index)
            .ok_or(TevError::InvalidMaterial)?;
        let mut slot_textures = [None; 8];
        let mut base = None;
        for stage in &m.stages {
            if let Some(Some(order)) = m.order.get(stage.index) {
                let slot = order[1] as usize;
                if slot < 8 {
                    if let Some(texture_index) = m.textures[slot] {
                        if let Some(texture) = self.textures.get(texture_index) {
                            base.get_or_insert((texture.height as usize, texture.width as usize));
                            slot_textures[slot] = Some(texture);
                        }
                    }
                }
            }
        }
        let (height, width) = base.unwrap_or((1, 1));
        let first_texture_stage = m
            .stages
            .iter()
            .position(|stage| stage.color.iter().any(|&input| input == 8 || input == 9));
        if first_texture_stage.is_none_or(|stage| stage > 0) {
            if let Some(texture) = slot_textures.iter().flatten().next() {
                let sampled = sample_texture(texture, width, height);
                return Ok((width, height, sampled));
            }
            return Ok((width, height, vec![m.color; width * height]));
        }

        let max_stages = stage_limit.unwrap_or_else(|| auto_stage_limit(&m.stages));
        let first_tex = first_texture_stage.unwrap();
        let stage_count = max_stages.max(first_tex + 1).min(m.stages.len());
        let base_color = if m.ras_from_vertex {
            self.vertex_color_mean(material_index).unwrap_or(m.color)
        } else {
            m.color
        };
        let n = width.checked_mul(height).ok_or(TevError::Truncated)?;
        let mut previous = vec![m.regs[3]; n];
        let mut registers = [vec![m.regs[0]; n], vec![m.regs[1]; n], vec![m.regs[2]; n]];
        let ones = [1.0; 4];
        for stage in m.stages.iter().take(stage_count) {
            let order = m.order[stage.index];
            let slot = order.and_then(|o| (o[1] < 8).then_some(o[1] as usize));
            let texture = slot.and_then(|slot| {
                slot_textures[slot].map(|texture| sample_texture(texture, width, height))
            });
            let channel = order.map_or(0xff, |o| o[2]);
            let rs = m.swap[stage.index].0 as usize % 4;
            let ts = m.swap[stage.index].1 as usize % 4;
            let (color_konst, alpha_konst) = stage_konst(m, stage.index);
            let ras = swizzle(
                if matches!(channel, 0 | 2 | 4) {
                    base_color
                } else {
                    [0.0; 4]
                },
                m.swap_tables[rs],
            );
            let mut out_color = vec![[0.0; 3]; n];
            let mut out_alpha = vec![0.0; n];
            for p in 0..n {
                let texel = swizzle(texture.as_ref().map_or(ones, |t| t[p]), m.swap_tables[ts]);
                out_color[p] = stage_color(
                    stage,
                    previous[p],
                    registers[0][p],
                    registers[1][p],
                    registers[2][p],
                    texel,
                    ras,
                    color_konst,
                );
                out_alpha[p] = stage_alpha(
                    stage,
                    previous[p],
                    registers[0][p],
                    registers[1][p],
                    registers[2][p],
                    texel,
                    ras,
                    alpha_konst,
                );
            }
            let color_dst = if stage.color_reg == 0 {
                &mut previous
            } else {
                &mut registers[(stage.color_reg - 1).min(2) as usize]
            };
            for (pixel, color) in color_dst.iter_mut().zip(out_color) {
                pixel[..3].copy_from_slice(&color);
            }
            let alpha_dst = if stage.alpha_reg == 0 {
                &mut previous
            } else {
                &mut registers[(stage.alpha_reg - 1).min(2) as usize]
            };
            for (pixel, alpha) in alpha_dst.iter_mut().zip(out_alpha) {
                pixel[3] = alpha;
            }
        }
        for pixel in &mut previous {
            for value in pixel {
                *value = clip(*value, 0.0, 1.0);
            }
        }
        Ok((width, height, previous))
    }

    pub fn effective_mean_rgb(
        &self,
        material_index: usize,
        stage_limit: Option<usize>,
    ) -> Result<[u8; 3], TevError> {
        let (width, height, rgba) = self.effective_rgba_with_stages(material_index, stage_limit)?;
        let mut sum = [0.0; 3];
        for pixel in rgba.chunks_exact(4) {
            for c in 0..3 {
                sum[c] += pixel[c] as f64 / 255.0;
            }
        }
        let count = width.checked_mul(height).ok_or(TevError::Truncated)? as f64;
        Ok(sum.map(|value| round_even(value / count * 255.0)))
    }

    pub fn vertex_color_mean(&self, material: usize) -> Option<[f64; 4]> {
        let mut sum = [0.0; 4];
        let mut count = 0usize;
        for (index, mat) in self.shape_materials.iter().enumerate() {
            if *mat != Some(material) {
                continue;
            }
            for color_index in &self.shape_color_indices[index] {
                let Some(color) = self.colors.get(*color_index) else {
                    continue;
                };
                for c in 0..4 {
                    sum[c] += color[c];
                }
                count += 1;
            }
        }
        if count == 0 {
            None
        } else {
            Some(sum.map(|v| v / count as f64))
        }
    }

    pub fn env_flat(&self, material_index: usize) -> Result<[u8; 3], TevError> {
        let material = self
            .materials
            .get(material_index)
            .ok_or(TevError::InvalidMaterial)?;
        let ras = if material.ras_from_vertex {
            self.vertex_color_mean(material_index)
                .unwrap_or(material.color)
        } else {
            material.color
        };
        let color = self.environment_color(material_index, ras)?;
        Ok([
            round_even(color[0] * 255.0),
            round_even(color[1] * 255.0),
            round_even(color[2] * 255.0),
        ])
    }

    pub fn ramp(
        &self,
        material_index: usize,
        width: usize,
        height: usize,
    ) -> Result<Vec<u8>, TevError> {
        let hue = self
            .vertex_hue(material_index)
            .ok_or(TevError::InvalidMaterial)?;
        if width == 0 || height == 0 {
            return Err(TevError::InvalidMaterial);
        }
        let mut row = Vec::with_capacity(width * 4);
        for x in 0..width {
            let scale = (x as f64 + 0.5) / width as f64;
            let color = self.environment_color(
                material_index,
                [hue[0] * scale, hue[1] * scale, hue[2] * scale, 1.0],
            )?;
            for value in color[..3].iter().copied() {
                row.push(to_byte_f32(value as f32));
            }
            row.push(255);
        }
        let mut out = Vec::with_capacity(width * height * 4);
        for _ in 0..height {
            out.extend_from_slice(&row);
        }
        Ok(out)
    }

    pub fn flat_mean(&self, material_index: usize) -> Result<[u8; 3], TevError> {
        let m = self
            .materials
            .get(material_index)
            .ok_or(TevError::InvalidMaterial)?;
        let mut textures = BTreeMap::<usize, &TextureLevel>::new();
        let mut height = 1usize;
        let mut width = 1usize;
        for stage in &m.stages {
            let Some(order) = m.order[stage.index] else {
                continue;
            };
            let slot = order[1] as usize;
            if slot >= 8 {
                continue;
            }
            if let Some(Some(texture_index)) = m.textures.get(slot) {
                if let Some(texture) = self.textures.get(*texture_index) {
                    textures.insert(stage.index, texture);
                    height = height.max(texture.height as usize);
                    width = width.max(texture.width as usize);
                }
            }
        }
        let n = width.checked_mul(height).ok_or(TevError::Truncated)?;
        let mut regs = [
            vec![[0.0; 3]; n],
            vec![[0.0; 3]; n],
            vec![[0.0; 3]; n],
            vec![[0.0; 3]; n],
        ];
        regs[0].fill([m.regs[3][0], m.regs[3][1], m.regs[3][2]]);
        regs[1].fill([m.regs[0][0], m.regs[0][1], m.regs[0][2]]);
        regs[2].fill([m.regs[1][0], m.regs[1][1], m.regs[1][2]]);
        regs[3].fill([m.regs[2][0], m.regs[2][1], m.regs[2][2]]);
        let ras = [m.color[0], m.color[1], m.color[2]];
        for stage in &m.stages {
            let tex = textures
                .get(&stage.index)
                .map(|texture| sample_texture(texture, width, height));
            let (kc, _) = stage_konst(m, stage.index);
            let bias = match stage.color_bias {
                1 => 0.5,
                2 => -0.5,
                _ => 0.0,
            };
            let scale = match stage.color_scale {
                1 => 2.0,
                2 => 4.0,
                3 => 0.5,
                _ => 1.0,
            };
            let mut out = vec![[0.0; 3]; n];
            for p in 0..n {
                for ch in 0..3 {
                    let inputs = stage.color.map(|selector| match selector {
                        0 => regs[0][p][ch],
                        2 => regs[1][p][ch],
                        4 => regs[2][p][ch],
                        6 => regs[3][p][ch],
                        8 => tex.as_ref().map_or(1.0, |v| v[p][ch]),
                        10 => ras[ch],
                        12 => 1.0,
                        13 => 0.5,
                        14 => kc[ch],
                        _ => 0.0,
                    });
                    let [a, b, c, d] = inputs;
                    out[p][ch] = clip((d + a * (1.0 - c) + b * c + bias) * scale, 0.0, 1.0);
                }
            }
            let dst = 2usize * stage.color_reg as usize;
            if dst < regs.len() {
                regs[dst] = out;
            }
        }
        let mut avg = [0.0; 3];
        for pixel in &regs[0] {
            for c in 0..3 {
                avg[c] += pixel[c];
            }
        }
        for c in 0..3 {
            avg[c] /= n as f64;
        }
        Ok(avg.map(|value| round_even(value * 255.0)))
    }

    pub fn vlerp(
        &self,
        texture_index: usize,
        material_index: usize,
    ) -> Result<(usize, usize, Vec<u8>), TevError> {
        let texture = self.textures.get(texture_index).ok_or(TevError::Texture)?;
        let material = self
            .materials
            .get(material_index)
            .ok_or(TevError::InvalidMaterial)?;
        let ras = self
            .vertex_color_mean(material_index)
            .ok_or(TevError::InvalidMaterial)?;
        let mut out = vec![0u8; texture.rgba.len()];
        for (src, dst) in texture.rgba.chunks_exact(4).zip(out.chunks_exact_mut(4)) {
            let lum = ((byte_float(src[0]) + byte_float(src[1])) + byte_float(src[2])) / 3.0f32;
            let inv = 1.0f32 - lum;
            for c in 0..3 {
                let value =
                    ras[c] + inv as f64 * material.regs[0][c] + lum as f64 * material.konst[0][c];
                dst[c] = to_byte_f32(value as f32);
            }
            dst[3] = 255;
        }
        Ok((texture.width as usize, texture.height as usize, out))
    }

    fn vertex_hue(&self, material: usize) -> Option<[f64; 3]> {
        let mut rows = Vec::new();
        for (shape_index, mat) in self.shape_materials.iter().enumerate() {
            if *mat != Some(material) {
                continue;
            }
            for color_index in &self.shape_color_indices[shape_index] {
                let color = self.colors.get(*color_index)?;
                let max = color[0].max(color[1]).max(color[2]).max(1e-6);
                rows.push([color[0] / max, color[1] / max, color[2] / max]);
            }
        }
        if rows.is_empty() {
            return None;
        }
        let mut hue = [0.0; 3];
        for c in 0..3 {
            let mut values = rows.iter().map(|row| row[c]).collect::<Vec<_>>();
            values.sort_by(|a, b| a.total_cmp(b));
            let middle = values.len() / 2;
            hue[c] = if values.len() % 2 == 1 {
                values[middle]
            } else {
                (values[middle - 1] + values[middle]) / 2.0
            };
        }
        Some(hue)
    }

    fn environment_color(
        &self,
        material_index: usize,
        ras: [f64; 4],
    ) -> Result<[f64; 4], TevError> {
        let m = self
            .materials
            .get(material_index)
            .ok_or(TevError::InvalidMaterial)?;
        let mut texvals = [[1.0; 4]; 8];
        for (slot, texture) in m.textures.iter().enumerate() {
            let Some(texture) = texture.and_then(|index| self.textures.get(index)) else {
                continue;
            };
            texvals[slot] = texture_mean(texture, m.uv_src < 0);
        }
        let mut previous = m.regs[3];
        let mut registers = [m.regs[0], m.regs[1], m.regs[2]];
        for stage in &m.stages {
            let order = m.order[stage.index];
            let tex = order
                .and_then(|o| (o[1] < 8).then_some(texvals[o[1] as usize]))
                .unwrap_or([1.0; 4]);
            let channel = order.map_or(0xff, |o| o[2]);
            let rs = m.swap[stage.index].0 as usize % 4;
            let ts = m.swap[stage.index].1 as usize % 4;
            let (kc, ka) = stage_konst(m, stage.index);
            let raster = swizzle(
                if matches!(channel, 0 | 2 | 4) {
                    ras
                } else {
                    [0.0; 4]
                },
                m.swap_tables[rs],
            );
            let texel = swizzle(tex, m.swap_tables[ts]);
            let color = stage_color(
                stage,
                previous,
                registers[0],
                registers[1],
                registers[2],
                texel,
                raster,
                kc,
            );
            let alpha = stage_alpha(
                stage,
                previous,
                registers[0],
                registers[1],
                registers[2],
                texel,
                raster,
                ka,
            );
            let color_dst = if stage.color_reg == 0 {
                &mut previous
            } else {
                &mut registers[(stage.color_reg - 1).min(2) as usize]
            };
            color_dst[..3].copy_from_slice(&color);
            let alpha_dst = if stage.alpha_reg == 0 {
                &mut previous
            } else {
                &mut registers[(stage.alpha_reg - 1).min(2) as usize]
            };
            alpha_dst[3] = alpha;
        }
        Ok(previous.map(|value| clip(value, 0.0, 1.0)))
    }
}

fn parse_vertex_colors(
    data: &[u8],
    section: Option<(usize, usize)>,
) -> Result<Vec<[f64; 4]>, TevError> {
    let Some((section, size)) = section else {
        return Ok(Vec::new());
    };
    let formats = section
        .checked_add(u32_at(data, section + 8)? as usize)
        .ok_or(TevError::Truncated)?;
    let offsets = (0..13)
        .map(|i| u32_at(data, section + 0x0c + i * 4).map(|v| v as usize))
        .collect::<Result<Vec<_>, _>>()?;
    let mut p = formats;
    let (mut components, mut kind, mut fraction) = (None, 0u32, 0u8);
    loop {
        let attr = u32_at(data, p)?;
        let cc = u32_at(data, p + 4)?;
        let ct = u32_at(data, p + 8)?;
        let frac = u8_at(data, p + 12)?;
        p += 16;
        if attr == 0xff {
            break;
        }
        if attr == 11 {
            components = Some(cc);
            kind = ct;
            fraction = frac;
        }
    }
    let Some(components) = components else {
        return Ok(Vec::new());
    };
    let start = offsets[3];
    if start == 0 {
        return Ok(Vec::new());
    }
    let end = offsets
        .iter()
        .copied()
        .filter(|&v| v > start)
        .min()
        .unwrap_or(size);
    let raw = bytes(data, section + start, end - start)?;
    let _ = (components, fraction);
    let stride = match kind {
        0 | 3 => 2,
        1 | 4 => 3,
        2 | 5 => 4,
        _ => return Err(TevError::InvalidModel),
    };
    let mut colors = Vec::with_capacity(raw.len() / stride);
    for pixel in raw.chunks_exact(stride) {
        let c = match kind {
            0 => {
                let v = u16::from_be_bytes([pixel[0], pixel[1]]);
                let r = ((v >> 11) & 31) as u8;
                let g = ((v >> 5) & 63) as u8;
                let b = (v & 31) as u8;
                [
                    ((r << 3) | (r >> 2)) as f64,
                    ((g << 2) | (g >> 4)) as f64,
                    ((b << 3) | (b >> 2)) as f64,
                    255.0,
                ]
            }
            1 => [pixel[0] as f64, pixel[1] as f64, pixel[2] as f64, 255.0],
            2 => [
                pixel[0] as f64,
                pixel[1] as f64,
                pixel[2] as f64,
                pixel[3] as f64,
            ],
            3 => {
                let v = u16::from_be_bytes([pixel[0], pixel[1]]);
                [
                    ((v >> 12) & 15) as f64 * 17.0,
                    ((v >> 8) & 15) as f64 * 17.0,
                    ((v >> 4) & 15) as f64 * 17.0,
                    (v & 15) as f64 * 17.0,
                ]
            }
            4 => {
                let v = ((pixel[0] as u32) << 16) | ((pixel[1] as u32) << 8) | pixel[2] as u32;
                [
                    ((v >> 18) & 63) as f64 * 255.0 / 63.0,
                    ((v >> 12) & 63) as f64 * 255.0 / 63.0,
                    ((v >> 6) & 63) as f64 * 255.0 / 63.0,
                    (v & 63) as f64 * 255.0 / 63.0,
                ]
            }
            _ => [
                pixel[0] as f64,
                pixel[1] as f64,
                pixel[2] as f64,
                pixel[3] as f64,
            ],
        };
        colors.push(c);
    }
    let max = colors.iter().flatten().copied().fold(0.0f64, f64::max);
    if max > 1.0 {
        for color in &mut colors {
            for value in color {
                *value /= 255.0;
            }
        }
    }
    Ok(colors)
}

fn material_names(data: &[u8], base: usize) -> Result<Vec<String>, TevError> {
    let count = u16_at(data, base)? as usize;
    let mut names = Vec::with_capacity(count);
    for index in 0..count {
        let relative = u16_at(data, base + 6 + index * 4)? as usize;
        let start = base.checked_add(relative).ok_or(TevError::Truncated)?;
        let tail = data.get(start..).ok_or(TevError::Truncated)?;
        let end = tail
            .iter()
            .position(|&byte| byte == 0)
            .ok_or(TevError::Truncated)?;
        names.push(
            encoding_rs::SHIFT_JIS
                .decode_without_bom_handling(&tail[..end])
                .0
                .into_owned(),
        );
    }
    Ok(names)
}

fn parse_bake_material_info(
    data: &[u8],
    model: &J3dModel,
) -> Result<Vec<BakeMaterialInfo>, TevError> {
    let section = model
        .sections
        .get(b"MAT3")
        .or_else(|| model.sections.get(b"MAT2"))
        .copied()
        .ok_or(TevError::InvalidMaterial)?
        .0;
    let count = u16_at(data, section + 8)? as usize;
    let tables = (0..30)
        .map(|index| {
            let relative = u32_at(data, section + 0x0c + index * 4)? as usize;
            if relative == 0 {
                Ok(None)
            } else {
                Ok(Some(
                    section.checked_add(relative).ok_or(TevError::Truncated)?,
                ))
            }
        })
        .collect::<Result<Vec<_>, TevError>>()?;
    let init = tables[0].ok_or(TevError::InvalidMaterial)?;
    let remap = tables[1].ok_or(TevError::InvalidMaterial)?;
    let texgen_table = tables[11];
    let texmtx_table = tables[13];
    let texremap_table = tables[15];
    let tevorder_table = tables[16];
    let alphacmp_table = tables[24];
    let blend_table = tables[25];
    let mut output = Vec::with_capacity(count);

    for material_index in 0..count {
        let p = init
            .checked_add(u16_at(data, remap + material_index * 2)? as usize * 0x14c)
            .ok_or(TevError::Truncated)?;
        let mut texgens: [Option<[u8; 3]>; 8] = [None; 8];
        for (coord, entry) in texgens.iter_mut().enumerate() {
            let index = u16_at(data, p + 0x28 + coord * 2)?;
            if index != 0xffff {
                if let Some(base) = texgen_table {
                    *entry = Some(
                        bytes(data, base + index as usize * 4, 3)?
                            .try_into()
                            .unwrap(),
                    );
                }
            }
        }
        let mut texture_matrices: Vec<Option<TextureSrt>> = vec![None; 10];
        for (index, entry) in texture_matrices.iter_mut().enumerate() {
            let table_index = u16_at(data, p + 0x48 + index * 2)?;
            if table_index == 0xffff {
                continue;
            }
            if let Some(base) = texmtx_table {
                let q = base + table_index as usize * 0x64;
                *entry = Some(TextureSrt {
                    center: [
                        f32_at(data, q + 4)?,
                        f32_at(data, q + 8)?,
                        f32_at(data, q + 12)?,
                    ],
                    scale: [f32_at(data, q + 0x10)?, f32_at(data, q + 0x14)?],
                    rotation: i16_at(data, q + 0x18)?,
                    translation: [f32_at(data, q + 0x1c)?, f32_at(data, q + 0x20)?],
                });
            }
        }
        let mut slots = [None; 8];
        for (slot, value) in slots.iter_mut().enumerate() {
            let index = u16_at(data, p + 0x84 + slot * 2)?;
            if index != 0xffff {
                if let Some(base) = texremap_table {
                    *value = Some(u16_at(data, base + index as usize * 2)? as usize);
                }
            }
        }
        let mut used = [None; 8];
        if let Some(base) = tevorder_table {
            for order in 0..16 {
                let index = u16_at(data, p + 0xbc + order * 2)?;
                if index == 0xffff {
                    continue;
                }
                let entry = bytes(data, base + index as usize * 4, 3)?;
                let coord = entry[0] as usize;
                let slot = entry[1] as usize;
                if slot < 8 && slots[slot].is_some() && used[slot].is_none() {
                    used[slot] = Some(coord as u8);
                }
            }
        }
        let first_used = used.iter().position(Option::is_some);
        let (first, coord) = if let Some(slot) = first_used {
            (Some(slot), used[slot].map(usize::from))
        } else if let Some(slot) = slots.iter().position(Option::is_some) {
            (Some(slot), Some(slot))
        } else {
            (None, None)
        };
        let mut uv_src = 0i8;
        let mut srt = None;
        if let Some(coord) = coord.filter(|&coord| coord < 8) {
            if let Some(texgen) = texgens[coord] {
                uv_src = if (4..=11).contains(&texgen[1]) {
                    (texgen[1] - 4) as i8
                } else {
                    -1
                };
                let matrix = texgen[2] as usize;
                if (30..60).contains(&matrix) {
                    let matrix_index = (matrix - 30) / 3;
                    srt = texture_matrices.get(matrix_index).and_then(Clone::clone);
                }
            }
        }

        let mut alpha_mode = 0;
        let mut alpha_ref = 0.5f32;
        let alpha_index = u16_at(data, p + 0x146)?;
        if alpha_index != 0xffff {
            if let Some(base) = alphacmp_table {
                let q = base + alpha_index as usize * 8;
                let values = bytes(data, q, 5)?;
                let (comp0, ref0, op, comp1, ref1) =
                    (values[0], values[1], values[2], values[3], values[4]);
                let pass0 = comp0 == 7
                    || (matches!(comp0, 3 | 1) && ref0 == 255)
                    || (comp0 == 6 && ref0 == 0);
                let pass1 = comp1 == 7
                    || (matches!(comp1, 3 | 1) && ref1 == 255)
                    || (comp1 == 6 && ref1 == 0);
                if !((op == 0 && pass0 && pass1) || (op == 1 && (pass0 || pass1))) {
                    let lows = [(comp0, ref0), (comp1, ref1)].into_iter().filter_map(
                        |(compare, reference)| {
                            (matches!(compare, 4 | 6) && reference > 0).then_some(reference)
                        },
                    );
                    let lows: Vec<u8> = lows.collect();
                    alpha_mode = 1;
                    if let Some(reference) = if op == 0 {
                        lows.iter().max()
                    } else {
                        lows.iter().min()
                    } {
                        alpha_ref = *reference as f32 / 255.0;
                    }
                }
            }
        }
        let blend_index = u16_at(data, p + 0x148)?;
        if blend_index != 0xffff && alpha_mode == 0 {
            if let Some(base) = blend_table {
                let blend = bytes(data, base + blend_index as usize * 4, 3)?;
                if blend[0] == 1 && blend[1] == 4 && blend[2] == 5 {
                    alpha_mode = 2;
                }
            }
        }
        output.push(BakeMaterialInfo {
            first,
            uv_src,
            srt,
            alpha_mode,
            alpha_ref,
        });
    }
    Ok(output)
}

fn parse_material(
    data: &[u8],
    p: usize,
    tables: &[Option<usize>],
    name: String,
) -> Result<Material, TevError> {
    let table_rgba = |table: usize, index: u16| -> Result<[f64; 4], TevError> {
        if index == 0xffff {
            return Ok([1.0; 4]);
        }
        let Some(base) = tables[table] else {
            return Ok([1.0; 4]);
        };
        let raw: [u8; 4] = bytes(data, base + index as usize * 4, 4)?
            .try_into()
            .unwrap();
        Ok(raw.map(|value| value as f64 / 255.0))
    };
    let color = table_rgba(5, u16_at(data, p + 8)?)?;
    let channel = u16_at(data, p + 0x0c)?;
    let ras_from_vertex = tables[7].is_some_and(|base| {
        channel != 0xffff && u8_at(data, base + channel as usize * 8 + 1).ok() == Some(1)
    });
    let mut konst = [[1.0; 4]; 4];
    for k in 0..4 {
        konst[k] = table_rgba(18, u16_at(data, p + 0x94 + k * 2)?)?;
    }
    let color_konst = bytes(data, p + 0x9c, 16)?.try_into().unwrap();
    let alpha_konst = bytes(data, p + 0xac, 16)?.try_into().unwrap();
    let mut order = [None; 16];
    for (k, slot) in order.iter_mut().enumerate() {
        let index = u16_at(data, p + 0xbc + k * 2)?;
        if index != 0xffff {
            if let Some(base) = tables[16] {
                *slot = Some(
                    bytes(data, base + index as usize * 4, 3)?
                        .try_into()
                        .unwrap(),
                );
            }
        }
    }
    let mut regs = [[1.0; 4]; 4];
    for (k, reg) in regs.iter_mut().enumerate() {
        let index = u16_at(data, p + 0xdc + k * 2)?;
        if index != 0xffff {
            if let Some(base) = tables[17] {
                for (c, value) in reg.iter_mut().enumerate() {
                    *value = i16_at(data, base + index as usize * 8 + c * 2)? as f64 / 255.0;
                }
            }
        }
    }
    let stage_count = if let Some(base) = tables[19] {
        u8_at(data, base + u8_at(data, p + 4)? as usize)? as usize
    } else {
        1
    };
    let mut stages = Vec::with_capacity(stage_count);
    for k in 0..stage_count {
        let index = u16_at(data, p + 0xe4 + k * 2)?;
        if index == 0xffff {
            continue;
        }
        let base = tables[20].ok_or(TevError::InvalidMaterial)? + index as usize * 20;
        stages.push(Stage {
            color: bytes(data, base + 1, 4)?.try_into().unwrap(),
            color_op: u8_at(data, base + 5)?,
            color_bias: u8_at(data, base + 6)?,
            color_scale: u8_at(data, base + 7)?,
            color_clamp: u8_at(data, base + 8)?,
            color_reg: u8_at(data, base + 9)?,
            alpha: bytes(data, base + 10, 4)?.try_into().unwrap(),
            alpha_op: u8_at(data, base + 14)?,
            alpha_bias: u8_at(data, base + 15)?,
            alpha_scale: u8_at(data, base + 16)?,
            alpha_clamp: u8_at(data, base + 17)?,
            alpha_reg: u8_at(data, base + 18)?,
            index: k,
        });
    }
    let mut swap_tables = [[0, 1, 2, 3]; 4];
    for (k, table) in swap_tables.iter_mut().enumerate() {
        let index = u16_at(data, p + 0x124 + k * 2)?;
        if index != 0xffff {
            if let Some(base) = tables[22] {
                *table = bytes(data, base + index as usize * 4, 4)?
                    .try_into()
                    .unwrap();
            }
        }
    }
    let mut swap = [(0, 0); 16];
    for (k, pair) in swap.iter_mut().enumerate() {
        let index = u16_at(data, p + 0x104 + k * 2)?;
        if index != 0xffff {
            if let Some(base) = tables[21] {
                *pair = (
                    u8_at(data, base + index as usize * 4)?,
                    u8_at(data, base + index as usize * 4 + 1)?,
                );
            }
        }
    }
    let mut textures = [None; 8];
    for (k, slot) in textures.iter_mut().enumerate() {
        let index = u16_at(data, p + 0x84 + k * 2)?;
        if index != 0xffff {
            if let Some(base) = tables[15] {
                *slot = Some(u16_at(data, base + index as usize * 2)? as usize);
            }
        }
    }
    let mut used = [None; 8];
    for k in 0..16 {
        let order_index = u16_at(data, p + 0xbc + k * 2)?;
        if order_index == 0xffff {
            continue;
        }
        let Some(base) = tables[16] else { continue };
        let order = bytes(data, base + order_index as usize * 4, 3)?;
        let slot = order[1] as usize;
        if slot < 8 && textures[slot].is_some() && used[slot].is_none() {
            used[slot] = Some(order[0]);
        }
    }
    let first_texmap = used
        .iter()
        .position(Option::is_some)
        .or_else(|| textures.iter().position(Option::is_some));
    let coord = used
        .iter()
        .position(Option::is_some)
        .map(|slot| used[slot].unwrap() as usize)
        .or_else(|| textures.iter().position(Option::is_some));
    let mut uv_src = 0i8;
    if let (Some(coord), Some(table)) = (coord, tables[11]) {
        if coord < 8 {
            let texgen = u16_at(data, p + 0x28 + coord * 2)?;
            if texgen != 0xffff {
                let source = u8_at(data, table + texgen as usize * 4 + 1)?;
                uv_src = if (4..=11).contains(&source) {
                    (source - 4) as i8
                } else {
                    -1
                };
            }
        }
    }
    Ok(Material {
        name,
        color,
        ras_from_vertex,
        konst,
        color_konst,
        alpha_konst,
        order,
        regs,
        stages,
        swap_tables,
        swap,
        textures,
        first_texmap,
        uv_src,
    })
}

fn byte_float(value: u8) -> f32 {
    value as f32 * (1.0f32 / 255.0f32)
}
fn to_byte_f32(value: f32) -> u8 {
    const TOP: f32 = 1.0f32 - 0.5f32 / 255.0f32;
    if value <= 0.0 {
        0
    } else if value > TOP {
        255
    } else {
        (255.0f32 * value + 0.5f32) as u8
    }
}
fn texture_mean(texture: &TextureLevel, disc: bool) -> [f64; 4] {
    let width = texture.width as usize;
    let height = texture.height as usize;
    let mut sum = [0.0; 4];
    let mut count = 0usize;
    for y in 0..height {
        let dy = (y as f64 + 0.5 - height as f64 / 2.0) / (height as f64 / 2.0);
        for x in 0..width {
            let dx = (x as f64 + 0.5 - width as f64 / 2.0) / (width as f64 / 2.0);
            if disc && dx * dx + dy * dy > 1.0 {
                continue;
            }
            let at = (y * width + x) * 4;
            for c in 0..4 {
                sum[c] += texture.rgba[at + c] as f64 / 255.0;
            }
            count += 1;
        }
    }
    if count == 0 {
        return [0.0; 4];
    }
    sum.map(|value| value / count as f64)
}
fn sample_texture(texture: &TextureLevel, width: usize, height: usize) -> Vec<[f64; 4]> {
    let mut out = Vec::with_capacity(width * height);
    for y in 0..height {
        for x in 0..width {
            let sy = y * texture.height as usize / height;
            let sx = x * texture.width as usize / width;
            let at = (sy * texture.width as usize + sx) * 4;
            out.push([
                texture.rgba[at] as f64 / 255.0,
                texture.rgba[at + 1] as f64 / 255.0,
                texture.rgba[at + 2] as f64 / 255.0,
                texture.rgba[at + 3] as f64 / 255.0,
            ]);
        }
    }
    out
}
fn auto_stage_limit(stages: &[Stage]) -> usize {
    let Some(s) = stages.get(1) else { return 1 };
    if s.color[0] == 15
        && s.color_op == 0
        && matches!(s.color_scale, 1 | 2)
        && !s.color.contains(&8)
        && !s.color.contains(&9)
        && s.color[1..].contains(&0)
    {
        2
    } else {
        1
    }
}
fn stage_konst(m: &Material, index: usize) -> ([f64; 4], [f64; 4]) {
    fn select(m: &Material, sel: u8, alpha: bool) -> [f64; 4] {
        const FRACTIONS: [f64; 8] = [
            1.0,
            7.0 / 8.0,
            6.0 / 8.0,
            5.0 / 8.0,
            4.0 / 8.0,
            3.0 / 8.0,
            2.0 / 8.0,
            1.0 / 8.0,
        ];
        if sel < 8 {
            return [FRACTIONS[sel as usize]; 4];
        }
        if (12..=15).contains(&sel) && !alpha {
            return m.konst[(sel - 12) as usize];
        }
        if sel >= 16 {
            let k = ((sel - 16) % 4) as usize;
            let c = ((sel - 16) / 4) as usize;
            return [m.konst[k][c]; 4];
        }
        [1.0; 4]
    }
    (
        select(m, m.color_konst[index], false),
        select(m, m.alpha_konst[index], true),
    )
}
fn swizzle(value: [f64; 4], table: [u8; 4]) -> [f64; 4] {
    [
        value[table[0] as usize],
        value[table[1] as usize],
        value[table[2] as usize],
        value[table[3] as usize],
    ]
}
fn clip(value: f64, low: f64, high: f64) -> f64 {
    value.max(low).min(high)
}
fn combine(a: f64, b: f64, c: f64, d: f64, op: u8, bias: u8, scale: u8, clamp: u8) -> f64 {
    let v = (1.0 - clip(c, 0.0, 1.0)) * clip(a, 0.0, 1.0) + clip(c, 0.0, 1.0) * clip(b, 0.0, 1.0);
    let v = if op == 1 { d - v } else { d + v }
        + match bias {
            1 => 0.5,
            2 => -0.5,
            _ => 0.0,
        };
    let v = v * match scale {
        1 => 2.0,
        2 => 4.0,
        3 => 0.5,
        _ => 1.0,
    };
    clip(
        v,
        if clamp != 0 { 0.0 } else { -4.0 },
        if clamp != 0 { 1.0 } else { 4.0 },
    )
}
fn stage_color(
    s: &Stage,
    prev: [f64; 4],
    r0: [f64; 4],
    r1: [f64; 4],
    r2: [f64; 4],
    tex: [f64; 4],
    ras: [f64; 4],
    konst: [f64; 4],
) -> [f64; 3] {
    let mut out = [0.0; 3];
    for c in 0..3 {
        let source = [
            prev[c], prev[3], r0[c], r0[3], r1[c], r1[3], r2[c], r2[3], tex[c], tex[3], ras[c],
            ras[3], 1.0, 0.5, konst[c], 0.0,
        ];
        out[c] = combine(
            source[s.color[0].min(15) as usize],
            source[s.color[1].min(15) as usize],
            source[s.color[2].min(15) as usize],
            source[s.color[3].min(15) as usize],
            s.color_op,
            s.color_bias,
            s.color_scale,
            s.color_clamp,
        );
    }
    out
}
fn stage_alpha(
    s: &Stage,
    prev: [f64; 4],
    r0: [f64; 4],
    r1: [f64; 4],
    r2: [f64; 4],
    tex: [f64; 4],
    ras: [f64; 4],
    konst: [f64; 4],
) -> f64 {
    let source = [prev[3], r0[3], r1[3], r2[3], tex[3], ras[3], konst[3], 0.0];
    combine(
        source[s.alpha[0].min(7) as usize],
        source[s.alpha[1].min(7) as usize],
        source[s.alpha[2].min(7) as usize],
        source[s.alpha[3].min(7) as usize],
        s.alpha_op,
        s.alpha_bias,
        s.alpha_scale,
        s.alpha_clamp,
    )
}
fn round_even(value: f64) -> u8 {
    let base = value.floor();
    let fraction = value - base;
    let rounded = if fraction < 0.5 {
        base
    } else if fraction > 0.5 {
        base + 1.0
    } else if (base as i64) & 1 == 0 {
        base
    } else {
        base + 1.0
    };
    rounded.clamp(0.0, 255.0) as u8
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn combiner_clamps_inputs_and_output_like_native() {
        assert_eq!(combine(-2.0, 2.0, 0.5, 0.25, 0, 0, 0, 1), 0.75);
        assert_eq!(combine(0.0, 0.0, 0.0, -10.0, 0, 0, 0, 0), -4.0);
        assert_eq!(round_even(2.5), 2);
        assert_eq!(round_even(3.5), 4);
    }
}

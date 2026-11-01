//! Decoder for the tiled GX texture formats used by BTI and J3D TEX1 data.
//! Pixel and palette expansion follows `asset_construction/textures_gx.py`.

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TextureError {
    Truncated,
    UnsupportedFormat(u8),
    InvalidHeader,
    TooLarge,
    /// Internal stage marker for geometry-backed bake recipe failures.
    GeometryBake(u32),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextureLevel {
    pub width: u16,
    pub height: u16,
    pub rgba: Vec<u8>,
}

fn block(format: u8) -> Result<(usize, usize, usize), TextureError> {
    match format {
        0 | 8 | 14 => Ok((8, 8, 32)),
        1 | 2 | 9 => Ok((8, 4, 32)),
        3 | 4 | 5 | 10 => Ok((4, 4, 32)),
        6 => Ok((4, 4, 64)),
        _ => Err(TextureError::UnsupportedFormat(format)),
    }
}

pub fn byte_size(format: u8, width: usize, height: usize) -> Result<usize, TextureError> {
    let (bw, bh, bs) = block(format)?;
    width
        .checked_add(bw - 1)
        .and_then(|w| w.checked_div(bw))
        .and_then(|x| {
            height
                .checked_add(bh - 1)
                .and_then(|h| h.checked_div(bh).and_then(|y| x.checked_mul(y)))
        })
        .and_then(|n| n.checked_mul(bs))
        .ok_or(TextureError::TooLarge)
}

fn rgb565(v: u16) -> [u8; 4] {
    [
        (((v >> 11) & 31) as u32 * 255 / 31) as u8,
        (((v >> 5) & 63) as u32 * 255 / 63) as u8,
        ((v & 31) as u32 * 255 / 31) as u8,
        255,
    ]
}

fn rgb5a3(v: u16) -> [u8; 4] {
    if v & 0x8000 != 0 {
        [
            (((v >> 10) & 31) as u32 * 255 / 31) as u8,
            (((v >> 5) & 31) as u32 * 255 / 31) as u8,
            ((v & 31) as u32 * 255 / 31) as u8,
            255,
        ]
    } else {
        [
            (((v >> 8) & 15) as u8) * 17,
            (((v >> 4) & 15) as u8) * 17,
            ((v & 15) as u8) * 17,
            (((v >> 12) & 7) as u32 * 255 / 7) as u8,
        ]
    }
}

fn ia8(v: u16) -> [u8; 4] {
    let i = v as u8;
    [i, i, i, (v >> 8) as u8]
}

pub fn decode_palette(format: u8, data: &[u8]) -> Result<Vec<[u8; 4]>, TextureError> {
    let mut palette = Vec::new();
    palette
        .try_reserve(data.len() / 2)
        .map_err(|_| TextureError::TooLarge)?;
    for pair in data.chunks_exact(2) {
        let value = u16::from_be_bytes([pair[0], pair[1]]);
        palette.push(match format {
            0 => ia8(value),
            1 => rgb565(value),
            _ => rgb5a3(value),
        });
    }
    Ok(palette)
}

fn put(img: &mut [[u8; 4]], pitch: usize, x: usize, y: usize, color: [u8; 4]) {
    img[y * pitch + x] = color;
}

fn decode_cmpr(raw: &[u8], p: &mut usize, img: &mut [[u8; 4]], pitch: usize, ox: usize, oy: usize) {
    for sub in 0..4 {
        let sx = ox + (sub & 1) * 4;
        let sy = oy + (sub >> 1) * 4;
        let c0 = u16::from_be_bytes([raw[*p], raw[*p + 1]]);
        let c1 = u16::from_be_bytes([raw[*p + 2], raw[*p + 3]]);
        let p0 = rgb565(c0);
        let p1 = rgb565(c1);
        let mix = |a: usize, b: usize, wa: u16, wb: u16, div: u16| -> u8 {
            ((wa * p0[a] as u16 + wb * p1[b] as u16) / div) as u8
        };
        let p2 = if c0 > c1 {
            [
                mix(0, 0, 2, 1, 3),
                mix(1, 1, 2, 1, 3),
                mix(2, 2, 2, 1, 3),
                255,
            ]
        } else {
            [
                mix(0, 0, 1, 1, 2),
                mix(1, 1, 1, 1, 2),
                mix(2, 2, 1, 1, 2),
                255,
            ]
        };
        let p3 = if c0 > c1 {
            [
                mix(0, 0, 1, 2, 3),
                mix(1, 1, 1, 2, 3),
                mix(2, 2, 1, 2, 3),
                255,
            ]
        } else {
            [0, 0, 0, 0]
        };
        let colors = [p0, p1, p2, p3];
        for row in 0..4 {
            let bits = raw[*p + 4 + row];
            let base = (sy + row) * pitch + sx;
            img[base] = colors[(bits >> 6) as usize];
            img[base + 1] = colors[((bits >> 4) & 3) as usize];
            img[base + 2] = colors[((bits >> 2) & 3) as usize];
            img[base + 3] = colors[(bits & 3) as usize];
        }
        *p += 8;
    }
}

/// Decode one tiled GX image to tightly packed row-major RGBA8.
pub fn decode(
    format: u8,
    width: usize,
    height: usize,
    data: &[u8],
    palette: &[[u8; 4]],
) -> Result<Vec<u8>, TextureError> {
    let (bw, bh, _) = block(format)?;
    let bx = width.checked_add(bw - 1).ok_or(TextureError::TooLarge)? / bw;
    let by = height.checked_add(bh - 1).ok_or(TextureError::TooLarge)? / bh;
    let n = bx
        .checked_mul(by)
        .and_then(|v| v.checked_mul(block(format).ok()?.2))
        .ok_or(TextureError::TooLarge)?;
    let pitch = bx.checked_mul(bw).ok_or(TextureError::TooLarge)?;
    let rows = by.checked_mul(bh).ok_or(TextureError::TooLarge)?;
    let pixels = pitch.checked_mul(rows).ok_or(TextureError::TooLarge)?;
    let out_len = width
        .checked_mul(height)
        .and_then(|v| v.checked_mul(4))
        .ok_or(TextureError::TooLarge)?;
    let mut raw = Vec::new();
    raw.try_reserve_exact(n)
        .map_err(|_| TextureError::TooLarge)?;
    raw.extend_from_slice(&data[..data.len().min(n)]);
    raw.resize(n, 0);
    let mut img = Vec::new();
    img.try_reserve_exact(pixels)
        .map_err(|_| TextureError::TooLarge)?;
    img.resize(pixels, [0; 4]);
    let last = palette.len().saturating_sub(1);
    let mut p = 0usize;
    for byi in 0..by {
        for bxi in 0..bx {
            let ox = bxi * bw;
            let oy = byi * bh;
            if format == 14 {
                decode_cmpr(&raw, &mut p, &mut img, pitch, ox, oy);
                continue;
            }
            if format == 6 {
                for i in 0..16 {
                    let y = i / 4;
                    let x = i % 4;
                    put(
                        &mut img,
                        pitch,
                        ox + x,
                        oy + y,
                        [
                            raw[p + 2 * i + 1],
                            raw[p + 32 + 2 * i],
                            raw[p + 33 + 2 * i],
                            raw[p + 2 * i],
                        ],
                    );
                }
                p += 64;
                continue;
            }
            for y in 0..bh {
                let row = (oy + y) * pitch + ox;
                match format {
                    0 | 8 => {
                        for x in (0..8).step_by(2) {
                            let b = raw[p];
                            p += 1;
                            let hi = b >> 4;
                            let lo = b & 15;
                            if format == 0 {
                                let h = hi * 17;
                                let l = lo * 17;
                                img[row + x] = [h, h, h, 255];
                                img[row + x + 1] = [l, l, l, 255];
                            } else {
                                img[row + x] = palette
                                    .get((hi as usize).min(last))
                                    .copied()
                                    .unwrap_or([0, 0, 0, 0]);
                                img[row + x + 1] = palette
                                    .get((lo as usize).min(last))
                                    .copied()
                                    .unwrap_or([0, 0, 0, 0]);
                            }
                        }
                    }
                    1 | 9 => {
                        for x in 0..8 {
                            let b = raw[p];
                            p += 1;
                            img[row + x] = if format == 1 {
                                [b, b, b, 255]
                            } else {
                                palette
                                    .get((b as usize).min(last))
                                    .copied()
                                    .unwrap_or([0, 0, 0, 0])
                            };
                        }
                    }
                    2 => {
                        for x in 0..8 {
                            let b = raw[p];
                            p += 1;
                            let i = (b & 15) * 17;
                            img[row + x] = [i, i, i, (b >> 4) * 17];
                        }
                    }
                    3 => {
                        for x in 0..4 {
                            let a = raw[p];
                            let i = raw[p + 1];
                            p += 2;
                            img[row + x] = [i, i, i, a];
                        }
                    }
                    4 | 5 | 10 => {
                        for x in 0..4 {
                            let v = u16::from_be_bytes([raw[p], raw[p + 1]]);
                            p += 2;
                            img[row + x] = match format {
                                4 => rgb565(v),
                                5 => rgb5a3(v),
                                _ => palette
                                    .get(((v & 0x3fff) as usize).min(last))
                                    .copied()
                                    .unwrap_or([0, 0, 0, 0]),
                            };
                        }
                    }
                    _ => unreachable!(),
                }
            }
        }
    }
    let mut out = Vec::new();
    out.try_reserve_exact(out_len)
        .map_err(|_| TextureError::TooLarge)?;
    for y in 0..height {
        for x in 0..width {
            out.extend_from_slice(&img[y * pitch + x]);
        }
    }
    Ok(out)
}

fn be16(data: &[u8], at: usize) -> Result<u16, TextureError> {
    Ok(u16::from_be_bytes(
        data.get(at..at + 2)
            .ok_or(TextureError::Truncated)?
            .try_into()
            .unwrap(),
    ))
}
fn be32(data: &[u8], at: usize) -> Result<u32, TextureError> {
    Ok(u32::from_be_bytes(
        data.get(at..at + 4)
            .ok_or(TextureError::Truncated)?
            .try_into()
            .unwrap(),
    ))
}

/// Decode the mip chain in a BTI header at `header_offset`.
pub fn bti_levels(
    data: &[u8],
    header_offset: usize,
    all_levels: bool,
) -> Result<Vec<TextureLevel>, TextureError> {
    let header = data
        .get(
            header_offset
                ..header_offset
                    .checked_add(0x20)
                    .ok_or(TextureError::InvalidHeader)?,
        )
        .ok_or(TextureError::Truncated)?;
    let format = header[0];
    let mut width = be16(header, 2)?;
    let mut height = be16(header, 4)?;
    if width == 0 || height == 0 {
        return Err(TextureError::InvalidHeader);
    }
    let palette = if (8..=10).contains(&format) {
        let count = be16(header, 0x0a)? as usize;
        let offset = header_offset
            .checked_add(be32(header, 0x0c)? as usize)
            .ok_or(TextureError::InvalidHeader)?;
        let bytes = count
            .checked_mul(2)
            .and_then(|n| data.get(offset..offset.checked_add(n)?))
            .ok_or(TextureError::Truncated)?;
        decode_palette(header[9], bytes)?
    } else {
        Vec::new()
    };
    let mut offset = header_offset
        .checked_add(be32(header, 0x1c)? as usize)
        .ok_or(TextureError::InvalidHeader)?;
    let count = if all_levels {
        usize::from(header[0x18].max(1))
    } else {
        1
    };
    let mut levels = Vec::new();
    levels
        .try_reserve(count)
        .map_err(|_| TextureError::TooLarge)?;
    for _ in 0..count {
        let size = byte_size(format, width as usize, height as usize)?;
        let end = offset.checked_add(size).ok_or(TextureError::TooLarge)?;
        let bytes = data
            .get(offset..end)
            .unwrap_or_else(|| data.get(offset..).unwrap_or_default());
        levels.push(TextureLevel {
            width,
            height,
            rgba: decode(format, width as usize, height as usize, bytes, &palette)?,
        });
        offset = end;
        width /= 2;
        height /= 2;
        if width == 0 || height == 0 {
            break;
        }
    }
    Ok(levels)
}

/// Return offsets of the BTI headers in a J3D TEX1 section.
pub fn j3d_texture_headers(data: &[u8]) -> Result<Vec<usize>, TextureError> {
    if data.get(..4) != Some(b"J3D2") {
        return Ok(Vec::new());
    }
    let sections = be32(data, 0x0c)? as usize;
    let mut section = 0x20usize;
    for _ in 0..sections {
        let end = section.checked_add(8).ok_or(TextureError::InvalidHeader)?;
        let header = data.get(section..end).ok_or(TextureError::Truncated)?;
        let size = be32(header, 4)? as usize;
        if size < 8 {
            return Err(TextureError::InvalidHeader);
        }
        if &header[..4] == b"TEX1" {
            let count = be16(data, section + 8)? as usize;
            let base = section
                .checked_add(be32(data, section + 0x0c)? as usize)
                .ok_or(TextureError::InvalidHeader)?;
            let bytes = count
                .checked_mul(0x20)
                .and_then(|n| base.checked_add(n))
                .ok_or(TextureError::TooLarge)?;
            if bytes > data.len() {
                return Err(TextureError::Truncated);
            }
            return Ok((0..count).map(|i| base + i * 0x20).collect());
        }
        section = section.checked_add(size).ok_or(TextureError::TooLarge)?;
    }
    Ok(Vec::new())
}

fn texture_recipe() -> Result<serde_json::Value, TextureError> {
    let bytes = crate::asset_packs::get("textures/textures.pak", "meta.json")
        .ok_or(TextureError::InvalidHeader)?;
    serde_json::from_slice(bytes).map_err(|_| TextureError::InvalidHeader)
}

fn bake_texture_recipes() -> Result<serde_json::Value, TextureError> {
    let bytes = crate::asset_packs::get("textures/textures.pak", "bake_specs.json")
        .ok_or(TextureError::InvalidHeader)?;
    serde_json::from_slice(bytes).map_err(|_| TextureError::InvalidHeader)
}

/// Archives needed by source-backed texture image recipes.
pub fn simple_source_archives() -> Result<Vec<String>, TextureError> {
    SimpleImageGenerator::new()?.source_archives()
}

pub struct SimpleImageGenerator {
    meta: serde_json::Value,
    bake_specs: serde_json::Value,
}

impl SimpleImageGenerator {
    pub fn new() -> Result<Self, TextureError> {
        Ok(Self {
            meta: texture_recipe()?,
            bake_specs: bake_texture_recipes()?,
        })
    }

    pub fn source_archives(&self) -> Result<Vec<String>, TextureError> {
        source_archives_from_meta(&self.meta, &self.bake_specs)
    }

    pub fn geometry_source_archives(&self) -> Result<Vec<String>, TextureError> {
        geometry_source_archives(&self.meta, &self.bake_specs)
    }

    pub fn generate_archive(
        &self,
        archive_name: &str,
        archive: &[u8],
    ) -> Result<Vec<(Vec<String>, Vec<u8>)>, TextureError> {
        let mut output = generate_simple_archive(&self.meta, archive_name, archive)?;
        output.extend(generate_bake_archive(
            &self.meta,
            &self.bake_specs,
            archive_name,
            archive,
        )?);
        Ok(output)
    }
}

fn geometry_source_archives(
    meta: &serde_json::Value,
    bake_specs: &serde_json::Value,
) -> Result<Vec<String>, TextureError> {
    let images = meta
        .get("images")
        .and_then(|v| v.as_array())
        .ok_or(TextureError::InvalidHeader)?;
    let mut paths = std::collections::BTreeSet::new();
    for image in images {
        let pred = image.get("pred").ok_or(TextureError::InvalidHeader)?;
        if pred.get("op").and_then(|v| v.as_str()) != Some("bake") {
            continue;
        }
        let name = pred
            .get("spec")
            .and_then(|v| v.as_str())
            .ok_or(TextureError::InvalidHeader)?;
        let spec = bake_specs.get(name).ok_or(TextureError::InvalidHeader)?;
        if !matches!(
            spec.get("mode").and_then(|v| v.as_str()).unwrap_or("bake"),
            "bake"
                | "parts"
                | "stack"
                | "reproj"
                | "mariofire"
                | "beeparts"
                | "mariobee"
                | "ffwall"
        ) {
            continue;
        }
        let source = spec
            .get("src")
            .and_then(|v| v.as_str())
            .ok_or(TextureError::InvalidHeader)?;
        let (archive, _) = source.split_once(':').ok_or(TextureError::InvalidHeader)?;
        paths.insert(archive.to_owned());
        if let Some(source2) = spec.get("src2").and_then(|value| value.as_str()) {
            let (archive, _) = source2.split_once(':').ok_or(TextureError::InvalidHeader)?;
            paths.insert(archive.to_owned());
        }
        if spec.get("mode").and_then(|v| v.as_str()) == Some("ffwall") {
            let placement = spec
                .get("placement")
                .and_then(|v| v.as_array())
                .filter(|v| v.len() == 5)
                .ok_or(TextureError::InvalidHeader)?;
            for index in 0..2 {
                let name = placement[index]
                    .as_str()
                    .ok_or(TextureError::InvalidHeader)?;
                paths.insert(format!("StageData/{name}.arc"));
            }
        }
        for key in ["pose", "bva", "brk", "btp"] {
            if let Some(animation) = spec.get(key).and_then(|v| v.as_array()) {
                let path = animation
                    .first()
                    .and_then(|v| v.as_str())
                    .ok_or(TextureError::InvalidHeader)?;
                let (archive, _) = path.split_once(':').ok_or(TextureError::InvalidHeader)?;
                paths.insert(archive.to_owned());
            }
        }
    }
    Ok(paths.into_iter().collect())
}

/// Build geometry-backed image recipes after generated figure OBJ files exist.
/// BCK poses, BVA visibility, BRK register animation, and kmax part IDs are
/// sampled before the shared geometry baker runs.
#[cfg(feature = "processor")]
pub fn generate_geometry_bakes(
    archives: &std::collections::BTreeMap<String, Vec<u8>>,
    figure_files: &[(String, Vec<u8>)],
    generated_files: &[(String, Vec<u8>)],
    main_dol: &[u8],
) -> Result<Vec<(Vec<String>, Vec<u8>)>, TextureError> {
    let generator = SimpleImageGenerator::new()?;
    let mut output = Vec::new();
    for (archive_name, archive) in archives {
        output.extend(generate_geometry_bake_archive(
            &generator.meta,
            &generator.bake_specs,
            archives,
            archive_name,
            archive,
            figure_files,
            generated_files,
            main_dol,
        )?);
    }
    Ok(output)
}

#[cfg(feature = "processor")]
fn generate_geometry_bake_archive(
    meta: &serde_json::Value,
    bake_specs: &serde_json::Value,
    archives: &std::collections::BTreeMap<String, Vec<u8>>,
    archive_name: &str,
    archive_bytes: &[u8],
    figure_files: &[(String, Vec<u8>)],
    generated_files: &[(String, Vec<u8>)],
    main_dol: &[u8],
) -> Result<Vec<(Vec<String>, Vec<u8>)>, TextureError> {
    use crate::geometry_bake::{
        build_bake_materials, parse_obj, prepare_high_mesh_with_pose_and_visibility,
        serialize_bake_job, BakeOptions,
    };
    use crate::j3d_model::J3dModel;
    use std::collections::{BTreeMap, HashMap};

    const FLAG_PARTS: i32 = 65_536;
    const FLAG_CUT: i32 = 131_072;
    const FLAG_FAR: i32 = 2_097_152;
    const FLAG_PART_IDS: i32 = 1_048_576;
    const FLAG_REPROJECT: i32 = 4_194_304;
    const FLAG_NEAR_MAP: i32 = 8_388_608;
    const FLAG_RAYCAST: i32 = 33_554_432;

    let images = meta
        .get("images")
        .and_then(|v| v.as_array())
        .ok_or(TextureError::InvalidHeader)?;
    let object_files: HashMap<&str, &[u8]> = figure_files
        .iter()
        .chain(generated_files.iter())
        .map(|(path, bytes)| (path.as_str(), bytes.as_slice()))
        .collect();
    let mut models = BTreeMap::<String, (J3dModel, crate::tev::TevModel)>::new();
    let mut result = Vec::new();

    for (image_index, image) in images.iter().enumerate() {
        let pred = image.get("pred").ok_or(TextureError::InvalidHeader)?;
        if pred.get("op").and_then(|v| v.as_str()) != Some("bake") {
            continue;
        }
        let bake_error =
            |stage: u32| TextureError::GeometryBake(10_000 + image_index as u32 * 32 + stage);
        let recipe = pred
            .get("spec")
            .and_then(|v| v.as_str())
            .ok_or(TextureError::InvalidHeader)?;
        let spec = bake_specs.get(recipe).ok_or(TextureError::InvalidHeader)?;
        let mode = spec.get("mode").and_then(|v| v.as_str()).unwrap_or("bake");
        if mode == "ffwall" {
            let source = spec
                .get("src")
                .and_then(|v| v.as_str())
                .ok_or(TextureError::InvalidHeader)?;
            let (source_archive, _) = source.split_once(':').ok_or(TextureError::InvalidHeader)?;
            if source_archive != archive_name {
                continue;
            }
            let obj_name = spec
                .get("obj")
                .and_then(|v| v.as_str())
                .ok_or(TextureError::InvalidHeader)?;
            let obj_path = format!("assets/{obj_name}");
            if !figure_files
                .iter()
                .chain(generated_files.iter())
                .any(|(path, _)| path == &obj_path)
            {
                // Stage OBJ generation runs before these recipes in the native
                // pipeline. Keep this mode deferred until the WASM stage
                // converter supplies that OBJ instead of failing conversion.
                continue;
            }
            let generated = generate_ffwall_image(
                image_index,
                image,
                spec,
                archives,
                archive_bytes,
                figure_files,
                generated_files,
            )
            .map_err(|error| match error {
                TextureError::GeometryBake(diagnostic) => TextureError::GeometryBake(diagnostic),
                _ => bake_error(31),
            })?;
            result.push(generated);
            continue;
        }
        if mode == "beeparts" {
            let source = spec
                .get("src")
                .and_then(|value| value.as_str())
                .ok_or(TextureError::InvalidHeader)?;
            let (source_archive, source_inner) =
                source.split_once(':').ok_or(TextureError::InvalidHeader)?;
            if source_archive != archive_name {
                continue;
            }
            let source2 = spec
                .get("src2")
                .and_then(|value| value.as_str())
                .ok_or(TextureError::InvalidHeader)?;
            let (source2_archive, source2_inner) =
                source2.split_once(':').ok_or(TextureError::InvalidHeader)?;
            let second_archive = archives
                .get(source2_archive)
                .ok_or_else(|| bake_error(28))?;
            let wing = crate::asset_formats::rarc_find(archive_bytes, source_inner)
                .map_err(|_| bake_error(29))?;
            let body = crate::asset_formats::rarc_find(second_archive, source2_inner)
                .map_err(|_| bake_error(30))?;
            let signs = spec.get("signs").ok_or(TextureError::InvalidHeader)?;
            let mut rgba =
                crate::bee_texture::parts_atlas(&wing, &body, signs).map_err(|_| bake_error(31))?;
            let width = image
                .get("w")
                .and_then(|value| value.as_u64())
                .ok_or(TextureError::InvalidHeader)? as usize;
            let height = image
                .get("h")
                .and_then(|value| value.as_u64())
                .ok_or(TextureError::InvalidHeader)? as usize;
            if width != 32 || height != 32 || rgba.len() != width * height * 4 {
                return Err(bake_error(31));
            }
            apply_recipe_residual(image_index, image, &mut rgba, 4)?;
            let png = encode_png(width, height, 6, 4, &rgba)?;
            let paths = image
                .get("files")
                .and_then(|value| value.as_array())
                .ok_or(TextureError::InvalidHeader)?
                .iter()
                .map(|file| {
                    file.get("path")
                        .and_then(|value| value.as_str())
                        .map(str::to_owned)
                        .ok_or(TextureError::InvalidHeader)
                })
                .collect::<Result<Vec<_>, _>>()?;
            result.push((paths, png));
            continue;
        }
        if mode == "mariobee" {
            let source = spec
                .get("src")
                .and_then(|value| value.as_str())
                .ok_or(TextureError::InvalidHeader)?;
            let (source_archive, source_inner) =
                source.split_once(':').ok_or(TextureError::InvalidHeader)?;
            if source_archive != archive_name {
                continue;
            }
            let source2 = spec
                .get("src2")
                .and_then(|value| value.as_str())
                .ok_or(TextureError::InvalidHeader)?;
            let (source2_archive, source2_inner) =
                source2.split_once(':').ok_or(TextureError::InvalidHeader)?;
            let second_archive = archives
                .get(source2_archive)
                .ok_or_else(|| bake_error(28))?;
            let bee = crate::asset_formats::rarc_find(archive_bytes, source_inner)
                .map_err(|_| bake_error(29))?;
            let mario = crate::asset_formats::rarc_find(second_archive, source2_inner)
                .map_err(|_| bake_error(29))?;
            let obj = spec
                .get("obj")
                .and_then(|value| value.as_str())
                .ok_or(TextureError::InvalidHeader)?;
            let obj_path = format!("assets/{obj}");
            let obj_bytes = object_files
                .get(obj_path.as_str())
                .copied()
                .ok_or_else(|| bake_error(30))?;
            let base = spec
                .get("base")
                .and_then(|value| value.as_str())
                .ok_or(TextureError::InvalidHeader)?;
            let base_path = format!("assets/{base}");
            let (base_width, base_height, base_pixels) =
                if let Some(base_png) = object_files.get(base_path.as_str()).copied() {
                    decode_rgba_png(base_png)?
                } else {
                    let (base_image_index, base_image) = images
                        .iter()
                        .enumerate()
                        .find(|(_, candidate)| {
                            candidate
                                .get("files")
                                .and_then(|value| value.as_array())
                                .is_some_and(|files| {
                                    files.iter().any(|file| {
                                        file.get("path").and_then(|value| value.as_str())
                                            == Some(base_path.as_str())
                                    })
                                })
                        })
                        .ok_or_else(|| bake_error(30))?;
                    let base_recipe = base_image
                        .get("pred")
                        .and_then(|value| value.get("spec"))
                        .and_then(|value| value.as_str())
                        .ok_or_else(|| bake_error(30))?;
                    let base_spec = bake_specs.get(base_recipe).ok_or_else(|| bake_error(30))?;
                    let base_source = base_spec
                        .get("src")
                        .and_then(|value| value.as_str())
                        .ok_or_else(|| bake_error(30))?;
                    let (base_archive_name, _) =
                        base_source.split_once(':').ok_or_else(|| bake_error(30))?;
                    let base_archive = archives
                        .get(base_archive_name)
                        .ok_or_else(|| bake_error(28))?;
                    let mut base_request = base_image.clone();
                    base_request
                        .as_object_mut()
                        .ok_or(TextureError::InvalidHeader)?
                        .insert("skip_residual".into(), serde_json::Value::Bool(true));
                    let base_meta = serde_json::json!({ "images": [base_request] });
                    let rendered = generate_geometry_bake_archive(
                        &base_meta,
                        bake_specs,
                        archives,
                        base_archive_name,
                        base_archive,
                        figure_files,
                        generated_files,
                        main_dol,
                    )?;
                    let base_png = rendered
                        .iter()
                        .find(|(paths, _)| paths.iter().any(|path| path == &base_path))
                        .map(|(_, bytes)| bytes.as_slice())
                        .ok_or_else(|| bake_error(30))?;
                    let (width, height, mut pixels) = decode_rgba_png(base_png)?;
                    apply_recipe_residual(base_image_index, base_image, &mut pixels, 4)?;
                    (width, height, pixels)
                };
            let material = spec
                .get("mat")
                .and_then(|value| value.as_str())
                .ok_or(TextureError::InvalidHeader)?;
            let mut rgba =
                crate::bee_texture::suit_texture(&bee, &mario, obj_bytes, material, &base_pixels)
                    .map_err(|_| bake_error(31))?;
            let width = image
                .get("w")
                .and_then(|value| value.as_u64())
                .ok_or(TextureError::InvalidHeader)? as usize;
            let height = image
                .get("h")
                .and_then(|value| value.as_u64())
                .ok_or(TextureError::InvalidHeader)? as usize;
            if base_width != width || base_height != height || rgba.len() != width * height * 4 {
                return Err(bake_error(31));
            }
            apply_recipe_residual(image_index, image, &mut rgba, 4)?;
            let png = encode_png(width, height, 6, 4, &rgba)?;
            let paths = image
                .get("files")
                .and_then(|value| value.as_array())
                .ok_or(TextureError::InvalidHeader)?
                .iter()
                .map(|file| {
                    file.get("path")
                        .and_then(|value| value.as_str())
                        .map(str::to_owned)
                        .ok_or(TextureError::InvalidHeader)
                })
                .collect::<Result<Vec<_>, _>>()?;
            result.push((paths, png));
            continue;
        }
        if mode == "stack" {
            let source = spec
                .get("src")
                .and_then(|v| v.as_str())
                .ok_or(TextureError::InvalidHeader)?;
            let (source_archive, _) = source.split_once(':').ok_or(TextureError::InvalidHeader)?;
            if source_archive != archive_name {
                continue;
            }
            let image_width = image
                .get("w")
                .and_then(|v| v.as_u64())
                .ok_or(TextureError::InvalidHeader)? as usize;
            let image_height = image
                .get("h")
                .and_then(|v| v.as_u64())
                .ok_or(TextureError::InvalidHeader)? as usize;
            let layers = spec
                .get("layers")
                .and_then(|v| v.as_array())
                .ok_or(TextureError::InvalidHeader)?;
            let mut composed = None::<Vec<u8>>;
            for (layer_index, layer) in layers.iter().enumerate() {
                let mut layer_spec = spec
                    .as_object()
                    .cloned()
                    .ok_or(TextureError::InvalidHeader)?;
                layer_spec.remove("layers");
                layer_spec.insert("mode".into(), serde_json::Value::String("parts".into()));
                let layer_values = layer.as_object().ok_or(TextureError::InvalidHeader)?;
                for (key, value) in layer_values {
                    layer_spec.insert(key.clone(), value.clone());
                }
                let layer_recipe = format!("{recipe}#layer{layer_index}");
                let layer_specs = serde_json::Value::Object(
                    [(layer_recipe.clone(), serde_json::Value::Object(layer_spec))]
                        .into_iter()
                        .collect(),
                );
                let mut layer_image = image.clone();
                layer_image
                    .get_mut("pred")
                    .and_then(|value| value.as_object_mut())
                    .ok_or(TextureError::InvalidHeader)?
                    .insert("spec".into(), serde_json::Value::String(layer_recipe));
                layer_image
                    .as_object_mut()
                    .ok_or(TextureError::InvalidHeader)?
                    .insert("skip_residual".into(), serde_json::Value::Bool(true));
                let layer_meta = serde_json::json!({ "images": [layer_image] });
                let rendered = generate_geometry_bake_archive(
                    &layer_meta,
                    &layer_specs,
                    archives,
                    archive_name,
                    archive_bytes,
                    figure_files,
                    generated_files,
                    main_dol,
                )?;
                let image_path = image
                    .get("files")
                    .and_then(|v| v.as_array())
                    .and_then(|files| files.first())
                    .and_then(|file| file.get("path"))
                    .and_then(|value| value.as_str())
                    .ok_or(TextureError::InvalidHeader)?;
                let png = rendered
                    .iter()
                    .find(|(paths, _)| paths.iter().any(|path| path == image_path))
                    .map(|(_, bytes)| bytes.as_slice())
                    .ok_or(TextureError::InvalidHeader)?;
                let (width, height, pixels) = decode_rgba_png(png)?;
                if width != image_width || height != image_height {
                    return Err(TextureError::InvalidHeader);
                }
                if composed.is_none() {
                    composed = Some(pixels);
                    continue;
                }
                let output = composed.as_mut().ok_or(TextureError::InvalidHeader)?;
                let rect = layer
                    .get("rect")
                    .and_then(|value| value.as_array())
                    .map(|values| {
                        values
                            .iter()
                            .map(|value| {
                                value
                                    .as_u64()
                                    .map(|number| number as usize)
                                    .ok_or(TextureError::InvalidHeader)
                            })
                            .collect::<Result<Vec<_>, _>>()
                    })
                    .transpose()?;
                if rect.as_ref().is_some_and(|values| values.len() != 4) {
                    return Err(TextureError::InvalidHeader);
                }
                let [x0, y0, x1, y1] = rect
                    .as_deref()
                    .unwrap_or(&[0, 0, image_width, image_height])
                    .try_into()
                    .map_err(|_| TextureError::InvalidHeader)?;
                if x1 > image_width || y1 > image_height || x0 > x1 || y0 > y1 {
                    return Err(TextureError::InvalidHeader);
                }
                let cut = layer
                    .get("cut")
                    .and_then(|value| value.as_bool())
                    .unwrap_or(false);
                for y in y0..y1 {
                    for x in x0..x1 {
                        let offset = (y * image_width + x) * 4;
                        if !cut || pixels[offset + 3] >= 128 {
                            output[offset..offset + 3].copy_from_slice(&pixels[offset..offset + 3]);
                        }
                    }
                }
            }
            let mut rgba = composed.ok_or(TextureError::InvalidHeader)?;
            for alpha in rgba.iter_mut().skip(3).step_by(4) {
                *alpha = 255;
            }
            let channels = match image.get("ct").and_then(|v| v.as_u64()) {
                Some(2) => 3,
                Some(6) => 4,
                _ => return Err(TextureError::InvalidHeader),
            };
            apply_recipe_residual(image_index, image, &mut rgba, channels)?;
            let mut pixels = Vec::with_capacity(
                image_width
                    .checked_mul(image_height)
                    .and_then(|n| n.checked_mul(channels))
                    .ok_or(TextureError::TooLarge)?,
            );
            for pixel in rgba.chunks_exact(4) {
                pixels.extend_from_slice(&pixel[..channels]);
            }
            let png = encode_png(
                image_width,
                image_height,
                if channels == 4 { 6 } else { 2 },
                channels,
                &pixels,
            )?;
            let paths = image
                .get("files")
                .and_then(|v| v.as_array())
                .ok_or(TextureError::InvalidHeader)?
                .iter()
                .map(|file| {
                    file.get("path")
                        .and_then(|value| value.as_str())
                        .map(str::to_owned)
                        .ok_or(TextureError::InvalidHeader)
                })
                .collect::<Result<Vec<_>, _>>()?;
            result.push((paths, png));
            continue;
        }
        if !matches!(mode, "bake" | "parts" | "reproj" | "mariofire") {
            continue;
        }
        // BPK colors remain deferred; BTP texture slots, BRK registers, and
        // DOL-backed material colors are applied below.
        if spec.get("bpk").is_some() {
            continue;
        }
        let source = spec
            .get("src")
            .and_then(|v| v.as_str())
            .ok_or(TextureError::InvalidHeader)?;
        let (source_archive, inner) = source.split_once(':').ok_or(TextureError::InvalidHeader)?;
        if source_archive != archive_name {
            continue;
        }
        let obj = spec
            .get("obj")
            .and_then(|v| v.as_str())
            .ok_or(TextureError::InvalidHeader)?;
        let obj_path = format!("assets/{obj}");
        let Some(obj_bytes) = object_files.get(obj_path.as_str()).copied() else {
            continue;
        };

        if !models.contains_key(inner) {
            let body =
                crate::asset_formats::rarc_find(archive_bytes, inner).map_err(|_| bake_error(1))?;
            let model = J3dModel::parse(&body).map_err(|_| bake_error(2))?;
            let tev = crate::tev::TevModel::parse(&body).map_err(|_| bake_error(3))?;
            models.insert(inner.to_owned(), (model, tev));
        }
        let (model, base_tev) = models.get(inner).ok_or(TextureError::InvalidHeader)?;
        let mut tev = base_tev.clone();
        if let Some(values) = spec.get("brk").and_then(|v| v.as_array()) {
            if values.len() != 2 {
                return Err(bake_error(12));
            }
            let animation_path = values[0].as_str().ok_or_else(|| bake_error(12))?;
            let frame = values[1].as_f64().ok_or_else(|| bake_error(12))?;
            let (animation_archive_name, animation_inner) = animation_path
                .split_once(':')
                .ok_or_else(|| bake_error(12))?;
            let animation_archive = archives
                .get(animation_archive_name)
                .ok_or_else(|| bake_error(20))?;
            let animation = crate::asset_formats::rarc_find(animation_archive, animation_inner)
                .map_err(|_| bake_error(20))?;
            let registers = crate::j3d_animation::brk_registers(&animation, frame)
                .map_err(|_| bake_error(20))?;
            tev.apply_brk_registers(&registers);
        }
        if let Some(values) = spec.get("btp").and_then(|v| v.as_array()) {
            if values.len() != 2 {
                return Err(bake_error(12));
            }
            let animation_path = values[0].as_str().ok_or_else(|| bake_error(12))?;
            let frame = values[1].as_i64().ok_or_else(|| bake_error(12))?.max(0) as usize;
            let (animation_archive_name, animation_inner) = animation_path
                .split_once(':')
                .ok_or_else(|| bake_error(12))?;
            let animation_archive = archives
                .get(animation_archive_name)
                .ok_or_else(|| bake_error(21))?;
            let animation = crate::asset_formats::rarc_find(animation_archive, animation_inner)
                .map_err(|_| bake_error(21))?;
            let textures = crate::j3d_animation::btp_textures(&animation, frame)
                .map_err(|_| bake_error(21))?;
            tev.apply_btp_textures(&textures)
                .map_err(|_| bake_error(21))?;
        }
        if let Some(values) = spec.get("dolcol").and_then(|v| v.as_array()) {
            if values.len() != 3 {
                return Err(bake_error(12));
            }
            let material = values[0].as_str().ok_or_else(|| bake_error(12))?;
            let offset = values[1].as_u64().ok_or_else(|| bake_error(12))? as usize;
            let index = values[2].as_u64().ok_or_else(|| bake_error(12))? as usize;
            let start = offset
                .checked_add(index.checked_mul(4).ok_or_else(|| bake_error(12))?)
                .ok_or_else(|| bake_error(12))?;
            let rgba: [u8; 4] = main_dol
                .get(start..start.checked_add(4).ok_or_else(|| bake_error(12))?)
                .ok_or_else(|| bake_error(22))?
                .try_into()
                .map_err(|_| bake_error(22))?;
            tev.apply_material_color(material, rgba);
        }
        let fitted_pose = if let Some(values) = spec.get("pose").and_then(|v| v.as_array()) {
            if values.len() != 2 {
                return Err(bake_error(12));
            }
            let animation_path = values[0].as_str().ok_or_else(|| bake_error(12))?;
            let frame = values[1].as_f64().ok_or_else(|| bake_error(12))?;
            let (animation_archive_name, animation_inner) = animation_path
                .split_once(':')
                .ok_or_else(|| bake_error(12))?;
            let animation_archive = archives
                .get(animation_archive_name)
                .ok_or_else(|| bake_error(19))?;
            let animation = crate::asset_formats::rarc_find(animation_archive, animation_inner)
                .map_err(|_| bake_error(19))?;
            let bck = crate::j3d_animation::Bck::parse(&animation).map_err(|_| bake_error(19))?;
            let raw = bck.raw(model, frame);
            Some(crate::j3d_animation::fitted_world(model, &raw))
        } else {
            None
        };
        let visible_shapes = if let Some(values) = spec.get("bva").and_then(|v| v.as_array()) {
            if values.len() != 2 {
                return Err(bake_error(12));
            }
            let animation_path = values[0].as_str().ok_or_else(|| bake_error(12))?;
            let frame = values[1].as_i64().ok_or_else(|| bake_error(12))?.max(0) as usize;
            let (animation_archive_name, animation_inner) = animation_path
                .split_once(':')
                .ok_or_else(|| bake_error(12))?;
            let animation_archive = archives
                .get(animation_archive_name)
                .ok_or_else(|| bake_error(18))?;
            let animation = crate::asset_formats::rarc_find(animation_archive, animation_inner)
                .map_err(|_| bake_error(18))?;
            let channels =
                crate::j3d_animation::bva_visibility(&animation).map_err(|_| bake_error(18))?;
            let mut visible = vec![false; model.shapes.len()];
            for (shape, frames) in channels.iter().enumerate().take(visible.len()) {
                if let Some(&state) = frames.get(frame.min(frames.len().saturating_sub(1))) {
                    visible[shape] = state != 0;
                }
            }
            Some(visible)
        } else {
            None
        };
        let excluded = spec
            .get("excl")
            .and_then(|v| v.as_array())
            .map(|items| {
                items
                    .iter()
                    .map(|item| {
                        item.as_str()
                            .map(str::to_owned)
                            .ok_or(TextureError::InvalidHeader)
                    })
                    .collect::<Result<Vec<_>, _>>()
            })
            .transpose()
            .map_err(|_| bake_error(12))?
            .unwrap_or_default();
        let apply_fix = mode == "bake"
            && spec.get("pose").is_none()
            && !spec
                .get("static")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
        let mut high = prepare_high_mesh_with_pose_and_visibility(
            model,
            &tev,
            apply_fix,
            &excluded,
            fitted_pose.as_deref(),
            visible_shapes.as_deref(),
        )
        .map_err(|_| bake_error(5))?;
        let material = spec.get("mat").and_then(|v| v.as_str());
        let mut low = parse_obj(obj_bytes, material).map_err(|_| bake_error(4))?;

        // Native parts() recipes with a pose use the posed high mesh only to
        // map the posed low OBJ back to rest space. The actual bake then uses
        // the rest high mesh and its rest-space part IDs.
        if mode == "parts" {
            if let Some(pose) = fitted_pose.as_deref() {
                crate::geometry_bake::unpose_low_vertices(model, &high, pose, &mut low.positions)
                    .map_err(|_| bake_error(5))?;
                high = prepare_high_mesh_with_pose_and_visibility(
                    model,
                    &tev,
                    false,
                    &excluded,
                    None,
                    visible_shapes.as_deref(),
                )
                .map_err(|_| bake_error(5))?;
            }
        }

        if let Some(values) = spec.get("lowv").and_then(|v| v.as_array()) {
            let mut positions = Vec::with_capacity(values.len());
            for value in values {
                let xyz = value
                    .as_array()
                    .filter(|a| a.len() == 3)
                    .ok_or_else(|| bake_error(12))?;
                positions.push([
                    xyz[0].as_f64().ok_or_else(|| bake_error(12))? / 0.004,
                    xyz[1].as_f64().ok_or_else(|| bake_error(12))? / 0.004,
                    xyz[2].as_f64().ok_or_else(|| bake_error(12))? / 0.004,
                ]);
            }
            low.faces
                .retain(|face| face.iter().all(|(index, _)| *index < positions.len()));
            low.positions = positions;
        }
        if let Some(values) = spec.get("lowxf").and_then(|v| v.as_array()) {
            if values.len() != 5 {
                return Err(bake_error(12));
            }
            let [cx, cy, sx, sy, ty] = values
                .iter()
                .map(|v| v.as_f64().ok_or_else(|| bake_error(12)))
                .collect::<Result<Vec<_>, _>>()
                .map_err(|_| bake_error(12))?
                .try_into()
                .map_err(|_| bake_error(12))?;
            for point in &mut low.positions {
                point[0] = (point[0] - cx) * sx;
                point[1] = (point[1] - cy) * sy + cy + ty;
            }
        }
        if spec.get("flip").and_then(|v| v.as_bool()).unwrap_or(false) {
            for face in &mut low.faces {
                face.reverse();
            }
        }

        let raw = spec.get("raw").and_then(|v| v.as_bool()).unwrap_or(false);
        let stage_limit = spec
            .get("stages")
            .and_then(|v| v.as_u64())
            .map(|v| v as usize);
        let (materials, textures) =
            build_bake_materials(&tev, &high, raw || mode == "reproj", stage_limit)
                .map_err(|_| bake_error(6))?;
        let bake_size = if matches!(mode, "reproj" | "mariofire") {
            image
                .get("w")
                .and_then(|v| v.as_u64())
                .ok_or_else(|| bake_error(12))? as usize
        } else {
            spec.get("bake").and_then(|v| v.as_u64()).unwrap_or(64) as usize
        };
        let bake_height = if matches!(mode, "reproj" | "mariofire") {
            image
                .get("h")
                .and_then(|v| v.as_u64())
                .ok_or_else(|| bake_error(12))? as usize
        } else {
            bake_size
        };
        let part_mode = mode == "parts";
        let width = if part_mode { 64 } else { bake_size };
        let height = if part_mode { 64 } else { bake_height };
        let aa = if !part_mode && width == 64 {
            decode_aa64()
        } else {
            Vec::new()
        };
        let mut flags = if part_mode { FLAG_PARTS } else { 0 };
        if part_mode && spec.get("cut").and_then(|v| v.as_bool()).unwrap_or(false) {
            flags |= FLAG_CUT;
        }
        if part_mode && spec.get("far").and_then(|v| v.as_bool()).unwrap_or(false) {
            flags |= FLAG_FAR;
        }
        if mode == "reproj" {
            flags |= FLAG_REPROJECT;
            if spec.get("ray").and_then(|v| v.as_bool()).unwrap_or(false) {
                flags |= FLAG_RAYCAST;
            }
        }
        let part_ids = if part_mode {
            if let Some(kmax) = spec.get("kmax").and_then(|v| v.as_u64()) {
                flags |= FLAG_PART_IDS;
                Some(
                    crate::geometry_parts::part_ids(model, &high, kmax as usize)
                        .map_err(|_| bake_error(17))?,
                )
            } else {
                None
            }
        } else {
            None
        };
        let extf = spec.get("extf").and_then(|v| v.as_f64()).unwrap_or(0.06);
        let margin = if part_mode {
            0
        } else if mode == "reproj" {
            height as i32
        } else {
            4
        };
        let mut payload = serialize_bake_job(
            &low,
            &high,
            &materials,
            &textures,
            BakeOptions {
                width,
                height,
                margin,
                flags,
                extent_factor: extf,
            },
            &aa,
        )
        .map_err(|_| bake_error(7))?;
        if let Some(ids) = part_ids {
            for id in ids {
                payload.extend_from_slice(&id.to_le_bytes());
            }
        }
        let rendered = crate::textures_bake_core::run(&payload).map_err(|error| {
            bake_error(match error {
                crate::textures_bake_core::BakeCoreError::InvalidInput => 13,
                crate::textures_bake_core::BakeCoreError::ArenaExhausted => 14,
                crate::textures_bake_core::BakeCoreError::HostAllocation => 15,
                crate::textures_bake_core::BakeCoreError::InvalidOutput => 16,
            })
        })?;

        let (target_width, target_height, mut rgba) = if mode == "mariofire" {
            let base_name = spec
                .get("base")
                .and_then(|value| value.as_str())
                .ok_or_else(|| bake_error(12))?;
            let base_path = format!("assets/{base_name}");
            let (base_image_index, base_image) = images
                .iter()
                .enumerate()
                .find(|(_, candidate)| {
                    candidate
                        .get("files")
                        .and_then(|value| value.as_array())
                        .is_some_and(|files| {
                            files.iter().any(|file| {
                                file.get("path").and_then(|value| value.as_str())
                                    == Some(base_path.as_str())
                            })
                        })
                })
                .ok_or_else(|| bake_error(24))?;
            let (base_width, base_height, mut pixels) =
                if let Some(base_png) = object_files.get(base_path.as_str()).copied() {
                    decode_rgba_png(base_png)?
                } else {
                    let mut base_request = base_image.clone();
                    base_request
                        .as_object_mut()
                        .ok_or(TextureError::InvalidHeader)?
                        .insert("skip_residual".into(), serde_json::Value::Bool(true));
                    let base_meta = serde_json::json!({ "images": [base_request] });
                    let rendered = generate_geometry_bake_archive(
                        &base_meta,
                        bake_specs,
                        archives,
                        archive_name,
                        archive_bytes,
                        figure_files,
                        generated_files,
                        main_dol,
                    )?;
                    let base_png = rendered
                        .iter()
                        .find(|(paths, _)| paths.iter().any(|path| path == &base_path))
                        .map(|(_, bytes)| bytes.as_slice())
                        .ok_or_else(|| bake_error(26))?;
                    let (width, height, mut rgba) = decode_rgba_png(base_png)?;
                    apply_recipe_residual(base_image_index, base_image, &mut rgba, 4)?;
                    (width, height, rgba)
                };
            if base_width != width || base_height != height {
                return Err(bake_error(27));
            }
            let source_materials = high.materials.clone();
            let source_triangle_materials = high.triangle_materials.clone();
            high.triangle_materials.fill(0);
            high.materials = vec![0];
            let plain = crate::geometry_bake::BakedMaterial {
                texture: -1,
                wrap_s: 1,
                wrap_t: 1,
                alpha_mode: 0,
                cutoff: 0.5,
                factor: [1.0; 4],
                transform: None,
            };
            let payload = serialize_bake_job(
                &low,
                &high,
                &[plain],
                &[],
                BakeOptions {
                    width,
                    height,
                    margin: height as i32,
                    flags: FLAG_NEAR_MAP,
                    extent_factor: extf,
                },
                &[],
            )
            .map_err(|_| bake_error(7))?;
            let mapped = crate::textures_bake_core::run(&payload).map_err(|error| {
                bake_error(match error {
                    crate::textures_bake_core::BakeCoreError::InvalidInput => 13,
                    crate::textures_bake_core::BakeCoreError::ArenaExhausted => 14,
                    crate::textures_bake_core::BakeCoreError::HostAllocation => 15,
                    crate::textures_bake_core::BakeCoreError::InvalidOutput => 16,
                })
            })?;
            let count = u32::from_le_bytes(
                mapped
                    .get(..4)
                    .ok_or(TextureError::Truncated)?
                    .try_into()
                    .unwrap(),
            ) as usize;
            let record_bytes = count.checked_mul(20).ok_or(TextureError::TooLarge)?;
            if mapped.len() != 4 + record_bytes {
                return Err(TextureError::InvalidHeader);
            }
            let mut material_textures =
                HashMap::<usize, (usize, usize, Vec<[f64; 4]>, Vec<[f64; 4]>)>::new();
            let mut accumulation = vec![[0.0f64; 4]; width * height];
            let mut counts = vec![0u32; width * height];
            let material_name = spec
                .get("mat")
                .and_then(|value| value.as_str())
                .ok_or_else(|| bake_error(12))?;
            let material_bytes = material_name.as_bytes();
            if material_bytes.is_empty() {
                return Err(bake_error(12));
            }
            for record in mapped[4..].chunks_exact(20) {
                let float =
                    |at: usize| f32::from_le_bytes(record[at..at + 4].try_into().unwrap()) as f64;
                let pixel = float(0) as usize;
                let triangle = float(4) as usize;
                if pixel >= width * height || triangle >= high.triangles.len() {
                    return Err(TextureError::InvalidHeader);
                }
                let compact_material = source_triangle_materials
                    .get(triangle)
                    .copied()
                    .ok_or(TextureError::InvalidHeader)?
                    as usize;
                let source_material = *source_materials
                    .get(compact_material)
                    .ok_or(TextureError::InvalidHeader)?;
                let (texture_width, texture_height, normal, fire) =
                    if let Some(texture) = material_textures.get(&source_material) {
                        texture
                    } else {
                        let name = base_tev
                            .material_name(source_material)
                            .map_err(|_| bake_error(25))?;
                        let changed = tev
                            .material_name(source_material)
                            .map_err(|_| bake_error(25))?;
                        if name != changed {
                            return Err(bake_error(25));
                        }
                        let (normal_width, normal_height, normal) = base_tev
                            .effective_pixels_with_stages(source_material, None)
                            .map_err(|_| bake_error(25))?;
                        let (fire_width, fire_height, fire) = tev
                            .effective_pixels_with_stages(source_material, None)
                            .map_err(|_| bake_error(25))?;
                        if normal_width != fire_width || normal_height != fire_height {
                            return Err(bake_error(25));
                        }
                        material_textures
                            .insert(source_material, (normal_width, normal_height, normal, fire));
                        material_textures
                            .get(&source_material)
                            .ok_or(TextureError::InvalidHeader)?
                    };
                let [a, b, c] = high.triangles[triangle].map(|index| index as usize);
                let [u0, v0] = *high.uvs.get(a).ok_or(TextureError::InvalidHeader)?;
                let [u1, v1] = *high.uvs.get(b).ok_or(TextureError::InvalidHeader)?;
                let [u2, v2] = *high.uvs.get(c).ok_or(TextureError::InvalidHeader)?;
                let w0 = float(8);
                let w1 = float(12);
                let w2 = float(16);
                let u = w0 * u0 + w1 * u1 + w2 * u2;
                let v = w0 * v0 + w1 * v1 + w2 * v2;
                let x = ((u.rem_euclid(1.0) * *texture_width as f64) as usize)
                    .min(texture_width.saturating_sub(1));
                let y = ((v.rem_euclid(1.0) * *texture_height as f64) as usize)
                    .min(texture_height.saturating_sub(1));
                let source = y * texture_width + x;
                for channel in 0..4 {
                    accumulation[pixel][channel] += fire[source][channel] - normal[source][channel];
                }
                counts[pixel] += 1;
            }
            for (pixel, color) in pixels.chunks_exact_mut(4).enumerate() {
                if counts[pixel] == 0 {
                    continue;
                }
                for channel in 0..3 {
                    let value = (color[channel] as f64 / 255.0
                        + accumulation[pixel][channel] / counts[pixel] as f64)
                        .clamp(0.0, 1.0)
                        * 255.0;
                    let floor = value.floor();
                    let fraction = value - floor;
                    let rounded = if fraction > 0.5 || (fraction == 0.5 && floor as u32 % 2 != 0) {
                        floor + 1.0
                    } else {
                        floor
                    };
                    color[channel] = rounded as u8;
                }
            }
            (width, height, pixels)
        } else if mode == "reproj" {
            let mut pixels = flip_rows(&rendered, width, height)?;
            let texture = tev.texture_level(0).map_err(|_| bake_error(23))?;
            let copy = spec
                .get("copy")
                .and_then(|value| value.as_array())
                .filter(|values| values.len() == 2)
                .ok_or_else(|| bake_error(12))?;
            let y0 = copy[0].as_u64().ok_or_else(|| bake_error(12))? as usize;
            let y1 = copy[1].as_u64().ok_or_else(|| bake_error(12))? as usize;
            let texture_width = texture.width as usize;
            let texture_height = texture.height as usize;
            if y1 > height || y0 > y1 || texture_width == 0 || texture_height == 0 {
                return Err(bake_error(12));
            }
            for y in y0..y1 {
                for x in 0..width {
                    let source = ((y % texture_height) * texture_width + x % texture_width) * 4;
                    let destination = (y * width + x) * 4;
                    pixels[destination..destination + 4]
                        .copy_from_slice(&texture.rgba[source..source + 4]);
                }
            }
            (width, height, pixels)
        } else if part_mode {
            (32, 32, flip_rows(&rendered, 32, 32)?)
        } else {
            let mut pixels = flip_rows(&rendered, bake_size, bake_size)?;
            let target = image
                .get("w")
                .and_then(|v| v.as_u64())
                .ok_or_else(|| bake_error(12))? as usize;
            let target_h = image
                .get("h")
                .and_then(|v| v.as_u64())
                .ok_or_else(|| bake_error(12))? as usize;
            pixels = box_save(
                &pixels,
                bake_size,
                bake_size,
                target,
                target_h,
                false,
                spec.get("alpha").and_then(|v| v.as_u64()).unwrap_or(0) as u8,
            )
            .map_err(|_| bake_error(9))?;
            (target, target_h, pixels)
        };
        let color_type = image
            .get("ct")
            .and_then(|v| v.as_u64())
            .ok_or_else(|| bake_error(12))? as u8;
        let channels = match color_type {
            2 => 3,
            6 => 4,
            _ => return Err(bake_error(12)),
        };
        if rgba.len()
            != target_width
                .checked_mul(target_height)
                .and_then(|n| n.checked_mul(4))
                .ok_or(TextureError::TooLarge)?
        {
            return Err(bake_error(12));
        }
        apply_recipe_residual(image_index, image, &mut rgba, channels)
            .map_err(|_| bake_error(10))?;
        let mut pixels = Vec::with_capacity(rgba.len() / 4 * channels);
        for pixel in rgba.chunks_exact(4) {
            pixels.extend_from_slice(&pixel[..channels]);
        }
        let png = encode_png(target_width, target_height, color_type, channels, &pixels)
            .map_err(|_| bake_error(11))?;
        let files = image
            .get("files")
            .and_then(|v| v.as_array())
            .ok_or_else(|| bake_error(12))?;
        let paths = files
            .iter()
            .map(|file| {
                file.get("path")
                    .and_then(|v| v.as_str())
                    .map(str::to_owned)
                    .ok_or_else(|| bake_error(12))
            })
            .collect::<Result<Vec<_>, _>>()
            .map_err(|_| bake_error(12))?;
        result.push((paths, png));
    }
    Ok(result)
}

#[cfg(feature = "processor")]
fn generate_ffwall_image(
    image_index: usize,
    image: &serde_json::Value,
    spec: &serde_json::Value,
    archives: &std::collections::BTreeMap<String, Vec<u8>>,
    source_archive: &[u8],
    figure_files: &[(String, Vec<u8>)],
    generated_files: &[(String, Vec<u8>)],
) -> Result<(Vec<String>, Vec<u8>), TextureError> {
    use crate::geometry_bake::{
        build_bake_materials, parse_obj, prepare_high_mesh_with_options, serialize_bake_job,
        BakeOptions,
    };
    use std::collections::HashMap;

    let geometry_error =
        |stage: u32| TextureError::GeometryBake(10_000 + image_index as u32 * 32 + stage);

    let source = spec
        .get("src")
        .and_then(|v| v.as_str())
        .ok_or(TextureError::InvalidHeader)?;
    let (_, inner) = source.split_once(':').ok_or(TextureError::InvalidHeader)?;
    let model_bytes =
        crate::asset_formats::rarc_find(source_archive, inner).map_err(|_| geometry_error(1))?;
    let model = crate::j3d_model::J3dModel::parse(&model_bytes).map_err(|_| geometry_error(2))?;
    let tev = crate::tev::TevModel::parse(&model_bytes).map_err(|_| geometry_error(3))?;

    let only = spec
        .get("only")
        .and_then(|v| v.as_array())
        .ok_or(TextureError::InvalidHeader)?
        .iter()
        .map(|v| {
            v.as_str()
                .map(str::to_owned)
                .ok_or(TextureError::InvalidHeader)
        })
        .collect::<Result<Vec<_>, _>>()?;
    let excluded = (0..tev.material_count())
        .filter_map(|index| tev.material_name(index).ok())
        .filter(|name| !only.iter().any(|candidate| candidate == name))
        .map(str::to_owned)
        .collect::<Vec<_>>();
    let mut high = prepare_high_mesh_with_options(&model, &tev, false, &excluded)
        .map_err(|_| geometry_error(5))?;
    let (materials, textures) =
        build_bake_materials(&tev, &high, true, None).map_err(|_| geometry_error(6))?;

    let obj_name = spec
        .get("obj")
        .and_then(|v| v.as_str())
        .ok_or(TextureError::InvalidHeader)?;
    let obj_path = format!("assets/{obj_name}");
    let obj_bytes = figure_files
        .iter()
        .chain(generated_files.iter())
        .find(|(path, _)| path == &obj_path)
        .map(|(_, bytes)| bytes.as_slice())
        .ok_or_else(|| geometry_error(30))?;
    let material_name = spec
        .get("mat")
        .and_then(|v| v.as_str())
        .ok_or(TextureError::InvalidHeader)?;
    let low = parse_obj(obj_bytes, Some(material_name)).map_err(|_| geometry_error(4))?;

    let placement = spec
        .get("placement")
        .and_then(|v| v.as_array())
        .filter(|v| v.len() == 5)
        .ok_or(TextureError::InvalidHeader)?;
    let galaxy = placement[0].as_str().ok_or(TextureError::InvalidHeader)?;
    let zone = placement[1].as_str().ok_or(TextureError::InvalidHeader)?;
    let placed_name = placement[2].as_str().ok_or(TextureError::InvalidHeader)?;
    let placed_index = placement[3].as_u64().ok_or(TextureError::InvalidHeader)? as usize;
    let unit = placement[4].as_f64().ok_or(TextureError::InvalidHeader)?;
    let galaxy_arc = archives
        .get(&format!("StageData/{galaxy}.arc"))
        .ok_or(TextureError::InvalidHeader)?;
    let zone_arc = archives
        .get(&format!("StageData/{zone}.arc"))
        .ok_or_else(|| geometry_error(28))?;
    let zone_transform = jmap_named_transform(galaxy_arc, zone).map_err(|_| geometry_error(24))?;
    let placed_transform = jmap_named_transform_at(zone_arc, placed_name, placed_index)
        .map_err(|_| geometry_error(25))?;
    let world = ffwall_mul(zone_transform, placed_transform);
    let rot = ffwall_rotation(ffwall_euler(world));
    let translation = [
        ffwall_round3(world[0][3]),
        ffwall_round3(world[1][3]),
        ffwall_round3(world[2][3]),
    ];
    let inverse = ffwall_inverse3(rot).ok_or(TextureError::InvalidHeader)?;
    let mut placed_low = low;
    for point in &mut placed_low.positions {
        let raw = [
            point[0] * 0.004 * unit - translation[0],
            point[1] * 0.004 * unit - translation[1],
            point[2] * 0.004 * unit - translation[2],
        ];
        *point = ffwall_mul3(inverse, raw);
    }

    let face = placed_low
        .faces
        .first()
        .ok_or(TextureError::InvalidHeader)?;
    if face.len() < 3 {
        return Err(TextureError::InvalidHeader);
    }
    let quad = face
        .iter()
        .take(3)
        .map(|(position, _)| {
            placed_low
                .positions
                .get(*position)
                .copied()
                .ok_or(TextureError::InvalidHeader)
        })
        .collect::<Result<Vec<_>, _>>()?;
    let uv = face
        .iter()
        .take(3)
        .map(|(_, texcoord)| {
            placed_low
                .uvs
                .get(*texcoord)
                .copied()
                .ok_or(TextureError::InvalidHeader)
        })
        .collect::<Result<Vec<_>, _>>()?;
    let normal = ffwall_cross(ffwall_sub(quad[1], quad[0]), ffwall_sub(quad[2], quad[0]));
    let face_threshold = spec.get("face").and_then(|v| v.as_f64()).unwrap_or(0.01);
    let mut kept_triangles = Vec::new();
    let mut kept_materials = Vec::new();
    for (triangle, &material_index) in high.triangles.iter().zip(&high.triangle_materials) {
        let [a, b, c] = triangle.map(|index| high.positions[index as usize]);
        let source_normal = ffwall_cross(ffwall_sub(b, a), ffwall_sub(c, a));
        let dot = -ffwall_dot(source_normal, normal);
        let threshold = face_threshold
            * ffwall_dot(source_normal, source_normal).sqrt()
            * ffwall_dot(normal, normal).sqrt();
        if dot > threshold {
            kept_triangles.push(*triangle);
            kept_materials.push(material_index);
        }
    }
    high.triangles = kept_triangles;
    high.triangle_materials = kept_materials;
    if high.triangles.is_empty() {
        return Err(TextureError::InvalidHeader);
    }

    let mut cells = Vec::new();
    let mut points = Vec::new();
    let area = ffwall_edge(uv[0], uv[1], uv[2]);
    if area == 0.0 {
        return Err(TextureError::InvalidHeader);
    }
    for y in 0..32 {
        for x in 0..32 {
            let sample = [(x as f64 + 0.5) / 32.0, 1.0 - (y as f64 + 0.5) / 32.0];
            let bary = [
                ffwall_edge(uv[1], uv[2], sample) / area,
                ffwall_edge(uv[2], uv[0], sample) / area,
                ffwall_edge(uv[0], uv[1], sample) / area,
            ];
            if bary.iter().all(|v| *v >= 0.0) {
                cells.push(y * 32 + x);
                points.push([
                    quad[0][0] * bary[0] + quad[1][0] * bary[1] + quad[2][0] * bary[2],
                    quad[0][1] * bary[0] + quad[1][1] * bary[1] + quad[2][1] * bary[2],
                    quad[0][2] * bary[0] + quad[1][2] * bary[1] + quad[2][2] * bary[2],
                ]);
            }
        }
    }
    let queries = crate::geometry_bake::LowMesh {
        positions: points,
        uvs: Vec::new(),
        faces: Vec::new(),
    };
    const FLAG_POINT_NEAR: i32 = 67_108_864;
    let payload = serialize_bake_job(
        &queries,
        &high,
        &materials,
        &textures,
        BakeOptions {
            width: 1,
            height: 1,
            margin: 0,
            flags: FLAG_POINT_NEAR,
            extent_factor: 1.0e9,
        },
        &[],
    )
    .map_err(|_| geometry_error(7))?;
    let nearest = crate::textures_bake_core::run(&payload).map_err(|_| geometry_error(13))?;
    if nearest.len() != cells.len() * 16 {
        return Err(TextureError::InvalidHeader);
    }

    let gain = spec.get("gain").and_then(|v| v.as_f64()).unwrap_or(1.0);
    let mut sampled = HashMap::<usize, [u8; 3]>::new();
    for (sample_index, &cell) in cells.iter().enumerate() {
        let record = &nearest[sample_index * 16..sample_index * 16 + 16];
        let triangle_index = f32::from_le_bytes(record[0..4].try_into().unwrap());
        let mut color = [0u8; 3];
        if triangle_index >= 0.0 {
            let triangle = high.triangles[triangle_index as usize];
            let material_index = high.triangle_materials[triangle_index as usize] as usize;
            let b = f32::from_le_bytes(record[4..8].try_into().unwrap()) as f64;
            let c = f32::from_le_bytes(record[8..12].try_into().unwrap()) as f64;
            let weights = [1.0 - b - c, b, c];
            let ids = triangle.map(|index| index as usize);
            let mut texcoord = [0.0; 2];
            let mut vertex_color = [0.0; 3];
            for corner in 0..3 {
                for channel in 0..2 {
                    texcoord[channel] += high.uvs[ids[corner]][channel] * weights[corner];
                }
                for channel in 0..3 {
                    vertex_color[channel] += high.colors[ids[corner]][channel] * weights[corner];
                }
            }
            let material = materials
                .get(material_index)
                .ok_or(TextureError::InvalidHeader)?;
            let sampled_texture = if material.texture >= 0 {
                let texture = textures
                    .get(material.texture as usize)
                    .ok_or(TextureError::InvalidHeader)?;
                Some(ffwall_bilerp(
                    texture,
                    material.wrap_s,
                    material.wrap_t,
                    texcoord,
                ))
            } else {
                None
            };
            let original_material = *high
                .materials
                .get(material_index)
                .ok_or(TextureError::InvalidHeader)?;
            let (reg0, reg1) = tev
                .ffwall_registers(original_material)
                .map_err(|_| TextureError::InvalidHeader)?;
            for channel in 0..3 {
                let base = sampled_texture
                    .map(|rgba| rgba[channel])
                    .unwrap_or(material.factor[channel] as f64 * 255.0);
                let factor = gain
                    * (reg1[channel] + (reg0[channel] - reg1[channel]) * vertex_color[channel]);
                color[channel] = ((base * factor).clamp(0.0, 255.0) + 0.5).floor() as u8;
            }
        }
        sampled.insert(cell, color);
    }
    let mut rgba = vec![0u8; 32 * 32 * 4];
    for cell in 0..32 * 32 {
        if let Some(color) = sampled.get(&cell) {
            rgba[cell * 4..cell * 4 + 3].copy_from_slice(color);
        } else {
            let y = cell / 32;
            let x = cell % 32;
            let mut best_distance = u32::MAX;
            let mut best_cell = cell;
            for &candidate in &cells {
                let cy = candidate / 32;
                let cx = candidate % 32;
                let dy = cy as i32 - y as i32;
                let dx = cx as i32 - x as i32;
                let distance = (dy * dy + dx * dx) as u32;
                if distance <= best_distance {
                    best_distance = distance;
                    best_cell = candidate;
                }
            }
            let color = sampled.get(&best_cell).copied().unwrap_or([0; 3]);
            rgba[cell * 4..cell * 4 + 3].copy_from_slice(&color);
        }
        rgba[cell * 4 + 3] = 255;
    }
    apply_recipe_residual(image_index, image, &mut rgba, 4)?;
    let files = image
        .get("files")
        .and_then(|v| v.as_array())
        .ok_or(TextureError::InvalidHeader)?
        .iter()
        .map(|file| {
            file.get("path")
                .and_then(|v| v.as_str())
                .map(str::to_owned)
                .ok_or(TextureError::InvalidHeader)
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok((files, encode_png(32, 32, 6, 4, &rgba)?))
}

fn ffwall_field_hash(name: &str) -> u32 {
    name.as_bytes().iter().fold(0u32, |hash, byte| {
        let value = if *byte >= 0x80 {
            *byte as i32 - 256
        } else {
            *byte as i32
        };
        hash.wrapping_mul(31).wrapping_add(value as u32)
    })
}

fn ffwall_named_transform(
    archive: &[u8],
    name: &str,
    wanted: usize,
) -> Result<[[f64; 4]; 3], TextureError> {
    let files = crate::asset_formats::rarc_listtree(archive, "Jmp/Placement")
        .map_err(|_| TextureError::InvalidHeader)?;
    let fields =
        ["name", "pos_x", "pos_y", "pos_z", "dir_x", "dir_y", "dir_z"].map(ffwall_field_hash);
    let mut found = 0usize;
    for (_, bytes) in files {
        let table =
            crate::asset_formats::bcsv_decode(&bytes).map_err(|_| TextureError::InvalidHeader)?;
        let indexes = fields.map(|hash| table.fields.iter().position(|field| field.hash == hash));
        if indexes.iter().any(Option::is_none) {
            continue;
        }
        for row in &table.rows {
            let Some(crate::asset_formats::BcsvValue::Text(row_name)) =
                row.get(indexes[0].unwrap())
            else {
                continue;
            };
            if row_name != name {
                continue;
            }
            if found == wanted {
                let mut value = [0.0; 6];
                for index in 0..6 {
                    value[index] = match row.get(indexes[index + 1].unwrap()) {
                        Some(crate::asset_formats::BcsvValue::Float(number)) => *number as f64,
                        Some(crate::asset_formats::BcsvValue::Integer(number)) => *number as f64,
                        _ => return Err(TextureError::InvalidHeader),
                    };
                }
                return Ok(ffwall_matrix(
                    [value[0], value[1], value[2]],
                    [value[3], value[4], value[5]],
                ));
            }
            found += 1;
        }
    }
    Err(TextureError::InvalidHeader)
}

fn jmap_named_transform(archive: &[u8], name: &str) -> Result<[[f64; 4]; 3], TextureError> {
    let transform = ffwall_named_transform(archive, name, 0)?;
    Ok(transform.map(|row| row.map(ffwall_round3)))
}

fn jmap_named_transform_at(
    archive: &[u8],
    name: &str,
    index: usize,
) -> Result<[[f64; 4]; 3], TextureError> {
    ffwall_named_transform(archive, name, index)
}

fn ffwall_matrix(position: [f64; 3], direction: [f64; 3]) -> [[f64; 4]; 3] {
    let [rx, ry, rz] = direction.map(f64::to_radians);
    let (sx, cx) = rx.sin_cos();
    let (sy, cy) = ry.sin_cos();
    let (sz, cz) = rz.sin_cos();
    [
        [
            cz * cy,
            cz * sy * sx - sz * cx,
            cz * sy * cx + sz * sx,
            position[0],
        ],
        [
            sz * cy,
            sz * sy * sx + cz * cx,
            sz * sy * cx - cz * sx,
            position[1],
        ],
        [-sy, cy * sx, cy * cx, position[2]],
    ]
}

fn ffwall_mul(a: [[f64; 4]; 3], b: [[f64; 4]; 3]) -> [[f64; 4]; 3] {
    let mut out = [[0.0; 4]; 3];
    for row in 0..3 {
        for col in 0..3 {
            out[row][col] = (0..3).map(|k| a[row][k] * b[k][col]).sum();
        }
        out[row][3] = (0..3).map(|k| a[row][k] * b[k][3]).sum::<f64>() + a[row][3];
    }
    out
}

fn ffwall_euler(matrix: [[f64; 4]; 3]) -> [f64; 3] {
    let r20 = matrix[2][0].clamp(-1.0, 1.0);
    let ry = (-r20).asin();
    let (rx, rz) = if r20.abs() < 0.99999 {
        (
            matrix[2][1].atan2(matrix[2][2]),
            matrix[1][0].atan2(matrix[0][0]),
        )
    } else {
        ((-matrix[1][2]).atan2(matrix[1][1]), 0.0)
    };
    [rx, ry, rz].map(|v| (v.to_degrees() * 10_000.0).round_ties_even() / 10_000.0)
}

fn ffwall_rotation(degrees: [f64; 3]) -> [[f64; 3]; 3] {
    let [x, y, z] = degrees.map(f64::to_radians);
    let (sx, cx) = x.sin_cos();
    let (sy, cy) = y.sin_cos();
    let (sz, cz) = z.sin_cos();
    let rz = [[cz, -sz, 0.0], [sz, cz, 0.0], [0.0, 0.0, 1.0]];
    let ry = [[cy, 0.0, sy], [0.0, 1.0, 0.0], [-sy, 0.0, cy]];
    let rx = [[1.0, 0.0, 0.0], [0.0, cx, -sx], [0.0, sx, cx]];
    ffwall_mul3x3(ffwall_mul3x3(rz, ry), rx)
}

fn ffwall_mul3x3(a: [[f64; 3]; 3], b: [[f64; 3]; 3]) -> [[f64; 3]; 3] {
    std::array::from_fn(|row| {
        std::array::from_fn(|col| (0..3).map(|k| a[row][k] * b[k][col]).sum())
    })
}

fn ffwall_inverse3(m: [[f64; 3]; 3]) -> Option<[[f64; 3]; 3]> {
    let cof = [
        [
            m[1][1] * m[2][2] - m[1][2] * m[2][1],
            m[1][2] * m[2][0] - m[1][0] * m[2][2],
            m[1][0] * m[2][1] - m[1][1] * m[2][0],
        ],
        [
            m[2][1] * m[0][2] - m[2][2] * m[0][1],
            m[2][2] * m[0][0] - m[2][0] * m[0][2],
            m[2][0] * m[0][1] - m[2][1] * m[0][0],
        ],
        [
            m[0][1] * m[1][2] - m[0][2] * m[1][1],
            m[0][2] * m[1][0] - m[0][0] * m[1][2],
            m[0][0] * m[1][1] - m[0][1] * m[1][0],
        ],
    ];
    let determinant = (0..3).map(|k| m[0][k] * cof[0][k]).sum::<f64>();
    if determinant == 0.0 {
        return None;
    }
    Some(std::array::from_fn(|row| {
        std::array::from_fn(|col| cof[col][row] / determinant)
    }))
}

fn ffwall_mul3(m: [[f64; 3]; 3], p: [f64; 3]) -> [f64; 3] {
    std::array::from_fn(|row| (0..3).map(|col| m[row][col] * p[col]).sum())
}

fn ffwall_round3(value: f64) -> f64 {
    (value * 1000.0).round_ties_even() / 1000.0
}

fn ffwall_cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn ffwall_sub(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn ffwall_dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    (0..3).map(|index| a[index] * b[index]).sum()
}

fn ffwall_edge(a: [f64; 2], b: [f64; 2], p: [f64; 2]) -> f64 {
    (b[0] - a[0]) * (p[1] - a[1]) - (b[1] - a[1]) * (p[0] - a[0])
}

fn ffwall_wrap(index: i64, length: usize, mode: i32) -> usize {
    let n = length as i64;
    match mode {
        1 => index.rem_euclid(n) as usize,
        2 => {
            let value = index.rem_euclid(2 * n);
            if value >= n {
                (2 * n - 1 - value) as usize
            } else {
                value as usize
            }
        }
        _ => index.clamp(0, n - 1) as usize,
    }
}

fn ffwall_bilerp(texture: &TextureLevel, wrap_s: i32, wrap_t: i32, uv: [f64; 2]) -> [f64; 3] {
    let width = texture.width as usize;
    let height = texture.height as usize;
    let x = uv[0] * width as f64 - 0.5;
    let y = uv[1] * height as f64 - 0.5;
    let x0 = x.floor() as i64;
    let y0 = y.floor() as i64;
    let fx = x - x0 as f64;
    let fy = y - y0 as f64;
    let mut output = [0.0; 3];
    for (dy, wy) in [(0, 1.0 - fy), (1, fy)] {
        let yy = ffwall_wrap(y0 + dy, height, wrap_t);
        for (dx, wx) in [(0, 1.0 - fx), (1, fx)] {
            let xx = ffwall_wrap(x0 + dx, width, wrap_s);
            let offset = (yy * width + xx) * 4;
            for channel in 0..3 {
                output[channel] += texture.rgba[offset + channel] as f64 * wx * wy;
            }
        }
    }
    output
}

fn decode_aa64() -> Vec<[f32; 2]> {
    let packed = crate::asset_packs::BAKE_AA64;
    let mut values = Vec::with_capacity(packed.len() * 4 / 3);
    for rgb in packed.chunks_exact(3) {
        let bits = ((rgb[0] as u32) << 16) | ((rgb[1] as u32) << 8) | rgb[2] as u32;
        values.extend_from_slice(&[
            (bits >> 18) as u8,
            ((bits >> 12) & 63) as u8,
            ((bits >> 6) & 63) as u8,
            (bits & 63) as u8,
        ]);
    }
    let mut offsets = Vec::with_capacity(64 * 64 * 4);
    for pixel in 0..64 * 64 {
        offsets.push([0.0, 0.0]);
        for sample in 0..3 {
            let start = pixel * 6 + sample * 2;
            offsets.push([
                (values[start] as f32 + 0.5) / 64.0 - 0.5,
                (values[start + 1] as f32 + 0.5) / 64.0 - 0.5,
            ]);
        }
    }
    offsets
}

#[cfg(feature = "processor")]
fn flip_rows(rgba: &[u8], width: usize, height: usize) -> Result<Vec<u8>, TextureError> {
    let row_bytes = width.checked_mul(4).ok_or(TextureError::TooLarge)?;
    if rgba.len()
        != row_bytes
            .checked_mul(height)
            .ok_or(TextureError::TooLarge)?
    {
        return Err(TextureError::InvalidHeader);
    }
    let mut output = vec![0; rgba.len()];
    for y in 0..height {
        output[(height - 1 - y) * row_bytes..(height - y) * row_bytes]
            .copy_from_slice(&rgba[y * row_bytes..(y + 1) * row_bytes]);
    }
    Ok(output)
}

fn recipe_sources(pred: &serde_json::Value) -> Result<Vec<usize>, TextureError> {
    let op = pred
        .get("op")
        .and_then(|v| v.as_str())
        .ok_or(TextureError::InvalidHeader)?;
    let index = |name: &str| {
        pred.get(name)
            .and_then(|v| v.as_u64())
            .map(|v| v as usize)
            .ok_or(TextureError::InvalidHeader)
    };
    match op {
        "id" | "box" => Ok(vec![index("src")?]),
        "envflat" | "ffflat" | "ramp" => Ok(vec![index("mat")?]),
        "vlerp" => Ok(vec![index("src")?, index("mat")?]),
        _ => Ok(Vec::new()),
    }
}

fn source_archives_from_meta(
    meta: &serde_json::Value,
    bake_specs: &serde_json::Value,
) -> Result<Vec<String>, TextureError> {
    let sources = meta
        .get("sources")
        .and_then(|v| v.as_array())
        .ok_or(TextureError::InvalidHeader)?;
    let images = meta
        .get("images")
        .and_then(|v| v.as_array())
        .ok_or(TextureError::InvalidHeader)?;
    let mut paths = std::collections::BTreeSet::new();
    for image in images {
        let pred = image.get("pred").ok_or(TextureError::InvalidHeader)?;
        let indices = recipe_sources(pred)?;
        if indices.is_empty() {
            continue;
        }
        let mut archives = std::collections::BTreeSet::new();
        for index in indices {
            let source = sources
                .get(index)
                .and_then(|v| v.as_array())
                .ok_or(TextureError::InvalidHeader)?;
            let kind = source
                .get(2)
                .and_then(|v| v.as_str())
                .ok_or(TextureError::InvalidHeader)?;
            if !matches!(kind, "j3d" | "bti" | "tev") {
                return Err(TextureError::InvalidHeader);
            }
            let archive = source
                .first()
                .and_then(|v| v.as_str())
                .ok_or(TextureError::InvalidHeader)?;
            archives.insert(archive.to_owned());
        }
        if archives.len() != 1 {
            return Err(TextureError::InvalidHeader);
        }
        paths.extend(archives);
    }
    for image in images {
        let Some(pred) = image.get("pred") else {
            continue;
        };
        if pred.get("op").and_then(|v| v.as_str()) != Some("bake") {
            continue;
        }
        let recipe = pred
            .get("spec")
            .and_then(|v| v.as_str())
            .ok_or(TextureError::InvalidHeader)?;
        let spec = bake_specs.get(recipe).ok_or(TextureError::InvalidHeader)?;
        if !matches!(
            spec.get("mode").and_then(|v| v.as_str()),
            Some("bti" | "stalk" | "tex" | "flatmean" | "ffice" | "rockball" | "jpa" | "pstar")
        ) {
            continue;
        }
        let source = spec
            .get("src")
            .and_then(|v| v.as_str())
            .ok_or(TextureError::InvalidHeader)?;
        let (archive, _) = source.split_once(':').ok_or(TextureError::InvalidHeader)?;
        paths.insert(archive.to_owned());
    }
    Ok(paths.into_iter().collect())
}

/// Build supported source-backed texture recipes as their archives are read.
pub fn generate_simple_images(
    archives: &[(String, Vec<u8>)],
) -> Result<Vec<(Vec<String>, Vec<u8>)>, TextureError> {
    let generator = SimpleImageGenerator::new()?;
    let mut output = Vec::new();
    for (name, bytes) in archives {
        output.extend(generator.generate_archive(name, bytes)?);
    }
    Ok(output)
}

fn generate_simple_archive(
    meta: &serde_json::Value,
    archive_name: &str,
    archive_bytes: &[u8],
) -> Result<Vec<(Vec<String>, Vec<u8>)>, TextureError> {
    use std::collections::{BTreeMap, BTreeSet};
    let sources = meta
        .get("sources")
        .and_then(|v| v.as_array())
        .ok_or(TextureError::InvalidHeader)?;
    let images = meta
        .get("images")
        .and_then(|v| v.as_array())
        .ok_or(TextureError::InvalidHeader)?;
    let mut decoded = BTreeMap::<usize, TextureLevel>::new();
    let mut models = BTreeMap::<String, crate::tev::TevModel>::new();
    let mut required = BTreeSet::new();
    for image in images {
        let pred = image.get("pred").ok_or(TextureError::InvalidHeader)?;
        let indices = recipe_sources(pred)?;
        if indices.is_empty() {
            continue;
        }
        let mut same_archive = true;
        for index in &indices {
            let source = sources
                .get(*index)
                .and_then(|v| v.as_array())
                .ok_or(TextureError::InvalidHeader)?;
            let source_archive = source
                .first()
                .and_then(|v| v.as_str())
                .ok_or(TextureError::InvalidHeader)?;
            same_archive &= source_archive == archive_name;
        }
        if same_archive {
            required.extend(indices);
        }
    }
    for index in required {
        let source = sources
            .get(index)
            .and_then(|v| v.as_array())
            .ok_or(TextureError::InvalidHeader)?;
        let archive = source
            .first()
            .and_then(|v| v.as_str())
            .ok_or(TextureError::InvalidHeader)?;
        let inner = source
            .get(1)
            .and_then(|v| v.as_str())
            .ok_or(TextureError::InvalidHeader)?;
        let kind = source
            .get(2)
            .and_then(|v| v.as_str())
            .ok_or(TextureError::InvalidHeader)?;
        let texture_index = source
            .get(3)
            .and_then(|v| v.as_u64())
            .ok_or(TextureError::InvalidHeader)? as usize;
        let level_index = source
            .get(4)
            .and_then(|v| v.as_u64())
            .ok_or(TextureError::InvalidHeader)? as usize;
        if archive != archive_name {
            return Err(TextureError::InvalidHeader);
        }
        if kind == "tev" {
            if !models.contains_key(inner) {
                let body = crate::asset_formats::rarc_find(archive_bytes, inner)
                    .map_err(|_| TextureError::Truncated)?;
                models.insert(
                    inner.to_owned(),
                    crate::tev::TevModel::parse(&body).map_err(|_| TextureError::InvalidHeader)?,
                );
            }
            let model = models.get(inner).ok_or(TextureError::InvalidHeader)?;
            let (width, height, rgba) = model
                .effective_rgba(texture_index)
                .map_err(|_| TextureError::InvalidHeader)?;
            decoded.insert(
                index,
                TextureLevel {
                    width: u16::try_from(width).map_err(|_| TextureError::TooLarge)?,
                    height: u16::try_from(height).map_err(|_| TextureError::TooLarge)?,
                    rgba,
                },
            );
        } else {
            let body = crate::asset_formats::rarc_find(archive_bytes, inner)
                .map_err(|_| TextureError::Truncated)?;
            let header = if kind == "bti" {
                0
            } else {
                *j3d_texture_headers(&body)?
                    .get(texture_index)
                    .ok_or(TextureError::InvalidHeader)?
            };
            let level = bti_levels(&body, header, true)?
                .into_iter()
                .nth(level_index)
                .ok_or(TextureError::InvalidHeader)?;
            decoded.insert(index, level);
        }
    }

    let mut result = Vec::new();
    for (image_index, image) in images.iter().enumerate() {
        let pred = image.get("pred").ok_or(TextureError::InvalidHeader)?;
        let op = pred
            .get("op")
            .and_then(|v| v.as_str())
            .ok_or(TextureError::InvalidHeader)?;
        if recipe_sources(pred)?.is_empty() {
            continue;
        }
        let indices = recipe_sources(pred)?;
        if indices
            .iter()
            .any(|index| sources[*index][0].as_str() != Some(archive_name))
        {
            continue;
        }
        let width = image
            .get("w")
            .and_then(|v| v.as_u64())
            .ok_or(TextureError::InvalidHeader)? as usize;
        let height = image
            .get("h")
            .and_then(|v| v.as_u64())
            .ok_or(TextureError::InvalidHeader)? as usize;
        let mut rgba = match op {
            "id" | "box" => {
                let index = indices[0];
                let level = decoded.get(&index).ok_or(TextureError::InvalidHeader)?;
                if op == "id" {
                    if width != level.width as usize || height != level.height as usize {
                        return Err(TextureError::InvalidHeader);
                    }
                    level.rgba.clone()
                } else {
                    let dither = pred.get("dither").and_then(|v| v.as_u64()).unwrap_or(0) != 0;
                    let alpha_mode = pred.get("am").and_then(|v| v.as_u64()).unwrap_or(0) as u8;
                    let mut rgba = box_save(
                        &level.rgba,
                        level.width as usize,
                        level.height as usize,
                        width,
                        height,
                        dither,
                        alpha_mode,
                    )?;
                    if let Some(cut) = pred.get("lumcut").and_then(|v| v.as_u64()) {
                        for pixel in rgba.chunks_exact_mut(4) {
                            let luminance = (299 * pixel[0] as u32
                                + 587 * pixel[1] as u32
                                + 114 * pixel[2] as u32)
                                / 1000;
                            pixel[3] = if luminance >= cut as u32 { 255 } else { 0 };
                        }
                    }
                    rgba
                }
            }
            "envflat" | "ffflat" => {
                let source = sources[indices[0]]
                    .as_array()
                    .ok_or(TextureError::InvalidHeader)?;
                let inner = source[1].as_str().ok_or(TextureError::InvalidHeader)?;
                let material = source[3].as_u64().ok_or(TextureError::InvalidHeader)? as usize;
                let model = models.get(inner).ok_or(TextureError::InvalidHeader)?;
                let color = if op == "envflat" {
                    model
                        .env_flat(material)
                        .map_err(|_| TextureError::InvalidHeader)?
                } else {
                    model
                        .flat_mean(material)
                        .map_err(|_| TextureError::InvalidHeader)?
                };
                let mut rgba = Vec::with_capacity(
                    width
                        .checked_mul(height)
                        .and_then(|n| n.checked_mul(4))
                        .ok_or(TextureError::TooLarge)?,
                );
                for _ in 0..width * height {
                    rgba.extend_from_slice(&[color[0], color[1], color[2], 255]);
                }
                rgba
            }
            "ramp" => {
                let source = sources[indices[0]]
                    .as_array()
                    .ok_or(TextureError::InvalidHeader)?;
                let inner = source[1].as_str().ok_or(TextureError::InvalidHeader)?;
                let material = source[3].as_u64().ok_or(TextureError::InvalidHeader)? as usize;
                models
                    .get(inner)
                    .ok_or(TextureError::InvalidHeader)?
                    .ramp(material, width, height)
                    .map_err(|_| TextureError::InvalidHeader)?
            }
            "vlerp" => {
                let source = sources[indices[0]]
                    .as_array()
                    .ok_or(TextureError::InvalidHeader)?;
                let source_index = indices[0];
                let texture_index = source[3].as_u64().ok_or(TextureError::InvalidHeader)? as usize;
                let mat_source = sources[indices[1]]
                    .as_array()
                    .ok_or(TextureError::InvalidHeader)?;
                let inner = mat_source[1].as_str().ok_or(TextureError::InvalidHeader)?;
                let material = mat_source[3].as_u64().ok_or(TextureError::InvalidHeader)? as usize;
                let level = decoded
                    .get(&source_index)
                    .ok_or(TextureError::InvalidHeader)?;
                let model = models.get(inner).ok_or(TextureError::InvalidHeader)?;
                let (_, _, lerped) = model
                    .vlerp(texture_index, material)
                    .map_err(|_| TextureError::InvalidHeader)?;
                box_save(
                    &lerped,
                    level.width as usize,
                    level.height as usize,
                    width,
                    height,
                    false,
                    0,
                )?
            }
            _ => continue,
        };
        let color_type = image
            .get("ct")
            .and_then(|v| v.as_u64())
            .ok_or(TextureError::InvalidHeader)? as u8;
        let (png_type, channels) = match color_type {
            2 => (2, 3),
            6 => (6, 4),
            _ => return Err(TextureError::InvalidHeader),
        };
        apply_recipe_residual(image_index, image, &mut rgba, channels)?;
        let mut pixels = Vec::new();
        pixels
            .try_reserve_exact(
                width
                    .checked_mul(height)
                    .and_then(|n| n.checked_mul(channels))
                    .ok_or(TextureError::TooLarge)?,
            )
            .map_err(|_| TextureError::TooLarge)?;
        for pixel in rgba.chunks_exact(4) {
            pixels.extend_from_slice(&pixel[..channels]);
        }
        let png = encode_png(width, height, png_type, channels, &pixels)?;
        let files = image
            .get("files")
            .and_then(|v| v.as_array())
            .ok_or(TextureError::InvalidHeader)?;
        let mut paths = Vec::new();
        paths
            .try_reserve(files.len())
            .map_err(|_| TextureError::TooLarge)?;
        for file in files {
            let path = file
                .get("path")
                .and_then(|v| v.as_str())
                .ok_or(TextureError::InvalidHeader)?;
            paths.push(path.to_owned());
        }
        result.push((paths, png));
    }
    Ok(result)
}

/// Build the source-backed bake recipes supported by the texture stage.
fn generate_bake_archive(
    meta: &serde_json::Value,
    bake_specs: &serde_json::Value,
    archive_name: &str,
    archive_bytes: &[u8],
) -> Result<Vec<(Vec<String>, Vec<u8>)>, TextureError> {
    let images = meta
        .get("images")
        .and_then(|v| v.as_array())
        .ok_or(TextureError::InvalidHeader)?;
    let mut result = Vec::new();
    let mut models = std::collections::BTreeMap::<String, crate::tev::TevModel>::new();
    let mut particle_bytes = None::<Vec<u8>>;
    let mut particles = None::<JpaArchive>;
    for (image_index, image) in images.iter().enumerate() {
        let pred = image.get("pred").ok_or(TextureError::InvalidHeader)?;
        if pred.get("op").and_then(|v| v.as_str()) != Some("bake") {
            continue;
        }
        let recipe = pred
            .get("spec")
            .and_then(|v| v.as_str())
            .ok_or(TextureError::InvalidHeader)?;
        let spec = bake_specs.get(recipe).ok_or(TextureError::InvalidHeader)?;
        let mode = spec.get("mode").and_then(|v| v.as_str()).unwrap_or("bake");
        if !matches!(
            mode,
            "bti" | "stalk" | "tex" | "flatmean" | "ffice" | "rockball" | "jpa" | "pstar"
        ) {
            continue;
        }
        let source = spec
            .get("src")
            .and_then(|v| v.as_str())
            .ok_or(TextureError::InvalidHeader)?;
        let (source_archive, inner) = source.split_once(':').ok_or(TextureError::InvalidHeader)?;
        if source_archive != archive_name {
            continue;
        }

        let width = image
            .get("w")
            .and_then(|v| v.as_u64())
            .ok_or(TextureError::InvalidHeader)? as usize;
        let height = image
            .get("h")
            .and_then(|v| v.as_u64())
            .ok_or(TextureError::InvalidHeader)? as usize;
        let mut rgba = match mode {
            "bti" | "stalk" => {
                let body = crate::asset_formats::rarc_find(archive_bytes, inner)
                    .map_err(|_| TextureError::Truncated)?;
                let level = bti_levels(&body, 0, false)?
                    .into_iter()
                    .next()
                    .ok_or(TextureError::InvalidHeader)?;
                if mode == "bti" {
                    box_save(
                        &level.rgba,
                        level.width as usize,
                        level.height as usize,
                        width,
                        height,
                        false,
                        spec.get("alpha").and_then(|v| v.as_u64()).unwrap_or(0) as u8,
                    )?
                } else {
                    stalk_rgba(spec, &level, width, height)?
                }
            }
            "tex" => {
                let material = spec
                    .get("gmat")
                    .and_then(|v| v.as_str())
                    .ok_or(TextureError::InvalidHeader)?;
                let model = load_texture_model(&mut models, archive_bytes, inner)?;
                let (sw, sh, pixels) = model
                    .material_texture(material)
                    .map_err(|_| TextureError::InvalidHeader)?;
                let crop = spec
                    .get("crop")
                    .and_then(|v| v.as_array())
                    .map(|items| {
                        items
                            .iter()
                            .map(|item| {
                                item.as_u64()
                                    .map(|value| value as usize)
                                    .ok_or(TextureError::InvalidHeader)
                            })
                            .collect::<Result<Vec<_>, _>>()
                    })
                    .transpose()?;
                if crop.as_ref().is_some_and(|rect| rect.len() != 4) {
                    return Err(TextureError::InvalidHeader);
                }
                box_save_crop(
                    &pixels,
                    sw,
                    sh,
                    width,
                    height,
                    crop.as_deref(),
                    spec.get("alpha").and_then(|v| v.as_u64()).unwrap_or(0) as u8,
                )?
            }
            "flatmean" => {
                let material = spec
                    .get("gmat")
                    .and_then(|v| v.as_str())
                    .ok_or(TextureError::InvalidHeader)?;
                let stages = spec
                    .get("stages")
                    .and_then(|v| v.as_u64())
                    .map(|value| value as usize);
                let model = load_texture_model(&mut models, archive_bytes, inner)?;
                let index = model
                    .material_index(material)
                    .map_err(|_| TextureError::InvalidHeader)?;
                let mean = model
                    .effective_mean_rgb(index, stages)
                    .map_err(|_| TextureError::InvalidHeader)?;
                constant_rgba(mean, width, height)?
            }
            "ffice" => {
                let material = spec
                    .get("mat")
                    .and_then(|v| v.as_str())
                    .ok_or(TextureError::InvalidHeader)?;
                let texture = spec
                    .get("tex")
                    .and_then(|v| v.as_u64())
                    .ok_or(TextureError::InvalidHeader)? as usize;
                let scale = spec.get("scale").and_then(|v| v.as_f64()).unwrap_or(2.0);
                let model = load_texture_model(&mut models, archive_bytes, inner)?;
                let level = model
                    .texture_level(texture)
                    .map_err(|_| TextureError::InvalidHeader)?;
                let (mat_color, konst) = model
                    .ice_parameters(material)
                    .map_err(|_| TextureError::InvalidHeader)?;
                ffice_rgba(&level, mat_color, konst, scale, width, height)?
            }
            "rockball" => {
                let model = load_texture_model(&mut models, archive_bytes, inner)?;
                rockball_rgba(spec, model, width, height)?
            }
            "jpa" => {
                if particle_bytes.is_none() {
                    particle_bytes = Some(
                        crate::asset_formats::rarc_find(archive_bytes, inner)
                            .map_err(|_| TextureError::Truncated)?,
                    );
                }
                let bytes = particle_bytes.as_ref().ok_or(TextureError::InvalidHeader)?;
                if particles.is_none() {
                    particles = Some(JpaArchive::parse(bytes)?);
                }
                jpa_rgba(
                    spec,
                    particles.as_ref().ok_or(TextureError::InvalidHeader)?,
                    bytes,
                    width,
                    height,
                )?
            }
            "pstar" => {
                let model = load_texture_model(&mut models, archive_bytes, inner)?;
                let palette_indices = pred.get("palx");
                power_star_rgba(spec, model, palette_indices, width, height)?
            }
            _ => return Err(TextureError::InvalidHeader),
        };
        let color_type = image
            .get("ct")
            .and_then(|v| v.as_u64())
            .ok_or(TextureError::InvalidHeader)? as u8;
        let channels = match color_type {
            2 => 3,
            6 => 4,
            _ => return Err(TextureError::InvalidHeader),
        };
        apply_recipe_residual(image_index, image, &mut rgba, channels)?;
        let mut pixels = Vec::with_capacity(
            width
                .checked_mul(height)
                .and_then(|n| n.checked_mul(channels))
                .ok_or(TextureError::TooLarge)?,
        );
        for pixel in rgba.chunks_exact(4) {
            pixels.extend_from_slice(&pixel[..channels]);
        }
        let png = encode_png(width, height, color_type, channels, &pixels)?;
        let files = image
            .get("files")
            .and_then(|v| v.as_array())
            .ok_or(TextureError::InvalidHeader)?;
        let paths = files
            .iter()
            .map(|file| {
                file.get("path")
                    .and_then(|v| v.as_str())
                    .map(str::to_owned)
                    .ok_or(TextureError::InvalidHeader)
            })
            .collect::<Result<Vec<_>, _>>()?;
        result.push((paths, png));
    }
    Ok(result)
}

fn rockball_rgba(
    spec: &serde_json::Value,
    model: &crate::tev::TevModel,
    width: usize,
    height: usize,
) -> Result<Vec<u8>, TextureError> {
    if width != 32 || height != 32 {
        return Err(TextureError::InvalidHeader);
    }
    let names = spec
        .get("texs")
        .and_then(|value| value.as_array())
        .ok_or(TextureError::InvalidHeader)?;
    if names.len() != 4 {
        return Err(TextureError::InvalidHeader);
    }
    let textures = names
        .iter()
        .map(|name| {
            model
                .texture_named(name.as_str().ok_or(TextureError::InvalidHeader)?)
                .map_err(|_| TextureError::InvalidHeader)
        })
        .collect::<Result<Vec<_>, _>>()?;
    let gain = spec
        .get("gain")
        .and_then(|value| value.as_f64())
        .ok_or(TextureError::InvalidHeader)?;
    let red = spec
        .get("red")
        .and_then(|value| value.as_array())
        .ok_or(TextureError::InvalidHeader)?;
    let ruby = spec
        .get("ruby")
        .and_then(|value| value.as_array())
        .ok_or(TextureError::InvalidHeader)?;
    if red.len() != 3 || ruby.len() != 2 {
        return Err(TextureError::InvalidHeader);
    }
    let red = red
        .iter()
        .map(|value| value.as_f64().ok_or(TextureError::InvalidHeader))
        .collect::<Result<Vec<_>, _>>()?;
    let base = ruby[0].as_f64().ok_or(TextureError::InvalidHeader)?;
    let lift = ruby[1].as_f64().ok_or(TextureError::InvalidHeader)?;

    let green = textures
        .iter()
        .take(2)
        .map(|texture| resize_rgb_channels(texture, 32, 24))
        .collect::<Result<Vec<_>, _>>()?;
    let total: u64 = green[1].iter().flatten().map(|&value| value as u64).sum();
    let mean = total as f64 / (32 * 24 * 3) as f64;
    if mean == 0.0 {
        return Err(TextureError::InvalidHeader);
    }
    let crystal = resize_channel(&textures[2], 0, 32, 8)?;
    let glint = resize_channel(&textures[3], 0, 32, 8)?;
    let mut out = vec![0u8; 32 * 32 * 4];
    for pixel in 0..32 * 24 {
        for channel in 0..3 {
            let value =
                gain * green[0][channel][pixel] as f64 * (green[1][channel][pixel] as f64 / mean);
            out[pixel * 4 + channel] = value.clamp(0.0, 255.0) as u8;
        }
        out[pixel * 4 + 3] = 255;
    }
    for pixel in 0..32 * 8 {
        let crystal = crystal[pixel] as f64 / 255.0;
        let glint = glint[pixel] as f64 / 255.0;
        let destination = (32 * 24 + pixel) * 4;
        for channel in 0..3 {
            let value = red[channel] * (base + lift * crystal) + 255.0 * glint;
            out[destination + channel] = value.clamp(0.0, 255.0) as u8;
        }
        out[destination + 3] = 255;
    }
    Ok(out)
}

fn power_star_rgba(
    spec: &serde_json::Value,
    model: &crate::tev::TevModel,
    palette_indices: Option<&serde_json::Value>,
    width: usize,
    height: usize,
) -> Result<Vec<u8>, TextureError> {
    let layers = spec
        .get("layers")
        .and_then(|value| value.as_array())
        .ok_or(TextureError::InvalidHeader)?;
    let mut sampled_layers = Vec::with_capacity(layers.len());
    for layer in layers {
        let values = layer.as_array().ok_or(TextureError::InvalidHeader)?;
        if values.len() != 6 {
            return Err(TextureError::InvalidHeader);
        }
        let texture_index = values[0].as_u64().ok_or(TextureError::InvalidHeader)? as usize;
        let texture = model
            .texture_level(texture_index)
            .map_err(|_| TextureError::InvalidHeader)?;
        let scale = values[1].as_f64().ok_or(TextureError::InvalidHeader)?;
        let rotation = values[2].as_i64().ok_or(TextureError::InvalidHeader)? as f64 / 32768.0
            * std::f64::consts::PI;
        let (sin, cos) = crate::j3d_model::sincos(rotation);
        let tx = values[3].as_f64().ok_or(TextureError::InvalidHeader)?;
        let ty = values[4].as_f64().ok_or(TextureError::InvalidHeader)?;
        let wrap = values[5].as_bool().ok_or(TextureError::InvalidHeader)?;
        sampled_layers.push((texture, scale, cos, sin, tx, ty, wrap));
    }
    let kind = spec
        .get("kind")
        .and_then(|value| value.as_str())
        .ok_or(TextureError::InvalidHeader)?;
    let material = if kind == "regs" {
        let name = spec
            .get("gmat")
            .and_then(|value| value.as_str())
            .ok_or(TextureError::InvalidHeader)?;
        Some(
            model
                .power_star_parameters(name)
                .map_err(|_| TextureError::InvalidHeader)?,
        )
    } else {
        None
    };
    let gain = spec
        .get("gain")
        .and_then(|value| value.as_f64())
        .unwrap_or(0.0);
    let pixel_count = width.checked_mul(height).ok_or(TextureError::TooLarge)?;
    let mut output = vec![0u8; pixel_count.checked_mul(4).ok_or(TextureError::TooLarge)?];
    let mut float_colors = Vec::with_capacity(pixel_count);
    for y in 0..height {
        for x in 0..width {
            let sx = (x as f64 + 0.5) / width as f64 * 2.0 - 1.0;
            let sy = -((y as f64 + 0.5) / height as f64 * 2.0 - 1.0);
            let radius_squared = sx * sx + sy * sy;
            let projection = if radius_squared > 0.995 {
                (0.995
                    / if radius_squared > 1e-9 {
                        radius_squared
                    } else {
                        1e-9
                    })
                .sqrt()
            } else {
                1.0
            };
            let nx = sx * projection;
            let ny = sy * projection;
            let mut colors = Vec::with_capacity(sampled_layers.len());
            for (texture, scale, cos, sin, tx, ty, wrap) in &sampled_layers {
                let u = 0.5 * nx;
                let v = -0.5 * ny;
                let rotated_u = (cos * u - sin * v) * scale;
                let rotated_v = (sin * u + cos * v) * scale;
                colors.push(bilinear_rgba(
                    texture,
                    rotated_u + 0.5 + tx,
                    rotated_v + 0.5 + ty,
                    *wrap,
                )?);
            }
            let color = match kind {
                "sum" if colors.len() == 2 => [
                    colors[0][0] + colors[1][0],
                    colors[0][1] + colors[1][1],
                    colors[0][2] + colors[1][2],
                    1.0,
                ],
                "gain" if colors.len() == 1 => [
                    gain * colors[0][0],
                    gain * colors[0][1],
                    gain * colors[0][2],
                    1.0,
                ],
                "regs" if colors.len() == 1 => {
                    let (c0, c1, material_color) = material.ok_or(TextureError::InvalidHeader)?;
                    [
                        c0[0] + colors[0][0] * c1[0],
                        c0[1] + colors[0][1] * c1[1],
                        c0[2] + colors[0][2] * c1[2],
                        power_star_clamp(c0[3] + colors[0][3]) * material_color[3],
                    ]
                }
                _ => return Err(TextureError::InvalidHeader),
            };
            float_colors.push([color[0], color[1], color[2]]);
            let destination = (y * width + x) * 4;
            for channel in 0..3 {
                output[destination + channel] =
                    round_power_star(power_star_clamp(color[channel]) * 255.0);
            }
            let stipple = spec
                .get("stipple")
                .and_then(|value| value.as_bool())
                .unwrap_or(false);
            output[destination + 3] = if stipple && (x + y) % 2 != 0 { 0 } else { 255 };
        }
    }
    if let Some(indices) = palette_indices {
        let palette = power_star_palette(&output, indices)?;
        for (pixel, target) in output.chunks_exact_mut(4).zip(float_colors) {
            let color = quantize_power_star(target.map(power_star_clamp), &palette);
            for channel in 0..3 {
                pixel[channel] = round_power_star(color[channel] * 255.0);
            }
        }
    }
    Ok(output)
}

fn bilinear_rgba(
    texture: &TextureLevel,
    u: f64,
    v: f64,
    wrap: bool,
) -> Result<[f64; 4], TextureError> {
    let width = texture.width as usize;
    let height = texture.height as usize;
    if width == 0
        || height == 0
        || texture.rgba.len()
            != width
                .checked_mul(height)
                .and_then(|n| n.checked_mul(4))
                .ok_or(TextureError::TooLarge)?
    {
        return Err(TextureError::InvalidHeader);
    }
    let x = u * width as f64 - 0.5;
    let y = v * height as f64 - 0.5;
    let x0 = x.floor() as isize;
    let y0 = y.floor() as isize;
    let fx = x - x0 as f64;
    let fy = y - y0 as f64;
    let index = |coordinate: isize, size: usize| -> usize {
        if wrap {
            coordinate.rem_euclid(size as isize) as usize
        } else {
            coordinate.clamp(0, size as isize - 1) as usize
        }
    };
    let x_left = index(x0, width);
    let x_right = index(x0 + 1, width);
    let y_top = index(y0, height);
    let y_bottom = index(y0 + 1, height);
    let mut output = [0.0; 4];
    for channel in 0..4 {
        let top_left = texture.rgba[(y_top * width + x_left) * 4 + channel] as f64 / 255.0;
        let top_right = texture.rgba[(y_top * width + x_right) * 4 + channel] as f64 / 255.0;
        let bottom_left = texture.rgba[(y_bottom * width + x_left) * 4 + channel] as f64 / 255.0;
        let bottom_right = texture.rgba[(y_bottom * width + x_right) * 4 + channel] as f64 / 255.0;
        output[channel] = top_left * (1.0 - fx) * (1.0 - fy)
            + top_right * fx * (1.0 - fy)
            + bottom_left * (1.0 - fx) * fy
            + bottom_right * fx * fy;
    }
    Ok(output)
}

fn power_star_palette(
    raw: &[u8],
    indices: &serde_json::Value,
) -> Result<Vec<[u8; 4]>, TextureError> {
    let values = indices.as_array().ok_or(TextureError::InvalidHeader)?;
    if values.is_empty() || values.len() % 5 != 0 || raw.len() % 4 != 0 {
        return Err(TextureError::InvalidHeader);
    }
    let colors = raw
        .chunks_exact(4)
        .map(|pixel| [pixel[0], pixel[1], pixel[2], pixel[3]])
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    let mut palette = Vec::with_capacity(values.len() / 5);
    for item in values.chunks_exact(5) {
        let index = item[0].as_u64().ok_or(TextureError::InvalidHeader)? as usize;
        let color = colors.get(index).ok_or(TextureError::InvalidHeader)?;
        let mut adjusted = *color;
        for channel in 0..4 {
            adjusted[channel] = (color[channel] as i64
                + item[channel + 1]
                    .as_i64()
                    .ok_or(TextureError::InvalidHeader)?)
            .try_into()
            .map_err(|_| TextureError::InvalidHeader)?;
        }
        palette.push(adjusted);
    }
    Ok(palette)
}

fn quantize_power_star(color: [f64; 3], palette: &[[u8; 4]]) -> [f64; 3] {
    let target = power_star_lab(color);
    let mut best = f64::INFINITY;
    let mut selected = [0.0; 3];
    for candidate in palette {
        let rgb = [
            candidate[0] as f64 / 255.0,
            candidate[1] as f64 / 255.0,
            candidate[2] as f64 / 255.0,
        ];
        let lab = power_star_lab(rgb);
        let dl = target[0] - lab[0];
        let da = (target[1] - lab[1]) * 2.0;
        let db = (target[2] - lab[2]) * 2.0;
        let distance = dl * dl + da * da + db * db;
        if distance < best {
            best = distance;
            selected = rgb;
        }
    }
    selected
}

fn power_star_lab(rgb: [f64; 3]) -> [f64; 3] {
    let mut linear = [0.0; 3];
    for channel in 0..3 {
        let value = rgb[channel];
        linear[channel] = if value > 0.04045 {
            let scaled = (value + 0.055) / 1.055;
            scaled * scaled * power_star_root(scaled * scaled, 5)
        } else {
            value / 12.92
        };
    }
    let matrix = [
        [0.4124, 0.3576, 0.1805],
        [0.2126, 0.7152, 0.0722],
        [0.0193, 0.1192, 0.9505],
    ];
    let reference = [0.9505, 1.0, 1.089];
    let mut transformed = [0.0; 3];
    for row in 0..3 {
        let value =
            (matrix[row][0] * linear[0] + matrix[row][1] * linear[1] + matrix[row][2] * linear[2])
                / reference[row];
        transformed[row] = if value > 0.008856 {
            power_star_root(value, 3)
        } else {
            7.787 * value + 16.0 / 116.0
        };
    }
    [
        116.0 * transformed[1] - 16.0,
        500.0 * (transformed[0] - transformed[1]),
        200.0 * (transformed[1] - transformed[2]),
    ]
}

fn power_star_root(value: f64, degree: usize) -> f64 {
    if value <= 0.0 {
        return 0.0;
    }
    let mut root = value.max(1.0);
    for _ in 0..400 {
        let mut power = 1.0;
        for _ in 1..degree {
            power *= root;
        }
        let next = ((degree - 1) as f64 * root + value / power) / degree as f64;
        if next >= root {
            break;
        }
        root = next;
    }
    root
}

fn power_star_clamp(value: f64) -> f64 {
    value.clamp(0.0, 1.0)
}

fn round_power_star(value: f64) -> u8 {
    value.round_ties_even().clamp(0.0, 255.0) as u8
}

fn resize_rgb_channels(
    texture: &TextureLevel,
    width: usize,
    height: usize,
) -> Result<[Vec<u8>; 3], TextureError> {
    Ok([
        resize_channel(texture, 0, width, height)?,
        resize_channel(texture, 1, width, height)?,
        resize_channel(texture, 2, width, height)?,
    ])
}

/// The rock ball recipe uses the separable fixed-point box filter from
/// `textures_bake_jpa.box_resize` on each RGB channel independently.
fn resize_channel(
    texture: &TextureLevel,
    channel: usize,
    width: usize,
    height: usize,
) -> Result<Vec<u8>, TextureError> {
    let source_width = texture.width as usize;
    let source_height = texture.height as usize;
    if channel >= 4
        || source_width == 0
        || source_height == 0
        || width == 0
        || height == 0
        || texture.rgba.len()
            != source_width
                .checked_mul(source_height)
                .and_then(|n| n.checked_mul(4))
                .ok_or(TextureError::TooLarge)?
    {
        return Err(TextureError::InvalidHeader);
    }
    const PRECISION: u32 = 22;
    let mut horizontal = vec![
        0u8;
        width
            .checked_mul(source_height)
            .ok_or(TextureError::TooLarge)?
    ];
    for y in 0..source_height {
        for x in 0..width {
            let (first, weights) = box_coefficients(source_width, width, x)?;
            let sum = weights
                .iter()
                .enumerate()
                .map(|(i, &weight)| {
                    texture.rgba[(y * source_width + first + i) * 4 + channel] as u64
                        * weight as u64
                })
                .sum::<u64>();
            horizontal[y * width + x] =
                ((sum + (1 << (PRECISION - 1))) >> PRECISION).min(255) as u8;
        }
    }
    let mut output = vec![0u8; width.checked_mul(height).ok_or(TextureError::TooLarge)?];
    for y in 0..height {
        let (first, weights) = box_coefficients(source_height, height, y)?;
        for x in 0..width {
            let sum = weights
                .iter()
                .enumerate()
                .map(|(i, &weight)| horizontal[(first + i) * width + x] as u64 * weight as u64)
                .sum::<u64>();
            output[y * width + x] = ((sum + (1 << (PRECISION - 1))) >> PRECISION).min(255) as u8;
        }
    }
    Ok(output)
}

fn box_coefficients(
    source: usize,
    target: usize,
    output: usize,
) -> Result<(usize, Vec<u32>), TextureError> {
    if source == 0 || target == 0 || output >= target {
        return Err(TextureError::InvalidHeader);
    }
    const SCALE: f64 = (1u32 << 22) as f64;
    let scale = source as f64 / target as f64;
    let filter_scale = scale.max(1.0);
    let support = 0.5 * filter_scale;
    let center = (output as f64 + 0.5) * scale;
    let mut first = (center - support + 0.5) as usize;
    let mut last = ((center + support + 0.5) as usize).min(source);
    first = first.min(source);
    last = last.max(first);
    let inv_scale = 1.0 / filter_scale;
    let mut active = Vec::with_capacity(last - first);
    for x in first..last {
        let coordinate = (x as f64 - center + 0.5) * inv_scale;
        active.push(coordinate > -0.5 && coordinate <= 0.5);
    }
    let count = active.iter().filter(|&&inside| inside).count();
    if count == 0 {
        return Err(TextureError::InvalidHeader);
    }
    let fixed = (0.5 + SCALE / count as f64) as u32;
    Ok((
        first,
        active
            .into_iter()
            .map(|inside| if inside { fixed } else { 0 })
            .collect(),
    ))
}

struct JpaResource {
    prm: [u8; 4],
    env: [u8; 4],
    textures: Vec<usize>,
}

struct JpaArchive {
    resources: std::collections::BTreeMap<u16, JpaResource>,
    texture_ranges: Vec<std::ops::Range<usize>>,
}

impl JpaArchive {
    fn parse(data: &[u8]) -> Result<Self, TextureError> {
        if data.get(..8) != Some(b"JPAC2-10") {
            return Err(TextureError::InvalidHeader);
        }
        let resource_count = be16(data, 8)? as usize;
        let texture_count = be16(data, 10)? as usize;
        let texture_offset = be32(data, 12)? as usize;
        let mut resources = std::collections::BTreeMap::new();
        let mut cursor = 16usize;
        for _ in 0..resource_count {
            let id = be16(data, cursor)?;
            let block_count = be16(data, cursor + 2)? as usize;
            cursor = cursor.checked_add(8).ok_or(TextureError::TooLarge)?;
            let mut resource = JpaResource {
                prm: [0; 4],
                env: [0; 4],
                textures: Vec::new(),
            };
            let mut has_params = false;
            for _ in 0..block_count {
                let tag = data
                    .get(cursor..cursor.checked_add(4).ok_or(TextureError::TooLarge)?)
                    .ok_or(TextureError::Truncated)?;
                let size = be32(data, cursor + 4)? as usize;
                if size < 8 {
                    return Err(TextureError::InvalidHeader);
                }
                let end = cursor.checked_add(size).ok_or(TextureError::TooLarge)?;
                data.get(cursor..end).ok_or(TextureError::Truncated)?;
                match tag {
                    b"BSP1" => {
                        if size < 46 {
                            return Err(TextureError::InvalidHeader);
                        }
                        resource
                            .prm
                            .copy_from_slice(&data[cursor + 38..cursor + 42]);
                        resource
                            .env
                            .copy_from_slice(&data[cursor + 42..cursor + 46]);
                        has_params = true;
                    }
                    b"TDB1" => {
                        if size < 8 || (size - 8) % 2 != 0 {
                            return Err(TextureError::InvalidHeader);
                        }
                        resource.textures = (0..(size - 8) / 2)
                            .map(|index| be16(data, cursor + 8 + index * 2).map(usize::from))
                            .collect::<Result<Vec<_>, _>>()?;
                    }
                    _ => {}
                }
                cursor = end;
            }
            if has_params && !resource.textures.is_empty() {
                resources.insert(id, resource);
            }
        }
        let mut texture_ranges = Vec::with_capacity(texture_count);
        cursor = texture_offset;
        for _ in 0..texture_count {
            let size = be32(data, cursor + 4)? as usize;
            if size < 32 {
                return Err(TextureError::InvalidHeader);
            }
            let start = cursor.checked_add(32).ok_or(TextureError::TooLarge)?;
            let end = cursor.checked_add(size).ok_or(TextureError::TooLarge)?;
            data.get(start..end).ok_or(TextureError::Truncated)?;
            texture_ranges.push(start..end);
            cursor = end;
        }
        Ok(Self {
            resources,
            texture_ranges,
        })
    }
}

fn jpa_rgba(
    spec: &serde_json::Value,
    particles: &JpaArchive,
    jpc: &[u8],
    width: usize,
    height: usize,
) -> Result<Vec<u8>, TextureError> {
    let resource_id = u16::try_from(
        spec.get("rid")
            .and_then(|value| value.as_u64())
            .ok_or(TextureError::InvalidHeader)?,
    )
    .map_err(|_| TextureError::InvalidHeader)?;
    let resource = particles
        .resources
        .get(&resource_id)
        .ok_or(TextureError::InvalidHeader)?;
    let texture_index = *resource
        .textures
        .first()
        .ok_or(TextureError::InvalidHeader)?;
    let texture_range = particles
        .texture_ranges
        .get(texture_index)
        .ok_or(TextureError::InvalidHeader)?;
    let source = jpc
        .get(texture_range.clone())
        .ok_or(TextureError::Truncated)?;
    let texture = bti_levels(source, 0, false)?
        .into_iter()
        .next()
        .ok_or(TextureError::InvalidHeader)?;
    let image = resize_lanczos_rgba(
        &texture.rgba,
        texture.width as usize,
        texture.height as usize,
        width,
        height,
    )?;
    if spec
        .get("keep")
        .and_then(|value| value.as_bool())
        .unwrap_or(false)
    {
        return Ok(image);
    }
    let choose = |name: &str, default: [u8; 4]| -> Result<[u8; 4], TextureError> {
        let Some(value) = spec.get(name) else {
            return Ok(default);
        };
        let items = value.as_array().ok_or(TextureError::InvalidHeader)?;
        if items.len() != 2 {
            return Err(TextureError::InvalidHeader);
        }
        let id = u16::try_from(items[0].as_u64().ok_or(TextureError::InvalidHeader)?)
            .map_err(|_| TextureError::InvalidHeader)?;
        let key = items[1].as_str().ok_or(TextureError::InvalidHeader)?;
        let source = particles
            .resources
            .get(&id)
            .ok_or(TextureError::InvalidHeader)?;
        match key {
            "prm" => Ok(source.prm),
            "env" => Ok(source.env),
            _ => Err(TextureError::InvalidHeader),
        }
    };
    let low = choose("lo", resource.env)?;
    let high = choose("hi", resource.prm)?;
    let scale = spec
        .get("scale")
        .and_then(|value| value.as_bool())
        .unwrap_or(false);
    let threshold = spec
        .get("thr")
        .and_then(|value| value.as_f64())
        .ok_or(TextureError::InvalidHeader)?;
    let pixels = width.checked_mul(height).ok_or(TextureError::TooLarge)?;
    let mut output = vec![0u8; pixels.checked_mul(4).ok_or(TextureError::TooLarge)?];
    for pixel in 0..pixels {
        let intensity = image[pixel * 4] as f64 / 255.0;
        for channel in 0..3 {
            let mut value =
                low[channel] as f64 * (1.0 - intensity) + high[channel] as f64 * intensity;
            if scale {
                value *= intensity;
            }
            output[pixel * 4 + channel] = value.clamp(0.0, 255.0) as u8;
        }
        output[pixel * 4 + 3] = if intensity >= threshold { 255 } else { 0 };
    }
    Ok(output)
}

fn resize_lanczos_rgba(
    px: &[u8],
    source_width: usize,
    source_height: usize,
    width: usize,
    height: usize,
) -> Result<Vec<u8>, TextureError> {
    if source_width == width && source_height == height {
        return Ok(px.to_vec());
    }
    if source_width == 0
        || source_height == 0
        || width == 0
        || height == 0
        || px.len()
            != source_width
                .checked_mul(source_height)
                .and_then(|n| n.checked_mul(4))
                .ok_or(TextureError::TooLarge)?
    {
        return Err(TextureError::InvalidHeader);
    }
    const PRECISION: u32 = 22;
    let horizontal_coefficients = lanczos_coefficients(source_width, width)?;
    let vertical_coefficients = lanczos_coefficients(source_height, height)?;
    let first_row = vertical_coefficients
        .first()
        .ok_or(TextureError::InvalidHeader)?
        .0;
    let last_row = vertical_coefficients
        .last()
        .ok_or(TextureError::InvalidHeader)?;
    let row_end = last_row
        .0
        .checked_add(last_row.1.len())
        .ok_or(TextureError::TooLarge)?;
    let rows = row_end
        .checked_sub(first_row)
        .ok_or(TextureError::InvalidHeader)?;
    let mut premultiplied = px.to_vec();
    for pixel in premultiplied.chunks_exact_mut(4) {
        let alpha = pixel[3] as u32;
        for channel in &mut pixel[..3] {
            let value = *channel as u32 * alpha + 128;
            *channel = (((value >> 8) + value) >> 8) as u8;
        }
    }
    let mut intermediate = vec![
        0u8;
        width
            .checked_mul(rows)
            .and_then(|n| n.checked_mul(4))
            .ok_or(TextureError::TooLarge)?
    ];
    let half = 1i64 << (PRECISION - 1);
    for row in 0..rows {
        let source_y = row + first_row;
        for x in 0..width {
            let (first, weights) = &horizontal_coefficients[x];
            for channel in 0..4 {
                let mut sum = half;
                for (offset, &weight) in weights.iter().enumerate() {
                    let source = (source_y * source_width + first + offset) * 4 + channel;
                    sum += premultiplied[source] as i64 * weight;
                }
                intermediate[(row * width + x) * 4 + channel] = clip_fixed(sum, PRECISION);
            }
        }
    }
    let mut output = vec![
        0u8;
        width
            .checked_mul(height)
            .and_then(|n| n.checked_mul(4))
            .ok_or(TextureError::TooLarge)?
    ];
    for y in 0..height {
        let (first, weights) = &vertical_coefficients[y];
        for x in 0..width {
            for channel in 0..4 {
                let mut sum = half;
                for (offset, &weight) in weights.iter().enumerate() {
                    let source = (((first + offset - first_row) * width + x) * 4) + channel;
                    sum += intermediate[source] as i64 * weight;
                }
                output[(y * width + x) * 4 + channel] = clip_fixed(sum, PRECISION);
            }
        }
    }
    for pixel in output.chunks_exact_mut(4) {
        let alpha = pixel[3];
        if alpha != 0 && alpha != 255 {
            for channel in &mut pixel[..3] {
                *channel = ((255 * *channel as u16) / alpha as u16).min(255) as u8;
            }
        }
    }
    Ok(output)
}

fn lanczos_coefficients(
    source: usize,
    target: usize,
) -> Result<Vec<(usize, Vec<i64>)>, TextureError> {
    use std::f64::consts::PI;
    const PRECISION: f64 = (1u32 << 22) as f64;
    let scale = source as f64 / target as f64;
    let filter_scale = scale.max(1.0);
    let support = 3.0 * filter_scale;
    let mut coefficients = Vec::with_capacity(target);
    for output in 0..target {
        let center = (output as f64 + 0.5) * scale;
        let mut first = (center - support + 0.5) as usize;
        if center - support + 0.5 < 0.0 {
            first = 0;
        }
        let mut end = ((center + support + 0.5) as usize).min(source);
        end = end.max(first);
        let inverse_scale = 1.0 / filter_scale;
        let mut weights = Vec::with_capacity(end - first);
        for offset in 0..end - first {
            let x = (offset as f64 + first as f64 - center + 0.5) * inverse_scale;
            let sinc = |value: f64| {
                if value == 0.0 {
                    1.0
                } else {
                    let radians = value * PI;
                    crate::j3d_model::sincos(radians).0 / radians
                }
            };
            let weight = if -3.0 <= x && x < 3.0 {
                sinc(x) * sinc(x / 3.0)
            } else {
                0.0
            };
            weights.push(weight);
        }
        let total = weights.iter().sum::<f64>();
        if total != 0.0 {
            for weight in &mut weights {
                *weight /= total;
            }
        }
        let fixed: Vec<i64> = weights
            .into_iter()
            .map(|weight| {
                let value = weight * PRECISION;
                if value < 0.0 {
                    (value - 0.5) as i64
                } else {
                    (value + 0.5) as i64
                }
            })
            .collect();
        coefficients.push((first, fixed));
    }
    if coefficients.iter().any(|(_, weights)| weights.is_empty()) {
        return Err(TextureError::InvalidHeader);
    }
    Ok(coefficients)
}

fn clip_fixed(value: i64, precision: u32) -> u8 {
    let shifted = value >> precision;
    shifted.clamp(0, 255) as u8
}

fn load_texture_model<'a>(
    models: &'a mut std::collections::BTreeMap<String, crate::tev::TevModel>,
    archive_bytes: &[u8],
    inner: &str,
) -> Result<&'a crate::tev::TevModel, TextureError> {
    if !models.contains_key(inner) {
        let body = crate::asset_formats::rarc_find(archive_bytes, inner)
            .map_err(|_| TextureError::Truncated)?;
        models.insert(
            inner.to_owned(),
            crate::tev::TevModel::parse(&body).map_err(|_| TextureError::InvalidHeader)?,
        );
    }
    models.get(inner).ok_or(TextureError::InvalidHeader)
}

fn constant_rgba(color: [u8; 3], width: usize, height: usize) -> Result<Vec<u8>, TextureError> {
    let pixels = width.checked_mul(height).ok_or(TextureError::TooLarge)?;
    let mut rgba = Vec::with_capacity(pixels.checked_mul(4).ok_or(TextureError::TooLarge)?);
    for _ in 0..pixels {
        rgba.extend_from_slice(&[color[0], color[1], color[2], 255]);
    }
    Ok(rgba)
}

fn ffice_rgba(
    texture: &TextureLevel,
    material_color: [f64; 3],
    konst: [f64; 3],
    scale: f64,
    width: usize,
    height: usize,
) -> Result<Vec<u8>, TextureError> {
    let tw = texture.width as usize;
    let th = texture.height as usize;
    if tw == 0 || th == 0 {
        return Err(TextureError::InvalidHeader);
    }
    let mut out = vec![
        0;
        width
            .checked_mul(height)
            .and_then(|n| n.checked_mul(4))
            .ok_or(TextureError::TooLarge)?
    ];
    for y in 0..height {
        for x in 0..width {
            let sx = (((x as f64 + 0.5) * tw as f64 / width as f64).floor() as usize).min(tw - 1);
            let sy = (((y as f64 + 0.5) * th as f64 / height as f64).floor() as usize).min(th - 1);
            let src = (sy * tw + sx) * 4;
            let dst = (y * width + x) * 4;
            for channel in 0..3 {
                let value = scale
                    * (texture.rgba[src + channel] as f64 / 255.0 * material_color[channel]
                        + konst[channel]
                        - 0.5);
                out[dst + channel] = (value * 255.0 + 0.5).floor().clamp(0.0, 255.0) as u8;
            }
            out[dst + 3] = 255;
        }
    }
    Ok(out)
}

fn stalk_rgba(
    spec: &serde_json::Value,
    texture: &TextureLevel,
    width: usize,
    height: usize,
) -> Result<Vec<u8>, TextureError> {
    let sw = texture.width as usize;
    let sh = texture.height as usize;
    if width == 0 || height == 0 || sw % width != 0 || sh % height != 0 {
        return Err(TextureError::InvalidHeader);
    }
    let ramp = spec
        .get("ramp")
        .and_then(|v| v.as_array())
        .ok_or(TextureError::InvalidHeader)?;
    if ramp.len() != 2 {
        return Err(TextureError::InvalidHeader);
    }
    let ca = ramp[0].as_f64().ok_or(TextureError::InvalidHeader)?;
    let cb = ramp[1].as_f64().ok_or(TextureError::InvalidHeader)?;
    let bw = sw / width;
    let bh = sh / height;
    let mut out = vec![
        0;
        width
            .checked_mul(height)
            .and_then(|n| n.checked_mul(4))
            .ok_or(TextureError::TooLarge)?
    ];
    for y in 0..height {
        for x in 0..width {
            let u = (x as f64 + 0.5) / width as f64;
            let ramp = (ca * (1.0 - u) + cb * u) / 255.0;
            for channel in 0..3 {
                let mut acc = 0.0;
                for dy in 0..bh {
                    for dx in 0..bw {
                        let at = ((y * bh + dy) * sw + x * bw + dx) * 4 + channel;
                        acc += texture.rgba[at] as f64 / 255.0;
                    }
                }
                let value = (acc / (bw * bh) as f64 * ramp * 255.0)
                    .round_ties_even()
                    .clamp(0.0, 255.0) as u8;
                out[(y * width + x) * 4 + channel] = value;
            }
            out[(y * width + x) * 4 + 3] = 255;
        }
    }
    Ok(out)
}

fn box_save_crop(
    pixels: &[u8],
    source_width: usize,
    source_height: usize,
    width: usize,
    height: usize,
    crop: Option<&[usize]>,
    alpha_mode: u8,
) -> Result<Vec<u8>, TextureError> {
    let Some([x0, y0, x1, y1]) = crop else {
        return box_save(
            pixels,
            source_width,
            source_height,
            width,
            height,
            false,
            alpha_mode,
        );
    };
    if x0 >= x1 || y0 >= y1 || *x1 > source_width || *y1 > source_height {
        return Err(TextureError::InvalidHeader);
    }
    let crop_width = x1 - x0;
    let crop_height = y1 - y0;
    let mut cropped = Vec::with_capacity(
        crop_width
            .checked_mul(crop_height)
            .and_then(|n| n.checked_mul(4))
            .ok_or(TextureError::TooLarge)?,
    );
    // Native crop coordinates are applied after the source has been vertically flipped.
    let raw_y0 = source_height - y1;
    for y in raw_y0..raw_y0 + crop_height {
        let start = (y * source_width + x0) * 4;
        let end = start + crop_width * 4;
        cropped.extend_from_slice(pixels.get(start..end).ok_or(TextureError::Truncated)?);
    }
    box_save(
        &cropped,
        crop_width,
        crop_height,
        width,
        height,
        false,
        alpha_mode,
    )
}

fn apply_recipe_residual(
    image_index: usize,
    image: &serde_json::Value,
    rgba: &mut [u8],
    channels: usize,
) -> Result<(), TextureError> {
    if image
        .get("skip_residual")
        .and_then(|value| value.as_bool())
        .unwrap_or(false)
    {
        return Ok(());
    }
    let name = format!("r/{image_index}");
    let Some(stream) = crate::asset_packs::get("textures/textures.pak", &name) else {
        return Ok(());
    };
    let mut cursor = 0usize;
    for pixel in rgba.chunks_exact_mut(4) {
        for channel in pixel.iter_mut().take(channels) {
            let mut value = 0u64;
            let mut shift = 0u32;
            loop {
                let byte = *stream.get(cursor).ok_or(TextureError::Truncated)?;
                cursor += 1;
                if shift >= 64 || (shift == 63 && byte & 0x7e != 0) {
                    return Err(TextureError::InvalidHeader);
                }
                value |= ((byte & 0x7f) as u64) << shift;
                if byte < 0x80 {
                    break;
                }
                shift += 7;
            }
            let delta = ((value >> 1) as i64) ^ (-((value & 1) as i64));
            *channel = channel.wrapping_add(delta as u8);
        }
    }
    if cursor != stream.len() {
        return Err(TextureError::InvalidHeader);
    }
    Ok(())
}

fn pairwise_sum(values: &[f32]) -> f32 {
    let n = values.len();
    if n < 8 {
        return values.iter().copied().fold(0.0, |sum, value| sum + value);
    }
    if n <= 128 {
        let mut sums = [0.0f32; 8];
        sums.copy_from_slice(&values[..8]);
        let mut i = 8;
        let end = n - n % 8;
        while i < end {
            for lane in 0..8 {
                sums[lane] += values[i + lane];
            }
            i += 8;
        }
        let mut result = ((sums[0] + sums[1]) + (sums[2] + sums[3]))
            + ((sums[4] + sums[5]) + (sums[6] + sums[7]));
        while i < n {
            result += values[i];
            i += 1;
        }
        return result;
    }
    let mut half = n / 2;
    half -= half % 8;
    pairwise_sum(&values[..half]) + pairwise_sum(&values[half..])
}

fn byte_float(value: u8) -> f32 {
    value as f32 * (1.0f32 / 255.0f32)
}

fn to_byte(value: f32) -> u8 {
    const TOP: f32 = 1.0f32 - 0.5f32 / 255.0f32;
    if value <= 0.0 {
        0
    } else if value > TOP {
        255
    } else {
        (255.0f32 * value + 0.5f32) as u8
    }
}

/// Match `textures_ops.box_save`, including its vertical flips and f32
/// accumulation order, for predictable downsampled GX source images.
fn box_save(
    px: &[u8],
    sw: usize,
    sh: usize,
    tw: usize,
    th: usize,
    dither: bool,
    alpha_mode: u8,
) -> Result<Vec<u8>, TextureError> {
    if sw == 0
        || sh == 0
        || tw == 0
        || th == 0
        || px.len()
            != sw
                .checked_mul(sh)
                .and_then(|n| n.checked_mul(4))
                .ok_or(TextureError::TooLarge)?
    {
        return Err(TextureError::InvalidHeader);
    }
    let mut source = px.to_vec();
    if dither {
        for y in 0..sh {
            let yb = sh - 1 - y;
            for x in 0..sw {
                source[(y * sw + x) * 4 + 3] = if (x + 2 * yb) % 4 == 0 { 255 } else { 0 };
            }
        }
    }
    let out_len = tw
        .checked_mul(th)
        .and_then(|n| n.checked_mul(4))
        .ok_or(TextureError::TooLarge)?;
    let mut flipped = vec![0u8; out_len];
    let mut indices = Vec::<usize>::new();
    let mut alphas = Vec::<f32>::new();
    for j in 0..th {
        let y0 = j * sh / th;
        let y1 = ((j + 1) * sh / th).max(y0 + 1);
        for i in 0..tw {
            let x0 = i * sw / tw;
            let x1 = ((i + 1) * sw / tw).max(x0 + 1);
            indices.clear();
            alphas.clear();
            for y in y0..y1 {
                for x in x0..x1 {
                    let at = ((sh - 1 - y) * sw + x) * 4;
                    indices.push(at);
                    alphas.push(byte_float(source[at + 3]));
                }
            }
            let count = indices.len();
            let alpha_sum = pairwise_sum(&alphas);
            let mut color = [0.0f32; 3];
            if alpha_sum > 1e-6 {
                for channel in 0..3 {
                    let first = indices[0];
                    let mut acc =
                        byte_float(source[first + channel]) * byte_float(source[first + 3]);
                    for &at in &indices[1..] {
                        acc += byte_float(source[at + channel]) * byte_float(source[at + 3]);
                    }
                    color[channel] = acc / alpha_sum;
                }
            } else {
                for channel in 0..3 {
                    let mut acc = byte_float(source[indices[0] + channel]);
                    for &at in &indices[1..] {
                        acc += byte_float(source[at + channel]);
                    }
                    color[channel] = acc / count as f32;
                }
            }
            let mut alpha = alpha_sum / count as f32;
            match alpha_mode {
                1 => alpha = 1.0,
                2 => {
                    alpha = if color[0].max(color[1]).max(color[2]) > 0.3 {
                        1.0
                    } else {
                        0.0
                    }
                }
                _ => (),
            }
            let dest = ((th - 1 - j) * tw + i) * 4;
            flipped[dest] = to_byte(color[0]);
            flipped[dest + 1] = to_byte(color[1]);
            flipped[dest + 2] = to_byte(color[2]);
            flipped[dest + 3] = to_byte(alpha);
        }
    }
    Ok(flipped)
}

fn encode_png(
    width: usize,
    height: usize,
    color_type: u8,
    channels: usize,
    pixels: &[u8],
) -> Result<Vec<u8>, TextureError> {
    let row = width.checked_mul(channels).ok_or(TextureError::TooLarge)?;
    if pixels.len() != row.checked_mul(height).ok_or(TextureError::TooLarge)? {
        return Err(TextureError::InvalidHeader);
    }
    let mut raw = Vec::new();
    raw.try_reserve_exact(
        (row + 1)
            .checked_mul(height)
            .ok_or(TextureError::TooLarge)?,
    )
    .map_err(|_| TextureError::TooLarge)?;
    for scanline in pixels.chunks_exact(row) {
        raw.push(0); // PNG filter: None
        raw.extend_from_slice(scanline);
    }
    let zlib = miniz_oxide::deflate::compress_to_vec_zlib(&raw, 1);
    let mut out = b"\x89PNG\r\n\x1a\n".to_vec();
    png_chunk(
        &mut out,
        b"IHDR",
        &[
            (width as u32).to_be_bytes().as_slice(),
            (height as u32).to_be_bytes().as_slice(),
            &[8, color_type, 0, 0, 0],
        ]
        .concat(),
    );
    png_chunk(&mut out, b"IDAT", &zlib);
    png_chunk(&mut out, b"IEND", &[]);
    Ok(out)
}

#[cfg(feature = "processor")]
fn decode_rgba_png(data: &[u8]) -> Result<(usize, usize, Vec<u8>), TextureError> {
    if data.get(..8) != Some(b"\x89PNG\r\n\x1a\n") {
        return Err(TextureError::InvalidHeader);
    }
    let mut cursor = 8usize;
    let mut dimensions = None;
    let mut compressed = Vec::new();
    while cursor < data.len() {
        let length = u32::from_be_bytes(
            data.get(cursor..cursor + 4)
                .ok_or(TextureError::Truncated)?
                .try_into()
                .unwrap(),
        ) as usize;
        let kind = data
            .get(cursor + 4..cursor + 8)
            .ok_or(TextureError::Truncated)?;
        let start = cursor.checked_add(8).ok_or(TextureError::TooLarge)?;
        let end = start.checked_add(length).ok_or(TextureError::TooLarge)?;
        let payload = data.get(start..end).ok_or(TextureError::Truncated)?;
        match kind {
            b"IHDR" => {
                if payload.len() != 13 || payload[8..] != [8, 6, 0, 0, 0] {
                    return Err(TextureError::InvalidHeader);
                }
                let width = u32::from_be_bytes(payload[..4].try_into().unwrap()) as usize;
                let height = u32::from_be_bytes(payload[4..8].try_into().unwrap()) as usize;
                if width == 0 || height == 0 {
                    return Err(TextureError::InvalidHeader);
                }
                dimensions = Some((width, height));
            }
            b"IDAT" => {
                compressed
                    .try_reserve(payload.len())
                    .map_err(|_| TextureError::TooLarge)?;
                compressed.extend_from_slice(payload);
            }
            b"IEND" => break,
            _ => {}
        }
        cursor = end.checked_add(4).ok_or(TextureError::TooLarge)?;
    }
    let (width, height) = dimensions.ok_or(TextureError::InvalidHeader)?;
    let row_bytes = width.checked_mul(4).ok_or(TextureError::TooLarge)?;
    let expected = row_bytes
        .checked_add(1)
        .and_then(|row| row.checked_mul(height))
        .ok_or(TextureError::TooLarge)?;
    let filtered = miniz_oxide::inflate::decompress_to_vec_zlib(&compressed)
        .map_err(|_| TextureError::InvalidHeader)?;
    if filtered.len() != expected {
        return Err(TextureError::InvalidHeader);
    }
    let mut output = vec![
        0u8;
        row_bytes
            .checked_mul(height)
            .ok_or(TextureError::TooLarge)?
    ];
    for y in 0..height {
        let source = y * (row_bytes + 1);
        let filter = filtered[source];
        for x in 0..row_bytes {
            let raw = filtered[source + 1 + x];
            let left = if x >= 4 {
                output[y * row_bytes + x - 4]
            } else {
                0
            };
            let above = if y > 0 {
                output[(y - 1) * row_bytes + x]
            } else {
                0
            };
            let upper_left = if y > 0 && x >= 4 {
                output[(y - 1) * row_bytes + x - 4]
            } else {
                0
            };
            let predictor = match filter {
                0 => 0,
                1 => left,
                2 => above,
                3 => ((left as u16 + above as u16) / 2) as u8,
                4 => {
                    let estimate = left as i32 + above as i32 - upper_left as i32;
                    let left_delta = (estimate - left as i32).abs();
                    let above_delta = (estimate - above as i32).abs();
                    let upper_left_delta = (estimate - upper_left as i32).abs();
                    if left_delta <= above_delta && left_delta <= upper_left_delta {
                        left
                    } else if above_delta <= upper_left_delta {
                        above
                    } else {
                        upper_left
                    }
                }
                _ => return Err(TextureError::InvalidHeader),
            };
            output[y * row_bytes + x] = raw.wrapping_add(predictor);
        }
    }
    Ok((width, height, output))
}

fn png_chunk(out: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
    out.extend_from_slice(&(data.len() as u32).to_be_bytes());
    out.extend_from_slice(kind);
    out.extend_from_slice(data);
    let mut crc = !0u32;
    for &byte in kind.iter().chain(data) {
        crc ^= byte as u32;
        for _ in 0..8 {
            crc = (crc >> 1) ^ (0xedb8_8320 & (0u32.wrapping_sub(crc & 1)));
        }
    }
    out.extend_from_slice(&(!crc).to_be_bytes());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expands_intensity_formats_and_crops_padded_blocks() {
        assert_eq!(decode(0, 1, 1, &[0xf0], &[]).unwrap(), [255, 255, 255, 255]);
        assert_eq!(decode(1, 1, 1, &[37], &[]).unwrap(), [37, 37, 37, 255]);
        assert_eq!(decode(2, 1, 1, &[0xf9], &[]).unwrap(), [153, 153, 153, 255]);
        assert_eq!(
            decode(3, 1, 1, &[72, 123], &[]).unwrap(),
            [123, 123, 123, 72]
        );
    }

    #[test]
    fn expands_direct_color_and_rgba8_block_formats() {
        assert_eq!(
            decode(4, 1, 1, &[0xf8, 0x00], &[]).unwrap(),
            [255, 0, 0, 255]
        );
        assert_eq!(
            decode(5, 1, 1, &[0x7f, 0xff], &[]).unwrap(),
            [255, 255, 255, 255]
        );
        let mut rgba8 = vec![0; 64];
        rgba8[0..2].copy_from_slice(&[99, 20]);
        rgba8[32..34].copy_from_slice(&[40, 60]);
        assert_eq!(decode(6, 1, 1, &rgba8, &[]).unwrap(), [20, 40, 60, 99]);
    }

    #[test]
    fn decodes_indexed_formats_and_palette_types() {
        let pal = decode_palette(1, &[0xf8, 0x00, 0x07, 0xe0]).unwrap();
        assert_eq!(decode(8, 1, 1, &[0x10], &pal).unwrap(), [0, 255, 0, 255]);
        assert_eq!(decode(9, 1, 1, &[0], &pal).unwrap(), [255, 0, 0, 255]);
        let rgba = decode_palette(0, &[0x80, 0x40]).unwrap();
        assert_eq!(decode(10, 1, 1, &[0, 0], &rgba).unwrap(), [64, 64, 64, 128]);
    }

    #[test]
    fn decodes_cmpr_transparency_and_rejects_unknown_formats() {
        let mut block = [0u8; 32];
        block[0..4].copy_from_slice(&[0, 0, 0, 0]);
        block[4..8].fill(0xff);
        assert_eq!(decode(14, 1, 1, &block, &[]).unwrap(), [0, 0, 0, 0]);
        assert_eq!(
            decode(7, 1, 1, &[], &[]),
            Err(TextureError::UnsupportedFormat(7))
        );
    }

    #[test]
    fn reports_bti_mip_levels() {
        let mut data = vec![0u8; 0x40 + 32 + 32];
        data[0] = 1;
        data[2..4].copy_from_slice(&8u16.to_be_bytes());
        data[4..6].copy_from_slice(&4u16.to_be_bytes());
        data[0x18] = 2;
        data[0x1c..0x20].copy_from_slice(&0x40u32.to_be_bytes());
        let levels = bti_levels(&data, 0, true).unwrap();
        assert_eq!(
            (levels[0].width, levels[0].height, levels[0].rgba.len()),
            (8, 4, 128)
        );
        assert_eq!(
            (levels[1].width, levels[1].height, levels[1].rgba.len()),
            (4, 2, 32)
        );
    }

    #[test]
    fn supported_recipes_select_their_source_archives() {
        let archives = simple_source_archives().unwrap();
        assert_eq!(archives.len(), 207);
        assert!(archives.windows(2).all(|pair| pair[0] < pair[1]));
        assert!(archives
            .iter()
            .any(|name| name == "ObjectData/SwingRope.arc"));
        assert!(archives
            .iter()
            .any(|name| name == "ObjectData/GhostMario.arc"));
        assert!(archives
            .iter()
            .any(|name| name == "ObjectData/IceRingBumpyPartsA.arc"));
        assert!(archives
            .iter()
            .any(|name| name == "ObjectData/DorayakiFortressPlanet.arc"));
        assert!(archives.iter().any(|name| name == "ObjectData/Creeper.arc"));
        assert!(archives.iter().any(|name| name == "ObjectData/Plant.arc"));
        assert!(archives
            .iter()
            .any(|name| name == "ObjectData/FlipPanel.arc"));
        assert!(archives
            .iter()
            .any(|name| name == "ObjectData/IceMario.arc"));
        assert!(archives.iter().any(|name| name == "ObjectData/WarpPod.arc"));
        assert!(archives
            .iter()
            .any(|name| name == "ObjectData/IceRingIcePlanet.arc"));
        assert!(archives.iter().any(|name| name == "ObjectData/Rock.arc"));
        assert!(archives
            .iter()
            .any(|name| name == "ParticleData/Effect.arc"));
        assert!(archives
            .iter()
            .any(|name| name == "ObjectData/PowerStar.arc"));
    }

    #[test]
    fn box_save_flips_and_downsamples_with_alpha_weighting() {
        let rgba = [
            255, 0, 0, 255, 0, 255, 0, 0, 0, 0, 255, 255, 255, 255, 255, 0,
        ];
        assert_eq!(
            box_save(&rgba, 2, 2, 1, 1, false, 0).unwrap(),
            [128, 0, 128, 128]
        );
        assert_eq!(
            box_save(&rgba, 2, 2, 1, 1, false, 1).unwrap(),
            [128, 0, 128, 255]
        );
    }

    #[test]
    fn box_dither_uses_source_row_coordinates() {
        let mut rgba = [80u8; 8 * 4 * 4];
        for pixel in rgba.chunks_exact_mut(4) {
            pixel[3] = 255;
        }
        let dithered = box_save(&rgba, 8, 4, 8, 4, true, 0).unwrap();
        for y in 0..4 {
            for x in 0..8 {
                let alpha = dithered[(y * 8 + x) * 4 + 3];
                assert_eq!(alpha == 255, (x + 2 * (3 - y)) % 4 == 0);
            }
        }
    }

    #[test]
    fn png_encoder_roundtrips_rgba_scanlines() {
        let pixels = [10, 20, 30, 40, 50, 60, 70, 80];
        let png = encode_png(2, 1, 6, 4, &pixels).unwrap();
        assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n");
        let mut pos = 8usize;
        let mut decoded = None;
        while pos < png.len() {
            let len = u32::from_be_bytes(png[pos..pos + 4].try_into().unwrap()) as usize;
            let kind = &png[pos + 4..pos + 8];
            let data = &png[pos + 8..pos + 8 + len];
            if kind == b"IDAT" {
                decoded = Some(miniz_oxide::inflate::decompress_to_vec_zlib(data).unwrap());
            }
            pos += len + 12;
        }
        let mut expected = vec![0];
        expected.extend_from_slice(&pixels);
        assert_eq!(decoded.unwrap(), expected);
        assert_eq!(&png[png.len() - 8..png.len() - 4], b"IEND");
    }
}

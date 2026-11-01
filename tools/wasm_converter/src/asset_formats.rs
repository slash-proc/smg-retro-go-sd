//! Readers for the archive formats needed by the asset construction stages.
//! These operate on one already-extracted archive; ISO range handling stays
//! in `extractor`.

#[derive(Debug, PartialEq, Eq)]
pub enum ArchiveError {
    Truncated,
    InvalidYaz0,
    InvalidRarc,
    NotFound,
}

fn slice(data: &[u8], offset: usize, len: usize) -> Result<&[u8], ArchiveError> {
    data.get(offset..offset.checked_add(len).ok_or(ArchiveError::Truncated)?)
        .ok_or(ArchiveError::Truncated)
}

fn be_u16(data: &[u8], offset: usize) -> Result<u16, ArchiveError> {
    Ok(u16::from_be_bytes(
        slice(data, offset, 2)?.try_into().unwrap(),
    ))
}

fn be_u32(data: &[u8], offset: usize) -> Result<u32, ArchiveError> {
    Ok(u32::from_be_bytes(
        slice(data, offset, 4)?.try_into().unwrap(),
    ))
}

fn be_i32(data: &[u8], offset: usize) -> Result<i32, ArchiveError> {
    Ok(i32::from_be_bytes(
        slice(data, offset, 4)?.try_into().unwrap(),
    ))
}

fn be_f32(data: &[u8], offset: usize) -> Result<f32, ArchiveError> {
    Ok(f32::from_bits(be_u32(data, offset)?))
}

fn add(a: usize, b: usize) -> Result<usize, ArchiveError> {
    a.checked_add(b).ok_or(ArchiveError::Truncated)
}

/// Decompress a Yaz0 stream, or copy through data that is not Yaz0 wrapped.
pub fn yaz0_decompress(src: &[u8]) -> Result<Vec<u8>, ArchiveError> {
    if src.get(..4) != Some(b"Yaz0") {
        return Ok(src.to_vec());
    }
    let size = be_u32(src, 4)? as usize;
    let mut dst = Vec::new();
    dst.try_reserve_exact(size)
        .map_err(|_| ArchiveError::Truncated)?;
    let mut sp = 16usize;
    while dst.len() < size {
        let code = *src.get(sp).ok_or(ArchiveError::InvalidYaz0)?;
        sp += 1;
        for bit in 0..8 {
            if dst.len() == size {
                break;
            }
            if code & (0x80 >> bit) != 0 {
                dst.push(*src.get(sp).ok_or(ArchiveError::InvalidYaz0)?);
                sp += 1;
                continue;
            }
            let b1 = *src.get(sp).ok_or(ArchiveError::InvalidYaz0)?;
            let b2 = *src.get(sp + 1).ok_or(ArchiveError::InvalidYaz0)?;
            sp += 2;
            let distance = (((b1 as usize & 0x0f) << 8) | b2 as usize) + 1;
            let count = if b1 >> 4 == 0 {
                let n = *src.get(sp).ok_or(ArchiveError::InvalidYaz0)? as usize;
                sp += 1;
                n + 0x12
            } else {
                (b1 >> 4) as usize + 2
            };
            if distance > dst.len() || count > size - dst.len() {
                return Err(ArchiveError::InvalidYaz0);
            }
            for _ in 0..count {
                let value = dst[dst.len() - distance];
                dst.push(value);
            }
        }
    }
    Ok(dst)
}

fn rarc_name(data: &[u8], string_base: usize, relative: usize) -> Result<String, ArchiveError> {
    let start = add(string_base, relative)?;
    let tail = data.get(start..).ok_or(ArchiveError::InvalidRarc)?;
    let end = tail
        .iter()
        .position(|&b| b == 0)
        .ok_or(ArchiveError::InvalidRarc)?;
    Ok(encoding_rs::SHIFT_JIS
        .decode_without_bom_handling(&tail[..end])
        .0
        .into_owned())
}

fn rarc_index(data: &[u8]) -> Result<(Vec<u8>, Vec<(String, u16, usize, usize)>), ArchiveError> {
    let data = yaz0_decompress(data)?;
    if data.get(..4) != Some(b"RARC") {
        return Err(ArchiveError::InvalidRarc);
    }
    let data_base = add(be_u32(&data, 12)? as usize, 0x20)?;
    let info = 0x20usize;
    let num_nodes = be_u32(&data, info)? as usize;
    let node_base = add(info, be_u32(&data, info + 4)? as usize)?;
    let num_entries = be_u32(&data, info + 8)? as usize;
    let entry_base = add(info, be_u32(&data, info + 12)? as usize)?;
    let string_base = add(info, be_u32(&data, info + 20)? as usize)?;
    if num_nodes == 0
        || node_base
            .checked_add(num_nodes.checked_mul(16).ok_or(ArchiveError::InvalidRarc)?)
            .filter(|&end| end <= data.len())
            .is_none()
        || entry_base
            .checked_add(
                num_entries
                    .checked_mul(20)
                    .ok_or(ArchiveError::InvalidRarc)?,
            )
            .filter(|&end| end <= data.len())
            .is_none()
    {
        return Err(ArchiveError::InvalidRarc);
    }
    let mut visited = vec![false; num_nodes];
    let mut files = Vec::new();

    fn visit(
        data: &[u8],
        data_base: usize,
        node_base: usize,
        entry_base: usize,
        string_base: usize,
        num_nodes: usize,
        num_entries: usize,
        visited: &mut [bool],
        files: &mut Vec<(String, u16, usize, usize)>,
        node_index: usize,
        prefix: &str,
    ) -> Result<(), ArchiveError> {
        if node_index >= num_nodes || visited[node_index] {
            return Err(ArchiveError::InvalidRarc);
        }
        visited[node_index] = true;
        let node = add(
            node_base,
            node_index
                .checked_mul(16)
                .ok_or(ArchiveError::InvalidRarc)?,
        )?;
        slice(&data, node, 16).map_err(|_| ArchiveError::InvalidRarc)?;
        let dir_name = rarc_name(&data, string_base, be_u32(&data, node + 4)? as usize)?;
        let count = be_u16(&data, node + 10)? as usize;
        let first = be_u32(&data, node + 12)? as usize;
        let path = format!("{}{}/", prefix, dir_name);
        if first
            .checked_add(count)
            .filter(|&x| x <= num_entries)
            .is_none()
        {
            return Err(ArchiveError::InvalidRarc);
        }
        for entry_index in first..first + count {
            let entry = add(
                entry_base,
                entry_index
                    .checked_mul(20)
                    .ok_or(ArchiveError::InvalidRarc)?,
            )?;
            slice(&data, entry, 20).map_err(|_| ArchiveError::InvalidRarc)?;
            let flags_and_name = be_u32(&data, entry + 4)?;
            let flags = flags_and_name >> 24;
            let entry_name =
                rarc_name(&data, string_base, (flags_and_name & 0x00ff_ffff) as usize)?;
            if entry_name == "." || entry_name == ".." {
                continue;
            }
            if flags & 0x02 != 0 {
                visit(
                    data,
                    data_base,
                    node_base,
                    entry_base,
                    string_base,
                    num_nodes,
                    num_entries,
                    visited,
                    files,
                    be_u32(data, entry + 8)? as usize,
                    &path,
                )?;
                continue;
            }
            let full = format!("{path}{entry_name}");
            let offset = add(data_base, be_u32(&data, entry + 8)? as usize)?;
            let length = be_u32(&data, entry + 12)? as usize;
            if offset
                .checked_add(length)
                .filter(|&end| end <= data.len())
                .is_none()
            {
                return Err(ArchiveError::InvalidRarc);
            }
            files.push((full, be_u16(&data, entry)?, offset, length));
        }
        Ok(())
    }

    visit(
        &data,
        data_base,
        node_base,
        entry_base,
        string_base,
        num_nodes,
        num_entries,
        &mut visited,
        &mut files,
        0,
        "",
    )?;
    Ok((data, files))
}

/// Find a file in a RARC path, accepting either a full path or a unique-style
/// basename suffix as the native `Dump.arc().find()` helper does.
pub fn rarc_find(data: &[u8], name: &str) -> Result<Vec<u8>, ArchiveError> {
    let (archive, files) = rarc_index(data)?;
    let expected = name.to_lowercase();
    for (path, _, offset, length) in files {
        let low = path.to_lowercase();
        if low == expected || low.ends_with(&format!("/{expected}")) {
            return yaz0_decompress(slice(&archive, offset, length)?);
        }
    }
    Err(ArchiveError::NotFound)
}

/// Find a file by its native 16-bit RARC file ID and decompress its body.
pub fn rarc_find_id(data: &[u8], file_id: u16) -> Result<Vec<u8>, ArchiveError> {
    let (archive, files) = rarc_index(data)?;
    for (_, id, offset, length) in files {
        if id == file_id {
            return yaz0_decompress(slice(&archive, offset, length)?);
        }
    }
    Err(ArchiveError::NotFound)
}

/// List the files directly under a RARC directory, matching the native stage
/// builder's `Arc.listdir()` behavior, and decompress each file's Yaz0 body.
pub fn rarc_listdir(data: &[u8], directory: &str) -> Result<Vec<(String, Vec<u8>)>, ArchiveError> {
    let (archive, files) = rarc_index(data)?;
    let expected = directory.trim_matches('/').to_lowercase();
    let mut result = Vec::new();
    for (path, _, offset, length) in files {
        let Some((parent, name)) = path.rsplit_once('/') else {
            continue;
        };
        let low_parent = parent.to_lowercase();
        let matches = low_parent == expected
            || low_parent
                .split_once('/')
                .is_some_and(|(_, suffix)| suffix == expected);
        if matches {
            result.push((
                name.to_owned(),
                yaz0_decompress(slice(&archive, offset, length)?)?,
            ));
        }
    }
    Ok(result)
}

/// List files in a named RARC directory and its descendants. RARC paths in
/// SMG stage archives place JMap tables several levels below `Jmp/Placement`.
pub fn rarc_listtree(data: &[u8], directory: &str) -> Result<Vec<(String, Vec<u8>)>, ArchiveError> {
    let (archive, files) = rarc_index(data)?;
    let expected = directory
        .trim_matches('/')
        .to_lowercase()
        .split('/')
        .map(str::to_owned)
        .collect::<Vec<_>>();
    let mut result = Vec::new();
    for (path, _, offset, length) in files {
        let Some((parent, _name)) = path.rsplit_once('/') else {
            continue;
        };
        let components = parent
            .to_lowercase()
            .split('/')
            .map(str::to_owned)
            .collect::<Vec<_>>();
        let contains_directory = components
            .windows(expected.len())
            .any(|window| window == expected.as_slice());
        if contains_directory {
            result.push((path, yaz0_decompress(slice(&archive, offset, length)?)?));
        }
    }
    Ok(result)
}

/// Read only matching archive files while retaining the archive's native file
/// order. This avoids inflating unrelated textures when a caller needs models
/// and collision meshes from an ObjectData archive.
pub fn rarc_listfiles_with_extensions(
    data: &[u8],
    extensions: &[&str],
) -> Result<Vec<(String, Vec<u8>)>, ArchiveError> {
    let (archive, files) = rarc_index(data)?;
    let mut result = Vec::new();
    for (path, _, offset, length) in files {
        let low = path.to_lowercase();
        if extensions.iter().any(|extension| low.ends_with(extension)) {
            result.push((path, yaz0_decompress(slice(&archive, offset, length)?)?));
        }
    }
    Ok(result)
}

#[derive(Debug, Clone, PartialEq)]
pub struct BcsvField {
    pub hash: u32,
    pub mask: u32,
    pub offset: u16,
    pub shift: u8,
    pub kind: u8,
}

#[derive(Debug, Clone, PartialEq)]
pub enum BcsvValue {
    Integer(i64),
    Float(f32),
    Text(String),
    Unknown,
}

#[derive(Debug, Clone, PartialEq)]
pub struct BcsvTable {
    pub fields: Vec<BcsvField>,
    pub rows: Vec<Vec<BcsvValue>>,
}

/// Decode BCSV field layouts and values used by the native stage builders.
/// Field hashes remain numeric so callers can add stage-specific name tables.
pub fn bcsv_decode(src: &[u8]) -> Result<BcsvTable, ArchiveError> {
    let data = yaz0_decompress(src)?;
    let count = be_i32(&data, 0).map_err(|_| ArchiveError::InvalidRarc)?;
    let field_count = be_i32(&data, 4).map_err(|_| ArchiveError::InvalidRarc)?;
    let data_offset = be_i32(&data, 8).map_err(|_| ArchiveError::InvalidRarc)?;
    let row_size = be_u32(&data, 12).map_err(|_| ArchiveError::InvalidRarc)? as usize;
    if count < 0 || field_count < 0 || data_offset < 0 || row_size == 0 {
        return Err(ArchiveError::InvalidRarc);
    }
    let count = count as usize;
    let field_count = field_count as usize;
    let data_offset = data_offset as usize;
    let field_end = 16usize
        .checked_add(field_count.checked_mul(12).ok_or(ArchiveError::Truncated)?)
        .ok_or(ArchiveError::Truncated)?;
    let rows_end = data_offset
        .checked_add(count.checked_mul(row_size).ok_or(ArchiveError::Truncated)?)
        .ok_or(ArchiveError::Truncated)?;
    let strings = rows_end;
    if field_end > data.len() || rows_end > data.len() || data_offset < field_end {
        return Err(ArchiveError::InvalidRarc);
    }
    let mut fields = Vec::with_capacity(field_count);
    for i in 0..field_count {
        let p = 16 + i * 12;
        let field = BcsvField {
            hash: be_u32(&data, p)?,
            mask: be_u32(&data, p + 4)?,
            offset: be_u16(&data, p + 8)?,
            shift: *data.get(p + 10).ok_or(ArchiveError::Truncated)?,
            kind: *data.get(p + 11).ok_or(ArchiveError::Truncated)?,
        };
        let width = match field.kind {
            0 | 2 | 3 | 6 => 4,
            4 => 2,
            5 => 1,
            1 => 32,
            _ => 0,
        };
        if width != 0
            && (field.offset as usize)
                .checked_add(width)
                .filter(|&end| end <= row_size)
                .is_none()
        {
            return Err(ArchiveError::InvalidRarc);
        }
        fields.push(field);
    }
    let mut rows = Vec::with_capacity(count);
    for i in 0..count {
        let row = data_offset
            .checked_add(i.checked_mul(row_size).ok_or(ArchiveError::Truncated)?)
            .ok_or(ArchiveError::Truncated)?;
        let mut values = Vec::with_capacity(field_count);
        for field in &fields {
            let p = row
                .checked_add(field.offset as usize)
                .ok_or(ArchiveError::Truncated)?;
            let value = match field.kind {
                0 | 3 => {
                    let mut v = ((be_u32(&data, p)? & field.mask) >> field.shift) as i64;
                    if field.mask == u32::MAX && field.shift == 0 {
                        v = v as i32 as i64;
                    }
                    BcsvValue::Integer(v)
                }
                4 => {
                    let mut v = ((be_u16(&data, p)? as u32 & field.mask) >> field.shift) as i64;
                    if field.mask == u16::MAX as u32 && field.shift == 0 {
                        v = v as i16 as i64;
                    }
                    BcsvValue::Integer(v)
                }
                5 => {
                    let mut v = ((*data.get(p).ok_or(ArchiveError::Truncated)? as u32 & field.mask)
                        >> field.shift) as i64;
                    if field.mask == u8::MAX as u32 && field.shift == 0 {
                        v = v as i8 as i64;
                    }
                    BcsvValue::Integer(v)
                }
                2 => BcsvValue::Float(be_f32(&data, p)?),
                1 => {
                    let bytes = slice(&data, p, 32)?;
                    let end = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
                    BcsvValue::Text(
                        encoding_rs::SHIFT_JIS
                            .decode_without_bom_handling(&bytes[..end])
                            .0
                            .into_owned(),
                    )
                }
                6 => {
                    let relative = be_u32(&data, p)? as usize;
                    let start = strings
                        .checked_add(relative)
                        .ok_or(ArchiveError::Truncated)?;
                    let tail = data.get(start..).ok_or(ArchiveError::Truncated)?;
                    let end = tail
                        .iter()
                        .position(|&b| b == 0)
                        .ok_or(ArchiveError::Truncated)?;
                    BcsvValue::Text(
                        encoding_rs::SHIFT_JIS
                            .decode_without_bom_handling(&tail[..end])
                            .0
                            .into_owned(),
                    )
                }
                _ => BcsvValue::Unknown,
            };
            values.push(value);
        }
        rows.push(values);
    }
    Ok(BcsvTable { fields, rows })
}

/// Read the BCSV particle-name table into name-to-ID pairs, matching the
/// `particle_ids()` helper used by `asset_construction/procedural.py`.
pub fn particle_ids(data: &[u8]) -> Result<Vec<(Vec<u8>, i32)>, ArchiveError> {
    let count = be_i32(data, 0)?;
    let field_count = be_i32(data, 4)?;
    let rows = be_i32(data, 8)?;
    let row_size = be_u32(data, 12)? as usize;
    if count < 0 || field_count < 0 || rows < 0 || row_size == 0 {
        return Err(ArchiveError::InvalidRarc);
    }
    let count = count as usize;
    let field_count = field_count as usize;
    let rows = rows as usize;
    let mut name_offset = None;
    let mut id_offset = None;
    for i in 0..field_count {
        let field = 16usize
            .checked_add(i.checked_mul(12).ok_or(ArchiveError::Truncated)?)
            .ok_or(ArchiveError::Truncated)?;
        let hash = be_u32(data, field)?;
        let offset = be_u16(data, field + 8)? as usize;
        match hash {
            // The BCSV key hash is the same signed-byte polynomial hash as
            // `jhash()`; these two keys are ASCII.
            0x0033_7a8b => name_offset = Some(offset),
            0x0000_0d1b => id_offset = Some(offset),
            _ => {}
        }
    }
    let name_offset = name_offset.ok_or(ArchiveError::InvalidRarc)?;
    let id_offset = id_offset.ok_or(ArchiveError::InvalidRarc)?;
    let rows_base = rows;
    let strings_base = rows_base
        .checked_add(count.checked_mul(row_size).ok_or(ArchiveError::Truncated)?)
        .ok_or(ArchiveError::Truncated)?;
    let mut out = Vec::with_capacity(count);
    for i in 0..count {
        let row = rows_base
            .checked_add(i.checked_mul(row_size).ok_or(ArchiveError::Truncated)?)
            .ok_or(ArchiveError::Truncated)?;
        let name_rel = be_u32(
            data,
            row.checked_add(name_offset)
                .ok_or(ArchiveError::Truncated)?,
        )? as usize;
        let name_start = strings_base
            .checked_add(name_rel)
            .ok_or(ArchiveError::Truncated)?;
        let tail = data.get(name_start..).ok_or(ArchiveError::Truncated)?;
        let name_end = tail
            .iter()
            .position(|&b| b == 0)
            .ok_or(ArchiveError::Truncated)?;
        let id = be_i32(
            data,
            row.checked_add(id_offset).ok_or(ArchiveError::Truncated)?,
        )?;
        out.push((tail[..name_end].to_vec(), id));
    }
    Ok(out)
}

/// Read BSP1 particle dimensions keyed by JPA resource ID, matching the
/// `particle_sizes()` helper used by `asset_construction/procedural.py`.
pub fn particle_sizes(data: &[u8]) -> Result<Vec<(u16, f32, f32)>, ArchiveError> {
    let count = be_u16(data, 8)? as usize;
    let mut p = 16usize;
    let mut out = Vec::new();
    for _ in 0..count {
        let id = be_u16(data, p)?;
        let blocks = be_u16(data, p + 2)? as usize;
        p = add(p, 8)?;
        for _ in 0..blocks {
            let header = slice(data, p, 8)?;
            let size = be_u32(header, 4)? as usize;
            if size < 8 {
                return Err(ArchiveError::InvalidRarc);
            }
            if &header[..4] == b"BSP1" {
                if size < 24 {
                    return Err(ArchiveError::InvalidRarc);
                }
                let width = be_f32(data, p + 16)?;
                let height = be_f32(data, p + 20)?;
                if let Some(existing) = out.iter_mut().find(|(old_id, _, _)| *old_id == id) {
                    *existing = (id, width, height);
                } else {
                    out.push((id, width, height));
                }
            }
            p = add(p, size)?;
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pass_through_unwrapped_data() {
        assert_eq!(yaz0_decompress(b"plain bytes").unwrap(), b"plain bytes");
    }

    #[test]
    fn decode_yaz0_literals_and_backreferences() {
        // Literal "abc", followed by a three-byte backreference to "abc".
        let mut input = b"Yaz0\0\0\0\x06\0\0\0\0\0\0\0\0".to_vec();
        input.extend_from_slice(&[0xe0, b'a', b'b', b'c', 0x10, 0x02]);
        assert_eq!(yaz0_decompress(&input).unwrap(), b"abcabc");
    }

    #[test]
    fn rejects_bad_backreference() {
        let mut input = b"Yaz0\0\0\0\x03\0\0\0\0\0\0\0\0".to_vec();
        input.extend_from_slice(&[0x00, 0x00, 0x00]);
        assert_eq!(yaz0_decompress(&input), Err(ArchiveError::InvalidYaz0));
    }
}

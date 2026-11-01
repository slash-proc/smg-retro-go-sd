//! Decoder for the compact line streams stored in stage construction recipes.
//!
//! Stage recipes store line references, token edits, and fixed-point numeric
//! residuals. This is the common layer used by the stage OBJ/MTL and galaxy
//! text reconstruction paths.

use std::collections::BTreeMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecipeError {
    MissingStream,
    InvalidStream,
    InvalidReference,
    InvalidNumber,
    InvalidUtf8,
    InvalidMetadata,
    PackMissing,
    FileMissing,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StageFile {
    pub name: String,
    pub lines: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StageSource {
    pub archive: String,
    pub object_index: usize,
    pub kinds: Vec<String>,
}

/// Metadata for a compressed stage recipe embedded by `build.rs`.
pub struct StageRecipe {
    pack_name: &'static str,
    pub galaxy: String,
    pub files: Vec<StageFile>,
    pub sources: Vec<StageSource>,
}

impl StageRecipe {
    pub fn load(pack_name: &str) -> Result<Self, RecipeError> {
        let pack = crate::asset_packs::EMBEDDED_PACKS
            .iter()
            .find(|pack| pack.name == pack_name)
            .ok_or(RecipeError::PackMissing)?;
        let (_, offset, size) = pack
            .entries
            .iter()
            .find(|(name, _, _)| *name == "meta")
            .ok_or(RecipeError::InvalidMetadata)?;
        let end = offset
            .checked_add(*size)
            .ok_or(RecipeError::InvalidMetadata)?;
        let metadata: serde_json::Value = serde_json::from_slice(
            pack.data
                .get(*offset..end)
                .ok_or(RecipeError::InvalidMetadata)?,
        )
        .map_err(|_| RecipeError::InvalidMetadata)?;
        let galaxy = metadata
            .get("galaxy")
            .and_then(serde_json::Value::as_str)
            .ok_or(RecipeError::InvalidMetadata)?
            .to_owned();
        let file_rows = metadata
            .get("files")
            .and_then(serde_json::Value::as_array)
            .ok_or(RecipeError::InvalidMetadata)?;
        let mut files = Vec::with_capacity(file_rows.len());
        for row in file_rows {
            let row = row.as_array().ok_or(RecipeError::InvalidMetadata)?;
            let name = row
                .first()
                .and_then(serde_json::Value::as_str)
                .ok_or(RecipeError::InvalidMetadata)?;
            let lines = row
                .get(1)
                .and_then(serde_json::Value::as_u64)
                .and_then(|lines| usize::try_from(lines).ok())
                .ok_or(RecipeError::InvalidMetadata)?;
            files.push(StageFile {
                name: name.to_owned(),
                lines,
            });
        }
        let source_rows = metadata
            .get("sources")
            .and_then(serde_json::Value::as_array)
            .ok_or(RecipeError::InvalidMetadata)?;
        let mut sources = Vec::with_capacity(source_rows.len());
        for row in source_rows {
            let row = row.as_array().ok_or(RecipeError::InvalidMetadata)?;
            let archive = row
                .first()
                .and_then(serde_json::Value::as_str)
                .ok_or(RecipeError::InvalidMetadata)?;
            let object_index = row
                .get(1)
                .and_then(serde_json::Value::as_u64)
                .and_then(|index| usize::try_from(index).ok())
                .ok_or(RecipeError::InvalidMetadata)?;
            let kinds = row
                .get(2)
                .and_then(serde_json::Value::as_array)
                .ok_or(RecipeError::InvalidMetadata)?
                .iter()
                .map(|kind| {
                    kind.as_str()
                        .map(str::to_owned)
                        .ok_or(RecipeError::InvalidMetadata)
                })
                .collect::<Result<Vec<_>, _>>()?;
            sources.push(StageSource {
                archive: archive.to_owned(),
                object_index,
                kinds,
            });
        }
        Ok(Self {
            pack_name: pack.name,
            galaxy,
            files,
            sources,
        })
    }

    /// Unique ObjectData archives used to reconstruct this stage's geometry.
    pub fn source_archives(&self) -> Vec<String> {
        let mut archives = self
            .sources
            .iter()
            .map(|source| format!("ObjectData/{}.arc", source.archive))
            .collect::<Vec<_>>();
        archives.sort();
        archives.dedup();
        archives
    }

    pub fn pack_name(&self) -> &str {
        self.pack_name
    }

    /// Decode one file's text lines without copying its embedded stream bytes.
    pub fn decode_file(
        &self,
        file_name: &str,
        candidates: &[String],
    ) -> Result<Vec<String>, RecipeError> {
        let (count, mut streams) = self.file_streams(file_name)?;
        decode_lines_from_streams(&mut streams, count, candidates)
    }

    /// Return a prevalidated literal descriptor bundled with this recipe.
    pub fn literal_file(&self, file_name: &str) -> Result<Vec<u8>, RecipeError> {
        let key = format!("{file_name}/native");
        let pack = crate::asset_packs::EMBEDDED_PACKS
            .iter()
            .find(|pack| pack.name == self.pack_name)
            .ok_or(RecipeError::PackMissing)?;
        let (_, offset, size) = pack
            .entries
            .iter()
            .find(|(name, _, _)| *name == key)
            .ok_or(RecipeError::FileMissing)?;
        let end = offset
            .checked_add(*size)
            .ok_or(RecipeError::InvalidMetadata)?;
        Ok(pack
            .data
            .get(*offset..end)
            .ok_or(RecipeError::InvalidMetadata)?
            .to_vec())
    }

    pub(crate) fn file_streams(
        &self,
        file_name: &str,
    ) -> Result<(usize, Streams<'static>), RecipeError> {
        let file = self
            .files
            .iter()
            .find(|file| file.name == file_name)
            .ok_or(RecipeError::FileMissing)?;
        let prefix = format!("{file_name}/");
        let pack = crate::asset_packs::EMBEDDED_PACKS
            .iter()
            .find(|pack| pack.name == self.pack_name)
            .ok_or(RecipeError::PackMissing)?;
        let mut streams = BTreeMap::new();
        for (name, offset, size) in pack.entries {
            let Some(stream_name) = name.strip_prefix(&prefix) else {
                continue;
            };
            let end = offset
                .checked_add(*size)
                .ok_or(RecipeError::InvalidMetadata)?;
            let bytes = pack
                .data
                .get(*offset..end)
                .ok_or(RecipeError::InvalidMetadata)?;
            streams.insert(stream_name, bytes);
        }
        Ok((file.lines, Streams::new(&streams)))
    }
}

/// Unique ObjectData inputs required by every embedded stage recipe.
pub fn all_stage_source_archives() -> Result<Vec<String>, RecipeError> {
    let mut archives = Vec::new();
    for pack in crate::asset_packs::EMBEDDED_PACKS
        .iter()
        .filter(|pack| pack.name.starts_with("stages/"))
    {
        archives.extend(StageRecipe::load(pack.name)?.source_archives());
    }
    archives.sort();
    archives.dedup();
    Ok(archives)
}

struct Reader<'a> {
    bytes: &'a [u8],
    cursor: usize,
}

impl<'a> Reader<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, cursor: 0 }
    }

    fn int(&mut self) -> Result<i64, RecipeError> {
        let mut value = 0u64;
        let mut shift = 0u32;
        loop {
            let byte = *self
                .bytes
                .get(self.cursor)
                .ok_or(RecipeError::InvalidStream)?;
            self.cursor += 1;
            if shift >= 64 || (shift == 63 && byte & 0x7e != 0) {
                return Err(RecipeError::InvalidStream);
            }
            value |= ((byte & 0x7f) as u64) << shift;
            if byte < 0x80 {
                break;
            }
            shift += 7;
        }
        Ok(((value >> 1) as i64) ^ (-((value & 1) as i64)))
    }
}

pub(crate) struct Streams<'a> {
    readers: BTreeMap<&'a str, Reader<'a>>,
}

impl<'a> Streams<'a> {
    fn new(streams: &BTreeMap<&'a str, &'a [u8]>) -> Self {
        Self {
            readers: streams
                .iter()
                .map(|(&name, &bytes)| (name, Reader::new(bytes)))
                .collect(),
        }
    }

    pub(crate) fn int(&mut self, name: &'static str) -> Result<i64, RecipeError> {
        self.readers
            .get_mut(name)
            .ok_or(RecipeError::MissingStream)?
            .int()
    }

    pub(crate) fn text(&mut self, name: &'static str) -> Result<String, RecipeError> {
        let len = usize::try_from(self.int(name)?).map_err(|_| RecipeError::InvalidStream)?;
        let mut bytes = Vec::with_capacity(len);
        for _ in 0..len {
            bytes.push(u8::try_from(self.int(name)?).map_err(|_| RecipeError::InvalidStream)?);
        }
        String::from_utf8(bytes).map_err(|_| RecipeError::InvalidUtf8)
    }
}

#[derive(Clone, Copy)]
struct Number {
    value: i64,
    decimals: u32,
    negative_zero: bool,
}

fn tokenize(line: &str) -> Vec<String> {
    let bytes = line.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        let start = i;
        let character = line[i..].chars().next().unwrap();
        let negative = bytes[i] == b'-' && bytes.get(i + 1).is_some_and(u8::is_ascii_digit);
        let digit_start = i + usize::from(negative);
        let starts_number = bytes[digit_start].is_ascii_digit()
            && (bytes[digit_start] != b'0'
                || bytes
                    .get(digit_start + 1)
                    .is_none_or(|b| !b.is_ascii_digit()));
        if starts_number {
            i = digit_start;
            while bytes.get(i).is_some_and(u8::is_ascii_digit) {
                i += 1;
            }
            if bytes.get(i) == Some(&b'.') && bytes.get(i + 1).is_some_and(u8::is_ascii_digit) {
                i += 1;
                while bytes.get(i).is_some_and(u8::is_ascii_digit) {
                    i += 1;
                }
            }
            out.push(line[start..i].to_owned());
        } else if character.is_ascii_alphabetic() {
            i += 1;
            while bytes.get(i).is_some_and(u8::is_ascii_alphabetic) {
                i += 1;
            }
            out.push(line[start..i].to_owned());
        } else {
            i += character.len_utf8();
            out.push(line[start..i].to_owned());
        }
    }
    out
}

fn parse_number(token: &str) -> Option<Number> {
    let (negative, body) = token
        .strip_prefix('-')
        .map_or((false, token), |body| (true, body));
    let (digits, decimals) = if let Some((whole, fraction)) = body.split_once('.') {
        (format!("{whole}{fraction}"), fraction.len())
    } else {
        (body.to_owned(), 0)
    };
    let value = digits.parse::<i64>().ok()?;
    Some(Number {
        value: if negative { -value } else { value },
        decimals: decimals as u32,
        negative_zero: negative && value == 0,
    })
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

fn rescale(value: i64, source: u32, destination: u32) -> Result<i64, RecipeError> {
    if destination >= source {
        return value
            .checked_mul(10i64.pow(destination - source))
            .ok_or(RecipeError::InvalidNumber);
    }
    let divisor = 10i64.pow(source - destination);
    value
        .checked_add(divisor / 2)
        .map(|rounded| rounded.div_euclid(divisor))
        .ok_or(RecipeError::InvalidNumber)
}

fn number_from(streams: &mut Streams<'_>, reference: &str) -> Result<String, RecipeError> {
    let number = parse_number(reference).ok_or(RecipeError::InvalidNumber)?;
    let code = streams.int("ln.res")?;
    let encoded_residual = code >> 2;
    let residual = (encoded_residual >> 1) ^ -(encoded_residual & 1);
    if code & 2 != 0 {
        let decimals =
            u32::try_from(streams.int("ln.dec")?).map_err(|_| RecipeError::InvalidNumber)?;
        let value = rescale(number.value, number.decimals, decimals)?
            .checked_add(residual)
            .ok_or(RecipeError::InvalidNumber)?;
        Ok(format_number(value, decimals, code & 1 != 0))
    } else {
        let value = number
            .value
            .checked_add(residual)
            .ok_or(RecipeError::InvalidNumber)?;
        Ok(format_number(
            value,
            number.decimals,
            code & 1 != 0 || number.negative_zero,
        ))
    }
}

fn decode_line(
    streams: &mut Streams<'_>,
    reference: &str,
    strings: &[String],
) -> Result<String, RecipeError> {
    let tokens = tokenize(reference);
    let numbers = tokens
        .iter()
        .filter(|token| parse_number(token).is_some())
        .cloned()
        .collect::<Vec<_>>();
    let mut cursor = 0usize;
    let mut out = String::new();
    let operations =
        usize::try_from(streams.int("ln.nops")?).map_err(|_| RecipeError::InvalidStream)?;
    for _ in 0..operations {
        let kind = streams.int("ln.op")?;
        let count =
            usize::try_from(streams.int("ln.n")?).map_err(|_| RecipeError::InvalidStream)?;
        match kind {
            0 => {
                let end = cursor
                    .checked_add(count)
                    .filter(|&end| end <= tokens.len())
                    .ok_or(RecipeError::InvalidStream)?;
                for token in &tokens[cursor..end] {
                    if parse_number(token).is_some() {
                        out.push_str(&number_from(streams, token)?);
                    } else {
                        out.push_str(token);
                    }
                }
                cursor = end;
            }
            1 => {
                cursor = cursor
                    .checked_add(count)
                    .filter(|&end| end <= tokens.len())
                    .ok_or(RecipeError::InvalidStream)?;
            }
            _ => {
                for _ in 0..count {
                    let code = streams.int("ln.ins")?;
                    if code > 0 {
                        let value = numbers
                            .get((code - 1) as usize)
                            .ok_or(RecipeError::InvalidReference)?;
                        out.push_str(&number_from(streams, value)?);
                    } else if code == 0 {
                        let encoded = streams.int("ln.lit")?;
                        let literal = encoded >> 1;
                        let value = (literal >> 1) ^ -(literal & 1);
                        let decimals = u32::try_from(streams.int("ln.litdec")?)
                            .map_err(|_| RecipeError::InvalidNumber)?;
                        out.push_str(&format_number(value, decimals, encoded & 1 != 0));
                    } else {
                        let index = usize::try_from(streams.int("ln.str")?)
                            .map_err(|_| RecipeError::InvalidReference)?;
                        out.push_str(strings.get(index).ok_or(RecipeError::InvalidReference)?);
                    }
                }
            }
        }
    }
    Ok(out)
}

/// Decode `count` lines using the stage recipe's shared line streams.
pub fn decode_lines(
    streams: &BTreeMap<&str, &[u8]>,
    count: usize,
    candidates: &[String],
) -> Result<Vec<String>, RecipeError> {
    let mut streams = Streams::new(streams);
    decode_lines_from_streams(&mut streams, count, candidates)
}

pub(crate) fn decode_lines_from_streams(
    streams: &mut Streams<'_>,
    count: usize,
    candidates: &[String],
) -> Result<Vec<String>, RecipeError> {
    let string_count =
        usize::try_from(streams.int("ls.nstr")?).map_err(|_| RecipeError::InvalidStream)?;
    let mut strings = Vec::with_capacity(string_count);
    for _ in 0..string_count {
        strings.push(streams.text("ls.strs")?);
    }
    let mut output = Vec::with_capacity(count);
    let mut previous = -1i64;
    for _ in 0..count {
        let delta = streams.int("ls.ref")?;
        let reference = if delta == -(1 << 30) {
            -1
        } else {
            previous
                .checked_add(1)
                .and_then(|base| base.checked_add(delta))
                .ok_or(RecipeError::InvalidReference)?
        };
        let exact = streams.int("ls.exact")? != 0;
        if exact {
            output.push(
                candidates
                    .get(usize::try_from(reference).map_err(|_| RecipeError::InvalidReference)?)
                    .ok_or(RecipeError::InvalidReference)?
                    .clone(),
            );
        } else {
            let source = if reference < 0 {
                String::new()
            } else {
                candidates
                    .get(reference as usize)
                    .ok_or(RecipeError::InvalidReference)?
                    .clone()
            };
            output.push(decode_line(streams, &source, &strings)?);
        }
        if reference >= 0 {
            previous = reference;
        }
    }
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_a_numeric_line_with_zero_residuals() {
        // Every stream value is zero except the first line-op descriptor.
        // Signed zero is encoded as an unsigned zero varint.
        let streams = BTreeMap::from([
            ("ls.nstr", &[0][..]),
            ("ls.ref", &[0][..]),
            ("ls.exact", &[0][..]),
            ("ln.nops", &[2][..]), // zigzag(1)
            ("ln.op", &[0][..]),
            ("ln.n", &[10][..]), // zigzag(5)
            ("ln.res", &[0, 0][..]),
        ]);
        let candidates = vec!["v 1 -2.50".to_owned()];
        assert_eq!(
            decode_lines(&streams, 1, &candidates).unwrap(),
            ["v 1 -2.50"]
        );
    }

    #[test]
    fn decodes_double_encoded_integer_literals() {
        let streams = BTreeMap::from([
            ("ls.nstr", &[0][..]),
            ("ls.ref", &[255, 255, 255, 255, 7][..]), // sentinel: -2^30
            ("ls.exact", &[0][..]),
            ("ln.nops", &[2][..]), // one operation
            ("ln.op", &[4][..]),   // insert
            ("ln.n", &[2][..]),
            ("ln.ins", &[0][..]),
            ("ln.lit", &[24][..]), // literal payload 12 => integer 3
            ("ln.litdec", &[0][..]),
        ]);
        assert_eq!(decode_lines(&streams, 1, &[]).unwrap(), ["3"]);
    }

    #[test]
    fn fixed_point_rescale_matches_floor_rounding_for_negative_values() {
        assert_eq!(rescale(-15, 1, 0).unwrap(), -1);
        assert_eq!(rescale(-16, 1, 0).unwrap(), -2);
        assert_eq!(format_number(-25, 2, false), "-0.25");
    }

    #[test]
    fn tokenizes_multibyte_candidate_text_without_splitting_utf8() {
        assert_eq!(
            tokenize("カメラ x 1"),
            ["カ", "メ", "ラ", " ", "x", " ", "1"]
        );
    }

    #[test]
    fn preserves_numeric_residual_and_negative_zero_metadata() {
        let streams = BTreeMap::from([("ln.res", &[8, 2][..])]);
        let mut streams = Streams::new(&streams);
        assert_eq!(number_from(&mut streams, "1.00").unwrap(), "0.99");
        assert_eq!(number_from(&mut streams, "0").unwrap(), "-0");
    }

    #[test]
    fn loads_embedded_stage_recipe_metadata() {
        let recipe = StageRecipe::load("stages/freezeflame.pak").unwrap();
        assert_eq!(recipe.galaxy, "IceVolcanoGalaxy");
        let obj = recipe
            .files
            .iter()
            .find(|file| file.name == "freezeflame.obj")
            .unwrap();
        assert_eq!(obj.lines, 102_148);
        assert!(recipe.sources.iter().any(|source| {
            source.archive == "LavaMiniSunPlanet"
                && source.object_index == 39
                && source.kinds == ["mdl", "kcl"]
        }));
        assert!(recipe
            .source_archives()
            .iter()
            .any(|path| path == "ObjectData/IceRingFirePlanet.arc"));
    }
}

//! Flatten the generated asset assembly into the exact section byte stream.
//!
//! This is the Rust counterpart of `tools/assets/assembly.py::Assembly`. It is
//! deliberately independent of filesystem access so the same assembler can be
//! used by the import-free WASM converter with an in-memory blob provider.

use std::collections::HashMap;

#[derive(Default)]
struct Section {
    alignment: usize,
    bytes: Vec<u8>,
}

#[derive(Clone)]
struct Label {
    section: usize,
    offset: usize,
}

struct Word {
    section: usize,
    offset: usize,
    expression: String,
    absolute_pointer: bool,
}

struct Alias {
    name: String,
    expression: String,
}

pub struct AssemblyLayout {
    pub data: Vec<u8>,
    pub symbols: HashMap<String, usize>,
    pub words: HashMap<usize, String>,
    pub statements: Vec<AssemblyStatement>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum AssemblyStatementKind {
    Other,
    Section,
    Label,
    Alias,
    Align,
    Incbin,
    Word,
}

pub struct AssemblyStatement {
    pub kind: AssemblyStatementKind,
    pub text: String,
    pub section: Option<String>,
    pub address: usize,
    pub size: usize,
    pub start: usize,
    pub name: Option<String>,
    pub expression: Option<String>,
}

/// Resolve `.incbin` blobs, lay out sections, and evaluate `.word` expressions.
///
/// The supported syntax intentionally matches the checked-in generator's
/// assembly subset: `.section`, `.global`, labels, `.set`, `.balign`,
/// `.incbin`, and `.word`.
pub fn assemble<F>(text: &str, asset_base: u32, mut read_blob: F) -> Result<Vec<u8>, String>
where
    F: FnMut(&str) -> Result<Vec<u8>, String>,
{
    Ok(assemble_layout(text, asset_base, read_blob)?.data)
}

pub fn assemble_layout<F>(
    text: &str,
    asset_base: u32,
    mut read_blob: F,
) -> Result<AssemblyLayout, String>
where
    F: FnMut(&str) -> Result<Vec<u8>, String>,
{
    let mut sections: Vec<Section> = Vec::new();
    let mut section_ids = HashMap::new();
    let mut labels: HashMap<String, Label> = HashMap::new();
    let mut aliases = Vec::new();
    let mut words = Vec::new();
    let mut statements = Vec::new();
    let mut blobs: HashMap<String, Vec<u8>> = HashMap::new();
    let mut current_section = None;

    for (line_number, line) in text.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('@') {
            statements.push(AssemblyStatement {
                kind: AssemblyStatementKind::Other,
                text: line.to_owned(),
                section: current_section.map(|index| section_name(&section_ids, index)),
                address: current_section.map_or(0, |index| sections[index].bytes.len()),
                size: 0,
                start: 0,
                name: None,
                expression: None,
            });
            continue;
        }

        if let Some(rest) = line.strip_prefix(".section") {
            let name = rest
                .trim_start()
                .split(|ch: char| ch == ',' || ch.is_whitespace())
                .next()
                .filter(|name| !name.is_empty())
                .ok_or_else(|| format!("line {}: malformed .section", line_number + 1))?;
            let section = if let Some(&index) = section_ids.get(name) {
                index
            } else {
                let index = sections.len();
                sections.push(Section {
                    alignment: 1,
                    bytes: Vec::new(),
                });
                section_ids.insert(name.to_owned(), index);
                index
            };
            current_section = Some(section);
            statements.push(AssemblyStatement {
                kind: AssemblyStatementKind::Section,
                text: line.to_owned(),
                section: Some(name.to_owned()),
                address: sections[section].bytes.len(),
                size: 0,
                start: 0,
                name: None,
                expression: None,
            });
            continue;
        }

        if line.starts_with(".global") {
            let section = current_section;
            statements.push(AssemblyStatement {
                kind: AssemblyStatementKind::Other,
                text: line.to_owned(),
                section: section.map(|index| section_name(&section_ids, index)),
                address: section.map_or(0, |index| sections[index].bytes.len()),
                size: 0,
                start: 0,
                name: None,
                expression: None,
            });
            continue;
        }

        if let Some(rest) = line.strip_prefix(".set") {
            let (name, expression) = rest
                .split_once(',')
                .ok_or_else(|| format!("line {}: malformed .set", line_number + 1))?;
            aliases.push(Alias {
                name: name.trim().to_owned(),
                expression: expression.trim().to_owned(),
            });
            statements.push(AssemblyStatement {
                kind: AssemblyStatementKind::Alias,
                text: line.to_owned(),
                section: current_section.map(|index| section_name(&section_ids, index)),
                address: current_section.map_or(0, |index| sections[index].bytes.len()),
                size: 0,
                start: 0,
                name: None,
                expression: None,
            });
            continue;
        }

        let section_index = current_section
            .ok_or_else(|| format!("line {}: statement before first .section", line_number + 1))?;
        let section = &mut sections[section_index];

        let mut statement = AssemblyStatement {
            kind: AssemblyStatementKind::Other,
            text: line.to_owned(),
            section: Some(section_name(&section_ids, section_index)),
            address: section.bytes.len(),
            size: 0,
            start: 0,
            name: None,
            expression: None,
        };
        if let Some(label) = line.strip_suffix(':') {
            statement.kind = AssemblyStatementKind::Label;
            statement.name = Some(label.to_owned());
            if labels
                .insert(
                    label.to_owned(),
                    Label {
                        section: section_index,
                        offset: section.bytes.len(),
                    },
                )
                .is_some()
            {
                return Err(format!("line {}: duplicate label {label}", line_number + 1));
            }
        } else if let Some(rest) = line.strip_prefix(".balign") {
            statement.kind = AssemblyStatementKind::Align;
            let alignment: usize = rest
                .trim()
                .parse()
                .map_err(|_| format!("line {}: invalid .balign", line_number + 1))?;
            if alignment == 0 || !alignment.is_power_of_two() {
                return Err(format!(
                    "line {}: unsupported .balign {alignment}",
                    line_number + 1
                ));
            }
            section.alignment = section.alignment.max(alignment);
            let padding = (alignment - section.bytes.len() % alignment) % alignment;
            section.bytes.resize(section.bytes.len() + padding, 0);
        } else if line.starts_with(".incbin") {
            let (filename, start, length) =
                parse_incbin(line).map_err(|error| format!("line {}: {error}", line_number + 1))?;
            if !blobs.contains_key(filename) {
                blobs.insert(filename.to_owned(), read_blob(filename)?);
            }
            let blob = &blobs[filename];
            if start > blob.len() {
                return Err(format!(
                    "line {}: .incbin offset {start} exceeds {filename} length {}",
                    line_number + 1,
                    blob.len()
                ));
            }
            let end = match length {
                Some(length) => start
                    .checked_add(length)
                    .filter(|&end| end <= blob.len())
                    .ok_or_else(|| {
                        format!("line {}: .incbin range exceeds {filename}", line_number + 1)
                    })?,
                None => blob.len(),
            };
            section.bytes.extend_from_slice(&blob[start..end]);
            statement.kind = AssemblyStatementKind::Incbin;
            statement.name = Some(filename.to_owned());
            statement.start = start;
            statement.size = end - start;
        } else if let Some(expression) = line.strip_prefix(".word ") {
            let offset = section.bytes.len();
            section.bytes.extend_from_slice(&[0; 4]);
            words.push(Word {
                section: section_index,
                offset,
                absolute_pointer: expression.contains('-') == false
                    && expression
                        .bytes()
                        .any(|byte| byte.is_ascii_alphabetic() || byte == b'_'),
                expression: expression.trim().to_owned(),
            });
            statement.kind = AssemblyStatementKind::Word;
            statement.size = 4;
            statement.expression = Some(expression.trim().to_owned());
        } else {
            return Err(format!(
                "line {}: unsupported assembly statement: {line}",
                line_number + 1
            ));
        }
        statements.push(statement);
    }

    let mut bases = Vec::with_capacity(sections.len());
    let mut total = 0usize;
    for section in &sections {
        total = align_up(total, section.alignment)?;
        bases.push(total);
        total = total
            .checked_add(section.bytes.len())
            .ok_or_else(|| "asset section layout overflow".to_owned())?;
    }

    let mut symbols = HashMap::with_capacity(labels.len() + aliases.len());
    for (name, label) in labels {
        let address = bases[label.section]
            .checked_add(label.offset)
            .ok_or_else(|| "asset symbol address overflow".to_owned())?;
        symbols.insert(name, address as i64);
    }
    for alias in aliases {
        let value = eval_expression(&alias.expression, &symbols)?;
        symbols.insert(alias.name, value);
    }

    let mut word_expressions = HashMap::with_capacity(words.len());
    for word in &words {
        let mut value = eval_expression(&word.expression, &symbols)?;
        if word.absolute_pointer {
            value = value
                .checked_add(asset_base as i64)
                .ok_or_else(|| format!("asset pointer overflow: {}", word.expression))?;
        }
        let value = value as u32;
        let at = word.offset;
        sections[word.section].bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
        word_expressions.insert(bases[word.section] + word.offset, word.expression.clone());
    }

    let mut output = vec![0; total];
    for (section, &base) in sections.iter().zip(&bases) {
        output[base..base + section.bytes.len()].copy_from_slice(&section.bytes);
    }
    let section_base =
        |name: &str| -> Option<usize> { section_ids.get(name).map(|index| bases[*index]) };
    for statement in &mut statements {
        if let Some(section) = statement.section.as_deref() {
            statement.address += section_base(section).unwrap_or(0);
        }
    }
    Ok(AssemblyLayout {
        data: output,
        symbols: symbols
            .into_iter()
            .map(|(name, value)| (name, value as usize))
            .collect(),
        words: word_expressions,
        statements,
    })
}

fn section_name(sections: &HashMap<String, usize>, index: usize) -> String {
    sections
        .iter()
        .find_map(|(name, value)| (*value == index).then(|| name.clone()))
        .unwrap_or_default()
}

fn align_up(value: usize, alignment: usize) -> Result<usize, String> {
    let padding = (alignment - value % alignment) % alignment;
    value
        .checked_add(padding)
        .ok_or_else(|| "asset section alignment overflow".to_owned())
}

fn parse_incbin(line: &str) -> Result<(&str, usize, Option<usize>), String> {
    let rest = line
        .strip_prefix(".incbin")
        .ok_or_else(|| "malformed .incbin".to_owned())?
        .trim();
    let after_open = rest
        .strip_prefix('"')
        .ok_or_else(|| "expected quoted .incbin path".to_owned())?;
    let close = after_open
        .find('"')
        .ok_or_else(|| "unterminated .incbin path".to_owned())?;
    let filename = &after_open[..close];
    let args = after_open[close + 1..].trim();
    if args.is_empty() {
        return Ok((filename, 0, None));
    }
    let args = args
        .strip_prefix(',')
        .ok_or_else(|| "unexpected text after .incbin path".to_owned())?;
    let mut args = args.split(',').map(str::trim);
    let start = args
        .next()
        .ok_or_else(|| "missing .incbin offset".to_owned())?
        .parse::<usize>()
        .map_err(|_| "invalid .incbin offset".to_owned())?;
    let length = args
        .next()
        .map(|value| {
            value
                .parse::<usize>()
                .map_err(|_| "invalid .incbin length".to_owned())
        })
        .transpose()?;
    if args.next().is_some() {
        return Err("too many .incbin arguments".to_owned());
    }
    Ok((filename, start, length))
}

fn eval_expression(expression: &str, symbols: &HashMap<String, i64>) -> Result<i64, String> {
    struct Parser<'a> {
        expression: &'a str,
        bytes: &'a [u8],
        cursor: usize,
        symbols: &'a HashMap<String, i64>,
    }

    impl Parser<'_> {
        fn spaces(&mut self) {
            while self.cursor < self.bytes.len() && self.bytes[self.cursor].is_ascii_whitespace() {
                self.cursor += 1;
            }
        }

        fn atom(&mut self) -> Result<i64, String> {
            self.spaces();
            if self.bytes.get(self.cursor) == Some(&b'(') {
                self.cursor += 1;
                let value = self.sum(true)?;
                self.spaces();
                if self.bytes.get(self.cursor) != Some(&b')') {
                    return Err(format!(
                        "unclosed group in asset expression: {}",
                        self.expression
                    ));
                }
                self.cursor += 1;
                return Ok(value);
            }
            let start = self.cursor;
            while self.cursor < self.bytes.len()
                && (self.bytes[self.cursor].is_ascii_alphanumeric()
                    || self.bytes[self.cursor] == b'_')
            {
                self.cursor += 1;
            }
            if start == self.cursor {
                return Err(format!(
                    "invalid token in asset expression: {}",
                    self.expression
                ));
            }
            let token = &self.expression[start..self.cursor];
            if token.as_bytes()[0].is_ascii_digit() {
                if let Some(hex) = token
                    .strip_prefix("0x")
                    .or_else(|| token.strip_prefix("0X"))
                {
                    i64::from_str_radix(hex, 16)
                        .map_err(|_| format!("invalid integer in asset expression: {token}"))
                } else {
                    token
                        .parse::<i64>()
                        .map_err(|_| format!("invalid integer in asset expression: {token}"))
                }
            } else {
                self.symbols
                    .get(token)
                    .copied()
                    .ok_or_else(|| format!("unknown asset symbol {token} in {}", self.expression))
            }
        }

        fn sum(&mut self, nested: bool) -> Result<i64, String> {
            self.spaces();
            let mut sign = 1i64;
            if self.bytes.get(self.cursor) == Some(&b'+')
                || self.bytes.get(self.cursor) == Some(&b'-')
            {
                sign = if self.bytes[self.cursor] == b'-' {
                    -1
                } else {
                    1
                };
                self.cursor += 1;
            }
            let mut value = sign
                .checked_mul(self.atom()?)
                .ok_or_else(|| format!("asset expression overflow: {}", self.expression))?;
            loop {
                self.spaces();
                if self.cursor == self.bytes.len() || self.bytes[self.cursor] == b')' {
                    break;
                }
                let sign: i64 = match self.bytes[self.cursor] {
                    b'+' => 1,
                    b'-' => -1,
                    _ => {
                        return Err(format!(
                            "invalid operator in asset expression: {}",
                            self.expression
                        ))
                    }
                };
                self.cursor += 1;
                let term = self.atom()?;
                value =
                    value
                        .checked_add(sign.checked_mul(term).ok_or_else(|| {
                            format!("asset expression overflow: {}", self.expression)
                        })?)
                        .ok_or_else(|| format!("asset expression overflow: {}", self.expression))?;
            }
            if nested && self.bytes.get(self.cursor) != Some(&b')') {
                return Err(format!("incomplete asset expression: {}", self.expression));
            }
            Ok(value)
        }
    }

    let mut parser = Parser {
        expression,
        bytes: expression.as_bytes(),
        cursor: 0,
        symbols,
    };
    let value = parser.sum(false)?;
    parser.spaces();
    if parser.cursor != parser.bytes.len() {
        return Err(format!(
            "unexpected token in asset expression: {expression}"
        ));
    }
    Ok(value)
}

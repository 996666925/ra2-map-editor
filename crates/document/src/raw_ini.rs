//! Byte-oriented physical INI editing. Unchanged lines are copied verbatim.
use anyhow::{Context, Result, ensure};
use std::{collections::BTreeMap, ops::Range, sync::Arc};

#[derive(Clone, Debug)]
pub struct Line {
    pub bytes: Range<usize>,
    pub section: String,
    pub key: Option<String>,
    pub value: Range<usize>,
}
#[derive(Clone)]
pub struct RawIni {
    pub bytes: Arc<[u8]>,
    pub lines: Vec<Line>,
    pub newline: Vec<u8>,
}
fn trim(bytes: &[u8]) -> &[u8] {
    bytes.trim_ascii()
}
impl RawIni {
    pub fn parse(bytes: Arc<[u8]>) -> Result<Self> {
        ensure!(
            !bytes.contains(&0),
            "包装或含 NUL 的地图仅支持原样导出，P2 编辑需普通 INI 地图"
        );
        let mut lines = Vec::new();
        let mut section = String::new();
        let mut start = 0;
        for part in bytes.split_inclusive(|b| *b == b'\n') {
            let end = start + part.len();
            let body = part.trim_ascii_end();
            let text = trim(body);
            if text.first() == Some(&b'[')
                && let Some(close) = text.iter().position(|b| *b == b']')
            {
                section = String::from_utf8(text[1..close].to_vec())
                    .context("非 ASCII section 不可编辑")?;
            }
            let mut key = None;
            let mut value = end..end;
            if !text.starts_with(b";")
                && !text.starts_with(b"[")
                && let Some(eq) = body.iter().position(|b| *b == b'=')
            {
                key = std::str::from_utf8(trim(&body[..eq]))
                    .ok()
                    .map(str::to_owned);
                let after = &body[eq + 1..];
                let leading = after.len() - after.trim_ascii_start().len();
                let no_comment = after
                    .split(|b| *b == b';')
                    .next()
                    .unwrap_or_default()
                    .trim_ascii_end();
                value = start + eq + 1 + leading..start + eq + 1 + no_comment.len().max(leading);
            }
            lines.push(Line {
                bytes: start..end,
                section: section.clone(),
                key,
                value,
            });
            start = end;
        }
        let newline = if bytes.windows(2).any(|w| w == b"\r\n") {
            b"\r\n".to_vec()
        } else {
            b"\n".to_vec()
        };
        Ok(Self {
            bytes,
            lines,
            newline,
        })
    }
    pub fn value(&self, line: usize) -> &[u8] {
        &self.bytes[self.lines[line].value.clone()]
    }
    pub fn entries<'a>(&'a self, section: &'a str) -> impl Iterator<Item = (usize, &'a Line)> + 'a {
        self.lines
            .iter()
            .enumerate()
            .filter(move |(_, line)| line.section == section && line.key.is_some())
    }
    pub fn packed(&self, section: &str) -> Result<Vec<u8>> {
        use vera20k_core::{rules::ini_parser::IniFile, util::base64::base64_decode};
        let ini = IniFile::from_bytes(&self.bytes)?;
        let Some(data) = ini.section(section) else {
            return Ok(vec![]);
        };
        base64_decode(&data.read_packed_text()).map_err(anyhow::Error::msg)
    }
    /// Physical line replacements never touch duplicate neighbours. Section
    /// Pack readers concatenate every key's value, including nonnumeric keys.
    /// Replace all payload entries while retaining comments and other sections.
    pub fn patch(
        &self,
        records: &BTreeMap<usize, Option<Vec<u8>>>,
        packs: &BTreeMap<String, Vec<u8>>,
        append: &BTreeMap<String, Vec<(String, Vec<u8>)>>,
        invalidate: bool,
    ) -> Vec<u8> {
        let mut out = Vec::new();
        let mut emitted = std::collections::HashSet::new();
        let mut extras = BTreeMap::<String, Vec<u8>>::new();
        for (section, rows) in append {
            let extra = extras.entry(section.clone()).or_default();
            for (key, value) in rows {
                extra.extend_from_slice(key.as_bytes());
                extra.push(b'=');
                extra.extend_from_slice(value);
                extra.extend_from_slice(&self.newline);
            }
        }
        for (section, pack) in packs {
            if self.entries(section).next().is_none() {
                extras
                    .entry(section.clone())
                    .or_default()
                    .extend_from_slice(pack);
            }
        }
        let mut last = BTreeMap::new();
        for (index, line) in self.lines.iter().enumerate() {
            last.insert(line.section.clone(), index);
        }
        for (index, line) in self.lines.iter().enumerate() {
            if !(invalidate
                && matches!(line.section.as_str(), "Digest" | "Preview" | "PreviewPack"))
            {
                if let Some(pack) = packs.get(&line.section)
                    && line.key.is_some()
                {
                    if emitted.insert(line.section.clone()) {
                        out.extend_from_slice(pack);
                    }
                } else if let Some(change) = records.get(&index) {
                    if let Some(value) = change {
                        out.extend_from_slice(&self.bytes[line.bytes.start..line.value.start]);
                        out.extend_from_slice(value);
                        out.extend_from_slice(&self.bytes[line.value.end..line.bytes.end]);
                    }
                } else {
                    out.extend_from_slice(&self.bytes[line.bytes.clone()]);
                }
            }
            if last.get(&line.section) == Some(&index)
                && let Some(extra) = extras.remove(&line.section)
            {
                ensure_newline(&mut out, &self.newline);
                out.extend_from_slice(&extra);
            }
        }
        for (section, extra) in extras {
            if extra.is_empty() {
                continue;
            }
            ensure_newline(&mut out, &self.newline);
            out.extend_from_slice(format!("[{section}]").as_bytes());
            out.extend_from_slice(&self.newline);
            out.extend_from_slice(&extra);
        }
        out
    }
}
fn ensure_newline(out: &mut Vec<u8>, newline: &[u8]) {
    if !out.is_empty() && !out.ends_with(b"\n") {
        out.extend_from_slice(newline);
    }
}
pub fn base64(bytes: &[u8]) -> String {
    const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for block in bytes.chunks(3) {
        let n = (u32::from(block[0]) << 16)
            | (u32::from(*block.get(1).unwrap_or(&0)) << 8)
            | u32::from(*block.get(2).unwrap_or(&0));
        out.push(ALPHABET[((n >> 18) & 63) as usize] as char);
        out.push(ALPHABET[((n >> 12) & 63) as usize] as char);
        out.push(if block.len() > 1 {
            ALPHABET[((n >> 6) & 63) as usize] as char
        } else {
            '='
        });
        out.push(if block.len() > 2 {
            ALPHABET[(n & 63) as usize] as char
        } else {
            '='
        });
    }
    out
}
pub fn pack_lines(bytes: &[u8], newline: &[u8]) -> Vec<u8> {
    let text = base64(bytes);
    let mut out = Vec::new();
    for (i, line) in text.as_bytes().chunks(70).enumerate() {
        out.extend_from_slice(format!("{}=", i + 1).as_bytes());
        out.extend_from_slice(line);
        out.extend_from_slice(newline);
    }
    out
}
/// Westwood Format80 chunks: literal packets plus the RLE fill opcode.
/// Frames are bounded to 8192 output bytes so all u16 lengths remain valid.
pub fn lcw_encode(bytes: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    for block in bytes.chunks(8192) {
        let mut encoded = Vec::new();
        let mut index = 0;
        while index < block.len() {
            let run = block[index..]
                .iter()
                .take_while(|b| **b == block[index])
                .count();
            if run >= 4 {
                encoded.push(0xfe);
                encoded.extend_from_slice(&(run as u16).to_le_bytes());
                encoded.push(block[index]);
                index += run;
            } else {
                let start = index;
                index += 1;
                while index < block.len() && index - start < 63 {
                    if block[index..]
                        .iter()
                        .take(4)
                        .filter(|b| **b == block[index])
                        .count()
                        == 4
                    {
                        break;
                    }
                    index += 1;
                }
                encoded.push(0x80 | (index - start) as u8);
                encoded.extend_from_slice(&block[start..index]);
            }
        }
        encoded.push(0x80);
        out.extend_from_slice(&(encoded.len() as u16).to_le_bytes());
        out.extend_from_slice(&(block.len() as u16).to_le_bytes());
        out.extend_from_slice(&encoded);
    }
    out
}

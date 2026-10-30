//! Character conversion for editor display names, never resource or map bytes.
use std::{collections::HashMap, sync::OnceLock};

pub(crate) fn simplified(text: &str) -> String {
    // Generated with Windows LCMapStringEx("zh-CN", LCMAP_SIMPLIFIED_CHINESE)
    // over U+3400..U+9FFF and U+F900..U+FAFF. Store changed character pairs
    // so every editor platform uses the same mapping without an OS dependency.
    static MAPPING: OnceLock<HashMap<char, char>> = OnceLock::new();
    let mapping = MAPPING.get_or_init(|| {
        include_str!("simplified_chinese.txt")
            .lines()
            .map(|line| {
                let mut chars = line.chars();
                (chars.next().unwrap(), chars.next().unwrap())
            })
            .collect()
    });
    text.chars()
        .map(|ch| mapping.get(&ch).copied().unwrap_or(ch))
        .collect()
}

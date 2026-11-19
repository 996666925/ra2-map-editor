//! Ordered resource sources. Earlier entries win; game archives stay engine-owned.
use crate::map_resources::MapResources;
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PathBase {
    #[default]
    Map,
    Game,
    Editor,
}
#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct ResourcePath {
    pub path: PathBuf,
    pub base: PathBase,
}
impl ResourcePath {
    pub fn map(path: impl Into<PathBuf>) -> Self {
        Self {
            path: path.into(),
            base: PathBase::Map,
        }
    }
    fn resolve(&self, game: &Path) -> Result<PathBuf> {
        let root = match self.base {
            PathBase::Game => game.to_owned(),
            PathBase::Editor => editor_directory()?,
            PathBase::Map => PathBuf::from("."),
        };
        ensure!(!self.path.as_os_str().is_empty(), "素材路径为空");
        root.join(&self.path)
            .canonicalize()
            .with_context(|| format!("找不到素材：{}", root.join(&self.path).display()))
    }
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AresMode {
    #[default]
    Auto,
    On,
    Off,
}
impl AresMode {
    pub fn setting(self) -> vera20k_core::util::ares_launch::AresSetting {
        use vera20k_core::util::ares_launch::AresSetting;
        match self {
            Self::Auto => AresSetting::Auto,
            Self::On => AresSetting::On,
            Self::Off => AresSetting::Off,
        }
    }
}
pub fn editor_directory() -> Result<PathBuf> {
    Ok(std::env::current_exe()?
        .parent()
        .context("编辑器目录不存在")?
        .to_owned())
}
pub const INI_ENTRIES: &[(&str, &str)] = &[
    ("rules", "rulesmd.ini"),
    ("art", "artmd.ini"),
    ("ai", "aimd.ini"),
    ("sound", "soundmd.ini"),
    ("eva", "evamd.ini"),
    ("theme", "thememd.ini"),
    ("temperate", "temperatmd.ini"),
    ("snow", "snowmd.ini"),
    ("urban", "urbanmd.ini"),
    ("newurban", "urbannmd.ini"),
    ("desert", "desertmd.ini"),
    ("lunar", "lunarmd.ini"),
];
#[derive(Clone, Debug)]
pub struct ResolvedPaths {
    pub high: Vec<PathBuf>,
    pub extra: Vec<PathBuf>,
    pub mixes: Vec<PathBuf>,
    pub low: Vec<PathBuf>,
}
impl ResolvedPaths {
    pub fn needs_view(&self) -> bool {
        !self.high.is_empty()
            || !self.extra.is_empty()
            || !self.mixes.is_empty()
            || !self.low.is_empty()
    }
    pub fn directories(&self) -> impl Iterator<Item = (&str, &PathBuf)> {
        self.high
            .iter()
            .map(|p| ("high", p))
            .chain(self.extra.iter().map(|p| ("extra", p)))
            .chain(self.low.iter().map(|p| ("low", p)))
    }
}
impl MapResources {
    pub fn is_empty(&self) -> bool {
        self == &Self::default()
    }
    pub(crate) fn validate(&self) -> Result<()> {
        ensure!(
            self.high_priority.len()
                + self.directories.len()
                + self.archives.len()
                + self.low_priority.len()
                <= 128,
            "资源来源最多 128 个"
        );
        let mut names = std::collections::HashSet::new();
        for (key, value) in &self.ini {
            ensure!(
                names.insert(key.to_ascii_lowercase()),
                "INI 入口重复：{key}"
            );
            ensure!(
                INI_ENTRIES
                    .iter()
                    .any(|(name, _)| name.eq_ignore_ascii_case(key)),
                "未知 INI 入口：{key}"
            );
            ini_name(value)?;
        }
        Ok(())
    }
    pub(crate) fn resolve_lists(&mut self, map: &Path) -> Result<()> {
        self.validate()?;
        let parent = map.parent().unwrap_or_else(|| Path::new("."));
        for item in self
            .high_priority
            .iter_mut()
            .chain(&mut self.directories)
            .chain(&mut self.archives)
            .chain(&mut self.low_priority)
        {
            if item.base == PathBase::Map {
                ensure!(!item.path.as_os_str().is_empty(), "素材路径为空");
                item.path = parent
                    .join(&item.path)
                    .canonicalize()
                    .with_context(|| format!("找不到地图素材：{}", item.path.display()))?;
            }
        }
        Ok(())
    }
    pub(crate) fn relativize_lists(&mut self, map: &Path) -> Result<()> {
        let parent = map
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."))
            .canonicalize()?;
        for item in self
            .high_priority
            .iter_mut()
            .chain(&mut self.directories)
            .chain(&mut self.archives)
            .chain(&mut self.low_priority)
        {
            if item.base == PathBase::Map
                && let Some(relative) = relative_to(&item.path, &parent)
            {
                item.path = if relative.as_os_str().is_empty() {
                    ".".into()
                } else {
                    relative
                };
            }
        }
        Ok(())
    }
    pub fn paths(&self, game: &Path) -> Result<ResolvedPaths> {
        self.validate()?;
        let dirs = |items: &[ResourcePath]| -> Result<Vec<PathBuf>> {
            items
                .iter()
                .map(|item| {
                    let path = item.resolve(game)?;
                    ensure!(path.is_dir(), "素材路径必须是目录：{}", path.display());
                    Ok(path)
                })
                .collect()
        };
        let mut high = dirs(&self.high_priority)?;
        let extra = dirs(&self.directories)?;
        let mixes = self
            .archives
            .iter()
            .map(|item| {
                let p = item.resolve(game)?;
                ensure!(
                    p.is_file() && p.extension().is_some_and(|e| e.eq_ignore_ascii_case("mix")),
                    "额外归档必须是 MIX 文件：{}",
                    p.display()
                );
                Ok(p)
            })
            .collect::<Result<_>>()?;
        let mut low = dirs(&self.low_priority)?;
        let editor = editor_directory()?;
        let automatic_high = editor.join("resources/overrides");
        if automatic_high.is_dir() {
            high.insert(0, automatic_high.canonicalize()?);
        }
        let automatic_low = editor.join("resources/fallbacks");
        if automatic_low.is_dir() {
            low.push(automatic_low.canonicalize()?);
        }
        Ok(ResolvedPaths {
            high,
            extra,
            mixes,
            low,
        })
    }
}

pub(crate) fn relative_to(path: &Path, parent: &Path) -> Option<PathBuf> {
    let path = path.components().collect::<Vec<_>>();
    let base = parent.components().collect::<Vec<_>>();
    let common = path.iter().zip(&base).take_while(|(a, b)| a == b).count();
    if common == 0 {
        return None;
    }
    let mut relative = PathBuf::new();
    for part in &base[common..] {
        if matches!(part, std::path::Component::Normal(_)) {
            relative.push("..");
        }
    }
    for part in &path[common..] {
        relative.push(part.as_os_str());
    }
    Some(relative)
}
fn ini_name(value: &str) -> Result<()> {
    let path = Path::new(value);
    ensure!(
        !value.contains([char::from(92), '/', ':', char::from(0)])
            && path.file_name() == Some(path.as_os_str())
            && path
                .extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("ini")),
        "INI 入口必须是文件名：{value}"
    );
    Ok(())
}

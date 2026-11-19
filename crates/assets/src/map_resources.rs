use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, File},
    io::{ErrorKind, Read, Write},
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

pub use crate::resource_config::{AresMode, INI_ENTRIES, PathBase, ResolvedPaths, ResourcePath};

#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct MapResources {
    pub directories: Vec<ResourcePath>,
    pub archives: Vec<ResourcePath>,
    pub high_priority: Vec<ResourcePath>,
    pub low_priority: Vec<ResourcePath>,
    pub ini: std::collections::BTreeMap<String, String>,
    pub ares: AresMode,
}

impl MapResources {
    pub fn for_map(&self, map: &Path) -> Result<Self> {
        self.clone().resolved(map)
    }
    pub fn sidecar(map: &Path) -> PathBuf {
        let mut path = map.as_os_str().to_os_string();
        path.push(".assets.json");
        path.into()
    }

    pub fn read(map: &Path) -> Result<Option<Self>> {
        let sidecar = Self::sidecar(map);
        let file = match File::open(&sidecar) {
            Ok(file) => file,
            Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error).with_context(|| sidecar.display().to_string()),
        };
        Self::load(file, map)
            .map(Some)
            .with_context(|| format!("读取地图资源配置：{}", sidecar.display()))
    }

    /// Relative map paths are anchored to the imported configuration file.
    pub fn import(path: &Path) -> Result<Self> {
        Self::load(File::open(path)?, path)
            .with_context(|| format!("导入资源配置：{}", path.display()))
    }

    fn load(file: File, anchor: &Path) -> Result<Self> {
        let mut bytes = Vec::new();
        file.take(65537).read_to_end(&mut bytes)?;
        ensure!(bytes.len() <= 65536, "资源配置文件超过 64 KiB");
        serde_json::from_slice::<Self>(&bytes)?.resolved(anchor)
    }

    fn resolved(mut self, map: &Path) -> Result<Self> {
        self.resolve_lists(map)?;

        Ok(self)
    }

    pub fn write(&self, map: &Path) -> Result<()> {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let mut resources = self.clone().resolved(map)?;
        resources.relativize_lists(map)?;
        let bytes = serde_json::to_vec_pretty(&resources)?;
        ensure!(bytes.len() <= 65536, "资源配置文件超过 64 KiB");
        let sidecar = Self::sidecar(map);
        let temp = sidecar.with_extension(format!(
            "{}-{}.tmp",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let mut file = File::create_new(&temp)?;
        let written = file.write_all(&bytes);
        drop(file);
        let result = written.and_then(|_| fs::rename(&temp, &sidecar));
        if result.is_err() {
            let _ = fs::remove_file(temp);
        }
        result.with_context(|| format!("保存地图资源配置：{}", sidecar.display()))
    }

    pub fn remove(map: &Path) -> Result<()> {
        let sidecar = Self::sidecar(map);
        match fs::remove_file(&sidecar) {
            Err(error) if error.kind() == ErrorKind::NotFound => Ok(()),
            result => result.with_context(|| format!("移除地图资源配置：{}", sidecar.display())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relative_assets_are_scoped_to_the_selected_map() {
        let fixture = crate::test_support::Fixture::new();
        let map = fixture.0.join("mission.yrm");
        let other = fixture.0.join("other.yrm");
        let assets = fixture.0.join("地图素材");
        fs::create_dir(&assets).unwrap();
        fs::write(assets.join("extra.ini"), b"[CUSTOM]\nFoundation=1x1").unwrap();

        let mut config = MapResources {
            directories: vec![ResourcePath::map(assets.canonicalize().unwrap())],
            ini: [("art".into(), "extra.ini".into())].into(),
            ..Default::default()
        };
        config.write(&map).unwrap();
        let stored: MapResources =
            serde_json::from_slice(&fs::read(MapResources::sidecar(&map)).unwrap()).unwrap();
        assert_eq!(stored.directories[0].path, Path::new("地图素材"));
        assert_eq!(MapResources::read(&map).unwrap(), Some(config.clone()));
        assert_eq!(MapResources::read(&other).unwrap(), None);

        config.ini.clear();
        config.write(&map).unwrap();
        assert_eq!(MapResources::read(&map).unwrap(), Some(config));
        MapResources::remove(&map).unwrap();
        assert_eq!(MapResources::read(&map).unwrap(), None);
    }
}

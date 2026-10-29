//! Explicit resource/schema identity. All file IO belongs on resource/IO workers.
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use std::{
    io::Read,
    path::{Path, PathBuf},
};

pub const SCHEMA: &str = "vera20k-pinned-yr-v1";

pub fn hash(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf29ce484222325, |h, b| {
        (h ^ u64::from(*b)).wrapping_mul(0x100000001b3)
    })
}
pub fn file_hash(path: &Path) -> Result<u64> {
    file_hash_controlled(path, &|| false)
}
fn file_hash_controlled(path: &Path, cancelled: &dyn Fn() -> bool) -> Result<u64> {
    let mut file = std::fs::File::open(path)?;
    let mut buffer = [0; 65536];
    let mut h = 0xcbf29ce484222325u64;
    loop {
        ensure!(!cancelled(), "resource scan cancelled");
        let n = file.read(&mut buffer)?;
        if n == 0 {
            return Ok(h);
        }
        for b in &buffer[..n] {
            h = (h ^ u64::from(*b)).wrapping_mul(0x100000001b3);
        }
    }
}
pub fn schema_id() -> String {
    let mut bytes = include_bytes!("../../document/src/mission_schema.rs").to_vec();
    bytes.extend_from_slice(include_bytes!(
        "../../../vendor/vera20k/src/map/map_file.rs"
    ));
    bytes.extend_from_slice(include_bytes!(
        "../../../vendor/vera20k/src/sim/trigger_runtime.rs"
    ));
    format!("{SCHEMA}-{:016x}", hash(&bytes))
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct Revision {
    pub schema: String,
    pub resources: String,
    pub files: usize,
    pub bytes: u64,
}
impl Revision {
    pub fn id(&self) -> String {
        format!("{}:{}", self.schema, self.resources)
    }
    /// Hash effective root resource inputs, including archives. Map outputs,
    /// logs, settings and exe files are deliberately separate from resources.
    pub fn scan(directory: &Path) -> Result<Self> {
        Self::scan_controlled(directory, &|| false)
    }
    pub fn scan_controlled(directory: &Path, cancelled: &dyn Fn() -> bool) -> Result<Self> {
        let mut files = Vec::new();
        for entry in std::fs::read_dir(directory)? {
            let entry = entry?;
            if !entry.file_type()?.is_file() {
                continue;
            }
            let path = entry.path();
            let ext = path
                .extension()
                .unwrap_or_default()
                .to_string_lossy()
                .to_ascii_lowercase();
            let name = entry.file_name().to_string_lossy().to_ascii_lowercase();
            if matches!(
                ext.as_str(),
                "mix"
                    | "ini"
                    | "pal"
                    | "shp"
                    | "tmp"
                    | "tem"
                    | "sno"
                    | "urb"
                    | "ubn"
                    | "des"
                    | "lun"
                    | "vxl"
                    | "hva"
                    | "vpl"
                    | "csf"
                    | "wav"
                    | "aud"
                    | "bik"
                    | "vqa"
                    | "bag"
                    | "idx"
                    | "pcx"
                    | "fnt"
                    | "pkt"
                    | "wma"
                    | "ogg"
            ) && !matches!(
                name.as_str(),
                "spawn.ini" | "spawnmap.ini" | "ra2.ini" | "ra2md.ini" | "ddraw.ini"
            ) {
                files.push((name, path));
            }
        }
        files.sort_by(|a, b| a.0.cmp(&b.0));
        ensure!(
            files.windows(2).all(|w| w[0].0 != w[1].0),
            "资源文件有大小写别名冲突"
        );
        let mut manifest = Vec::new();
        let mut bytes = 0;
        for (name, path) in &files {
            let size = std::fs::metadata(path)?.len();
            bytes += size;
            manifest.extend_from_slice(name.as_bytes());
            manifest.push(0);
            manifest.extend_from_slice(&size.to_le_bytes());
            manifest.extend_from_slice(&file_hash_controlled(path, cancelled)?.to_le_bytes());
        }
        Ok(Self {
            schema: schema_id(),
            resources: format!("{:016x}", hash(&manifest)),
            files: files.len(),
            bytes,
        })
    }
    pub fn check(&self, expected: Option<&str>) -> Result<()> {
        ensure!(
            expected.is_none_or(|id| id == self.id()),
            "资源或 schema 已变化，请先在地图设置中刷新资源并检查地图"
        );
        Ok(())
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Profile {
    pub version: u32,
    pub name: String,
    pub schema: String,
    pub game_dir: PathBuf,
    pub playtest_exe: Option<PathBuf>,
}
impl Profile {
    pub fn read(path: &Path) -> Result<Self> {
        ensure!(std::fs::metadata(path)?.len() <= 65536, "Profile 文件过大");
        let mut p: Self = serde_json::from_slice(&std::fs::read(path)?)?;
        ensure!(
            p.version == 1 && p.schema == SCHEMA,
            "不支持的 profile/schema；不会把未知引擎能力当作 VERA20k"
        );
        ensure!(!p.name.trim().is_empty(), "Profile 名称为空");
        let parent = path.parent().context("Profile 路径无父目录")?;
        if p.game_dir.is_relative() {
            p.game_dir = parent.join(&p.game_dir);
        }
        if let Some(exe) = &mut p.playtest_exe
            && exe.is_relative()
        {
            *exe = parent.join(&*exe);
        }
        ensure!(p.game_dir.is_dir(), "Profile 资源目录不存在");
        Ok(p)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn same_theater_modifications_change_identity_and_unknown_schema_is_rejected() {
        let root = std::env::temp_dir().join(format!("yr-profile-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        let rules = root.join("rulesmd.ini");
        std::fs::write(&rules, b"[General]\nX=1").unwrap();
        let a = Revision::scan(&root).unwrap();
        std::fs::write(&rules, b"[General]\nX=2").unwrap();
        let b = Revision::scan(&root).unwrap();
        assert_ne!(a.id(), b.id());
        assert!(b.check(Some(&a.id())).is_err());
        std::fs::write(root.join("out.map"), b"irrelevant").unwrap();
        assert_eq!(b, Revision::scan(&root).unwrap());
        std::fs::write(
            root.join("profile.json"),
            br#"{"version":1,"name":"unknown","schema":"phobos","game_dir":"."}"#,
        )
        .unwrap();
        assert!(Profile::read(&root.join("profile.json")).is_err());
        std::fs::remove_file(root.join("out.map")).unwrap();
        std::fs::remove_file(root.join("profile.json")).unwrap();
        std::fs::remove_file(rules).unwrap();
        std::fs::remove_dir(root).unwrap();
    }
}

//! Explicit resource/schema identity. All file IO belongs on resource/IO workers.
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    io::Read,
    path::{Path, PathBuf},
    sync::{Mutex, OnceLock},
};

/// Files whose size and filesystem change stamp are unchanged keep their content
/// digest between resource refreshes. Large MIX archives otherwise get read in
/// full on every refresh even when the user only changed a map-local source.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub(super) struct FileStamp {
    pub(super) size: u64,
    modified: u64,
    changed: u64,
    identity: u64,
}

impl FileStamp {
    pub(super) fn read(path: &Path) -> Result<Self> {
        let metadata = std::fs::metadata(path)?;
        #[cfg(windows)]
        {
            use std::os::windows::fs::MetadataExt as _;
            Ok(Self {
                size: metadata.len(),
                modified: metadata.last_write_time(),
                changed: metadata.creation_time(),
                identity: u64::from(metadata.file_attributes()),
            })
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt as _;
            Ok(Self {
                size: metadata.len(),
                modified: (metadata.mtime() as u64).rotate_left(17) ^ metadata.mtime_nsec() as u64,
                // Linking an immutable archive into a resource view changes
                // ctime on Unix without changing the bytes. Keep the stamp
                // stable across that operation and use the inode as identity.
                changed: 0,
                identity: metadata.dev().rotate_left(23) ^ metadata.ino(),
            })
        }
        #[cfg(not(any(windows, unix)))]
        {
            let modified = metadata
                .modified()?
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default();
            let created = metadata
                .created()
                .ok()
                .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
                .unwrap_or_default();
            Ok(Self {
                size: metadata.len(),
                modified: modified.as_nanos() as u64,
                changed: created.as_nanos() as u64,
                identity: 0,
            })
        }
    }
}

#[derive(Clone, Copy)]
struct CachedHash {
    stamp: FileStamp,
    hash: u64,
}

static FILE_HASHES: OnceLock<Mutex<HashMap<PathBuf, CachedHash>>> = OnceLock::new();
const MAX_CACHED_FILES: usize = 100_000;

fn file_hash_cache() -> &'static Mutex<HashMap<PathBuf, CachedHash>> {
    FILE_HASHES.get_or_init(|| Mutex::new(HashMap::new()))
}

pub(super) fn cached_file_hash(path: &Path, stamp: FileStamp) -> Option<u64> {
    file_hash_cache()
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .get(path)
        .filter(|entry| entry.stamp == stamp)
        .map(|entry| entry.hash)
}

fn remember_file_hash(path: &Path, stamp: FileStamp, hash: u64) {
    let mut cache = file_hash_cache()
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    if cache.len() >= MAX_CACHED_FILES && !cache.contains_key(path) {
        cache.clear();
    }
    cache.insert(path.to_owned(), CachedHash { stamp, hash });
}

pub(super) fn remember_staged_file(source: &Path, target: &Path) -> Result<()> {
    let hash = file_hash_controlled(source, &|| false)?;
    remember_file_hash(target, FileStamp::read(target)?, hash);
    Ok(())
}

pub(super) fn remember_linked_staged_file(source: &Path, target: &Path) {
    let mut cache = file_hash_cache()
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    if let Some(entry) = cache.get(source).copied() {
        cache.insert(target.to_owned(), entry);
    }
}

pub(super) fn relocate_staged_hashes(from: &Path, to: &Path) {
    let mut cache = file_hash_cache()
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    let moved = cache
        .keys()
        .filter_map(|path| {
            path.strip_prefix(from)
                .ok()
                .map(|relative| (path.clone(), to.join(relative)))
        })
        .collect::<Vec<_>>();
    for (old, new) in moved {
        if let Some(entry) = cache.remove(&old) {
            cache.insert(new, entry);
        }
    }
}

pub const SCHEMA: &str = "vera20k-pinned-yr-v1";

pub fn hash(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf29ce484222325, |h, b| {
        (h ^ u64::from(*b)).wrapping_mul(0x100000001b3)
    })
}
pub fn file_hash(path: &Path) -> Result<u64> {
    file_hash_controlled(path, &|| false)
}
pub(crate) fn file_hash_controlled(path: &Path, cancelled: &dyn Fn() -> bool) -> Result<u64> {
    file_hash_with_stamp_controlled(path, cancelled).map(|(_, hash)| hash)
}

pub(super) fn file_hash_with_stamp_controlled(
    path: &Path,
    cancelled: &dyn Fn() -> bool,
) -> Result<(FileStamp, u64)> {
    let stamp = FileStamp::read(path)?;
    if let Some(hash) = cached_file_hash(path, stamp) {
        return Ok((stamp, hash));
    }
    let mut file = std::fs::File::open(path)?;
    ensure!(
        FileStamp::read(path)? == stamp,
        "素材在读取时发生变化：{}",
        path.display()
    );
    let mut buffer = [0; 65536];
    let mut h = 0xcbf29ce484222325u64;
    loop {
        ensure!(!cancelled(), "resource scan cancelled");
        let n = file.read(&mut buffer)?;
        if n == 0 {
            if FileStamp::read(path)? == stamp {
                remember_file_hash(path, stamp, h);
            }
            return Ok((stamp, h));
        }
        for b in &buffer[..n] {
            h = (h ^ u64::from(*b)).wrapping_mul(0x100000001b3);
        }
    }
}

#[cfg(test)]
pub(super) fn file_hash_with_cache_status(path: &Path) -> Result<(u64, bool)> {
    let stamp = FileStamp::read(path)?;
    if let Some(hash) = cached_file_hash(path, stamp) {
        return Ok((hash, true));
    }
    Ok((file_hash_controlled(path, &|| false)?, false))
}
pub fn schema_id() -> String {
    let mut bytes = include_bytes!("../../document/src/mission_schema.rs").to_vec();
    bytes.extend_from_slice(include_bytes!("../../document/src/mission_ares.rs"));
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
        let mut files = std::collections::BTreeMap::new();
        crate::resource_view::collect_tree(
            directory,
            Path::new(""),
            &mut files,
            true,
            &mut Vec::new(),
            cancelled,
        )?;
        let mut manifest = Vec::new();
        let mut bytes = 0;
        for (name, path) in &files {
            let (stamp, digest) = file_hash_with_stamp_controlled(path, cancelled)?;
            bytes += stamp.size;
            manifest.extend_from_slice(name.as_bytes());
            manifest.push(0);
            manifest.extend_from_slice(&stamp.size.to_le_bytes());
            manifest.extend_from_slice(&digest.to_le_bytes());
        }
        Ok(Self {
            schema: schema_id(),
            resources: format!("{:016x}", hash(&manifest)),
            files: files.len(),
            bytes,
        })
    }
    pub fn scan_with_resources(
        directory: &Path,
        resources: Option<&crate::map_resources::MapResources>,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<Self> {
        let revision = Self::scan_controlled(directory, cancelled)?;
        Self::scan_resources_from_base(directory, resources, revision, cancelled)
    }

    /// Reuse a verified installation revision when only the map resource
    /// configuration changed. The base installation is still represented by
    /// `revision`; only newly selected directories and archives are scanned.
    pub fn scan_with_resources_from_base(
        directory: &Path,
        resources: Option<&crate::map_resources::MapResources>,
        revision: Self,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<Self> {
        Self::scan_resources_from_base(directory, resources, revision, cancelled)
    }

    fn scan_resources_from_base(
        directory: &Path,
        resources: Option<&crate::map_resources::MapResources>,
        mut revision: Self,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<Self> {
        let default = crate::map_resources::MapResources::default();
        let resources = resources.unwrap_or(&default);
        let paths = resources.paths(directory)?;
        if paths.needs_view()
            || !resources.ini.is_empty()
            || resources.ares != crate::map_resources::AresMode::Auto
        {
            let mut manifest = format!("resources-v1:{}:", revision.resources).into_bytes();
            manifest.extend(serde_json::to_vec(&(&resources.ini, resources.ares))?);
            for (role, path) in paths.directories() {
                let extra = Self::scan_controlled(path, cancelled)?;
                manifest.extend_from_slice(format!("{role}:{};", extra.resources).as_bytes());
                revision.files += extra.files;
                revision.bytes += extra.bytes;
            }
            for path in &paths.mixes {
                manifest.extend_from_slice(
                    format!("mix:{:016x};", file_hash_controlled(path, cancelled)?).as_bytes(),
                );
                revision.files += 1;
                revision.bytes += std::fs::metadata(path)?.len();
            }
            revision.resources = format!("{:016x}", hash(&manifest));
        }
        Ok(revision)
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
            p.version == 1 && matches!(p.schema.as_str(), SCHEMA | "vera20k-pinned-ares-v1"),
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
    fn unchanged_content_hashes_are_reused_and_same_size_edits_invalidate_them() {
        let root = std::env::temp_dir().join(format!(
            "yr-hash-cache-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let path = root.join("large.mix");
        std::fs::write(&path, vec![0x31; 1024 * 1024]).unwrap();
        let (first, hit) = file_hash_with_cache_status(&path).unwrap();
        assert!(!hit);
        assert_eq!(file_hash_with_cache_status(&path).unwrap(), (first, true));

        std::fs::write(&path, vec![0x72; 1024 * 1024]).unwrap();
        let file = std::fs::OpenOptions::new().write(true).open(&path).unwrap();
        file.set_times(
            std::fs::FileTimes::new()
                .set_modified(std::time::SystemTime::now() + std::time::Duration::from_secs(3)),
        )
        .unwrap();
        let (changed, hit) = file_hash_with_cache_status(&path).unwrap();
        assert!(!hit);
        assert_ne!(first, changed);
        std::fs::remove_dir_all(root).unwrap();
    }

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

    #[test]
    fn hard_link_staging_keeps_large_source_hashes_warm() {
        let root = std::env::temp_dir().join(format!(
            "yr-hash-link-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let source = root.join("source.mix");
        let target = root.join("target.mix");
        std::fs::write(&source, vec![0x5a; 1024 * 1024]).unwrap();
        file_hash_controlled(&source, &|| false).unwrap();
        std::fs::hard_link(&source, &target).unwrap();
        assert!(file_hash_with_cache_status(&source).unwrap().1);
        remember_linked_staged_file(&source, &target);
        assert!(file_hash_with_cache_status(&target).unwrap().1);
        std::fs::remove_dir_all(root).unwrap();
    }
}

//! Disposable editor-owned resource views. Check every payload's file stamp and
//! rehash it when it changed or its digest is not warm in this process.
use super::*;
use serde::{Deserialize, Serialize};
use std::sync::{
    Mutex,
    atomic::{AtomicU64, Ordering},
};

#[derive(Serialize, Deserialize)]
struct Entry {
    path: PathBuf,
    size: u64,
    hash: u64,
    stamp: crate::profile::FileStamp,
}
static BUILD: Mutex<()> = Mutex::new(());
static NEXT: AtomicU64 = AtomicU64::new(0);
pub(super) fn materialize(
    view: &ResourceView,
    theater: &str,
    cancelled: &dyn Fn() -> bool,
) -> Result<PathBuf> {
    let _lock = BUILD.lock().unwrap_or_else(|e| e.into_inner());
    ensure!(!cancelled(), "resource load cancelled");
    let cache = cache_directory(view);
    if fs::create_dir_all(&cache).is_err() {
        let fallback = std::env::temp_dir().join("ra2-map-editor-resource-views-v6");
        fs::create_dir_all(&fallback)?;
        return materialize_in(view, theater, cancelled, fallback);
    }
    materialize_in(view, theater, cancelled, cache)
}

fn materialize_in(
    view: &ResourceView,
    theater: &str,
    cancelled: &dyn Fn() -> bool,
    cache: PathBuf,
) -> Result<PathBuf> {
    let identity = format!(
        "editor-view-v6:{}:{:?}:{:?}:{:?}:{theater}",
        view.revision.id(),
        view.directory,
        view.paths,
        view.ares
    );
    let root = cache.join(format!(
        "{:016x}",
        crate::profile::hash(identity.as_bytes())
    ));
    if valid(&root, cancelled)? {
        return Ok(root);
    }
    let tag = format!(
        "{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    );
    let staging = cache.join(format!(".building-{tag}"));
    fs::create_dir(&staging)?;
    let result = (|| -> Result<PathBuf> {
        view.build(&staging, theater, cancelled)?;
        ensure!(
            Revision::scan_with_resources_from_base(
                &view.directory,
                Some(&view.config),
                view.base_revision.clone(),
                cancelled,
            )? == view.revision,
            "装配期间资源已变化，请刷新后重试"
        );
        fs::write(
            staging.join(".complete"),
            serde_json::to_vec(&manifest(&staging, cancelled)?)?,
        )?;
        if root.exists() {
            if valid(&root, cancelled)? {
                fs::remove_dir_all(&staging)?;
                return Ok(root.clone());
            }
            let retired = cache.join(format!(".retired-{tag}"));
            match fs::rename(&root, &retired) {
                Ok(()) => {
                    let _ = fs::remove_dir_all(retired);
                }
                // Another live engine may retain an invalidated view on Windows.
                // Publish this complete independent view rather than deleting its files.
                Err(_) => {
                    let ready = cache.join(format!(".ready-{tag}"));
                    fs::rename(&staging, &ready)?;
                    crate::profile::relocate_staged_hashes(&staging, &ready);
                    return Ok(ready);
                }
            }
        }
        match fs::rename(&staging, &root) {
            Ok(()) => {
                crate::profile::relocate_staged_hashes(&staging, &root);
                Ok(root.clone())
            }
            Err(_) if valid(&root, cancelled)? => {
                fs::remove_dir_all(&staging)?;
                Ok(root.clone())
            }
            Err(e) => Err(e.into()),
        }
    })();
    if result.is_err() {
        let _ = fs::remove_dir_all(&staging);
    }
    if let Ok(current) = &result {
        prune(&cache, current);
    }
    result
}

fn cache_directory(view: &ResourceView) -> PathBuf {
    let excluded = view
        .paths
        .directories()
        .map(|(_, path)| path.as_path())
        .chain(std::iter::once(view.directory.as_path()))
        .collect::<Vec<_>>();
    let sources = view
        .paths
        .directories()
        .map(|(_, path)| path.as_path())
        .chain(view.paths.mixes.iter().map(PathBuf::as_path));
    for source in sources {
        let mut parent = source.parent();
        while let Some(path) = parent {
            let candidate = path.join(".ra2-map-editor-resource-views-v6");
            if excluded.iter().all(|root| !candidate.starts_with(root)) {
                return candidate;
            }
            parent = path.parent();
        }
    }
    std::env::temp_dir().join("ra2-map-editor-resource-views-v6")
}
fn manifest(root: &Path, cancelled: &dyn Fn() -> bool) -> Result<Vec<Entry>> {
    let mut files = BTreeMap::new();
    collect_tree(
        root,
        Path::new(""),
        &mut files,
        true,
        &mut Vec::new(),
        cancelled,
    )?;
    files
        .into_iter()
        .map(|(name, path)| {
            ensure!(!cancelled(), "resource load cancelled");
            let (stamp, hash) = crate::profile::file_hash_with_stamp_controlled(&path, cancelled)?;
            Ok(Entry {
                path: name.into(),
                size: stamp.size,
                hash,
                stamp,
            })
        })
        .collect()
}
fn valid(root: &Path, cancelled: &dyn Fn() -> bool) -> Result<bool> {
    let check = || -> Result<bool> {
        let records: Vec<Entry> = serde_json::from_slice(&fs::read(root.join(".complete"))?)?;
        let mut files = BTreeMap::new();
        collect_tree(
            root,
            Path::new(""),
            &mut files,
            true,
            &mut Vec::new(),
            cancelled,
        )?;
        if files.len() != records.len() {
            return Ok(false);
        }
        for record in records {
            ensure!(!cancelled(), "resource load cancelled");
            if record.path.is_absolute()
                || record
                    .path
                    .components()
                    .any(|c| !matches!(c, std::path::Component::Normal(_)))
            {
                return Ok(false);
            }
            let Some(path) = files.get(&record.path.to_string_lossy().to_ascii_lowercase()) else {
                return Ok(false);
            };
            ensure!(!cancelled(), "resource load cancelled");
            let stamp = crate::profile::FileStamp::read(path)?;
            if stamp.size != record.size || stamp != record.stamp {
                return Ok(false);
            }
            let hash = crate::profile::cached_file_hash(path, stamp)
                .map(Ok)
                .unwrap_or_else(|| crate::profile::file_hash_controlled(path, cancelled));
            if hash? != record.hash {
                return Ok(false);
            }
        }
        Ok(true)
    };
    match check() {
        Ok(value) => Ok(value),
        Err(_) if cancelled() => anyhow::bail!("resource load cancelled"),
        Err(_) => Ok(false),
    }
}
fn prune(cache: &Path, current: &Path) {
    let Ok(entries) = fs::read_dir(cache) else {
        return;
    };
    let mut old = entries
        .filter_map(Result::ok)
        .filter_map(|e| {
            let metadata = e.metadata().ok()?;
            if !metadata.is_dir()
                || e.path() == current
                || e.file_name().to_string_lossy().starts_with(".building")
            {
                return None;
            }
            Some((metadata.modified().ok()?, e.path()))
        })
        .collect::<Vec<_>>();
    old.sort_by_key(|entry| std::cmp::Reverse(entry.0));
    for (_, path) in old.into_iter().skip(24) {
        let Ok(file) = fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(path.join(".lease"))
        else {
            continue;
        };
        if file.try_lock().is_ok() {
            // Keep the lock through removal so other processes cannot retire a live view.
            let _ = fs::remove_dir_all(path);
        }
    }
}

//! Disk and process work stays off the UI thread. Authoring snapshots are immutable.
use anyhow::{Context, Result, ensure};
use ra2_map_assets::map_resources::MapResources;
use ra2_map_document::document::{Baseline, Snapshot};

use serde::{Deserialize, Serialize};
use std::{
    fs::{self, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::{Arc, mpsc},
    time::{SystemTime, UNIX_EPOCH},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Stamp {
    len: u64,
    hash: u64,
}
impl Stamp {
    pub fn bytes(bytes: &[u8]) -> Self {
        let hash = bytes.iter().fold(0xcbf29ce484222325u64, |h, b| {
            (h ^ u64::from(*b)).wrapping_mul(0x100000001b3)
        });
        Self {
            len: bytes.len() as u64,
            hash,
        }
    }
    pub fn file(path: &Path) -> Result<Option<Self>> {
        match fs::read(path) {
            Ok(bytes) => Ok(Some(Self::bytes(&bytes))),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e.into()),
        }
    }
}
#[derive(Clone)]
pub enum Expected {
    Exact(Option<Stamp>),
    Capture,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct RecoveryMeta {
    pub version: u32,
    pub name: String,
    pub directory: PathBuf,
    pub target: Option<PathBuf>,
    pub expected: Option<Stamp>,
    #[serde(default)]
    pub resource_profile: Option<String>,
    #[serde(default)]
    pub reads_ares: Option<bool>,
}
pub struct RecoveryEntry {
    pub path: PathBuf,
    pub meta: RecoveryMeta,
}
pub enum Job {
    Save {
        directory: Option<PathBuf>,
        source: Option<PathBuf>,
        snapshot: Snapshot,
        path: PathBuf,
        expected: Expected,
    },
    Recover {
        snapshot: Snapshot,
        path: PathBuf,
        meta: RecoveryMeta,
    },
    Restore(PathBuf),
    Discard(PathBuf),
    ExportPreview {
        bytes: Arc<[u8]>,
        path: PathBuf,
    },
    ExportHdPreview {
        snapshot: Snapshot,
        directory: PathBuf,
        source: Option<PathBuf>,
        exe: PathBuf,
        path: PathBuf,
    },
    Export {
        snapshot: Snapshot,
        directory: PathBuf,
        source: Option<PathBuf>,
        path: PathBuf,
        max_edge: u32,
        options: ra2_map_scene::visual::ViewOptions,
    },
    Play {
        options: crate::playtest::PlayOptions,
        snapshot: Snapshot,
        exe: PathBuf,
        directory: PathBuf,
        source: Option<PathBuf>,
        root: PathBuf,
    },
}
pub struct PlayProcess {
    pub child: Child,
    pub folder: PathBuf,
}
pub struct Saved {
    pub stamp: Stamp,
    pub warning: Option<String>,
}
pub enum Done {
    Export {
        path: PathBuf,
        result: Result<[u32; 2]>,
    },
    Scan(Result<Vec<RecoveryEntry>>),
    Save {
        document: u64,
        state: u64,
        path: PathBuf,
        result: Result<Saved>,
    },
    Recover {
        document: u64,
        state: u64,
        path: PathBuf,
        result: Result<()>,
    },
    Restore {
        path: PathBuf,
        result: Result<(Arc<Baseline>, RecoveryMeta)>,
    },
    Play(Result<PlayProcess>),
    Discard(Result<()>),
}
pub struct EditorIo {
    sender: mpsc::Sender<Job>,
    pub results: mpsc::Receiver<Done>,
    pub root: PathBuf,
}
impl EditorIo {
    pub fn new(ctx: egui::Context, root: PathBuf) -> Result<Self> {
        let (sender, requests) = mpsc::channel();
        let (tx, results) = mpsc::channel();
        let work_root = root.clone();
        std::thread::Builder::new()
            .name("editor-io".into())
            .spawn(move || {
                let _ = tx.send(Done::Scan(scan(&work_root)));
                ctx.request_repaint();
                let mut publisher = crate::publication::Publisher::default();
                while let Ok(job) = requests.recv() {
                    let done = match job {
                        Job::Save {
                            directory,
                            source,
                            snapshot,
                            path,
                            expected,
                        } => Done::Save {
                            document: snapshot.document_id,
                            state: snapshot.state_id,
                            result: (|| {
                                let _dialect = vera20k_core::rules::ares_switch::read_ares_way(
                                    snapshot.base.reads_ares,
                                );
                                let expected = match expected {
                                    Expected::Exact(v) => v,
                                    Expected::Capture => Stamp::file(&path)?,
                                };
                                let bytes = snapshot.bytes()?;
                                let resources = source
                                    .as_deref()
                                    .map(MapResources::read)
                                    .transpose()?
                                    .flatten();
                                let save_as = source.as_deref() != Some(path.as_path());
                                if save_as && MapResources::sidecar(&path).is_file() {
                                    ensure!(
                                        MapResources::read(&path)? == resources,
                                        "目标地图已有不同的素材配置：{}",
                                        path.display()
                                    );
                                }
                                let revision = directory
                                    .as_ref()
                                    .map(|d| {
                                        publisher.check_profile_with_resources(
                                            d,
                                            resources.as_ref(),
                                            snapshot.resource_profile.as_deref(),
                                        )
                                    })
                                    .transpose()?;
                                let bytes = if let Some(directory) = directory {
                                    let scene = publisher.scene_with_resources(
                                        bytes,
                                        &directory,
                                        resources.clone(),
                                    )?;
                                    crate::publication::embed_scene_preview(&scene)?
                                } else {
                                    bytes
                                };
                                validate(&bytes)?;
                                let stamp = replace_map_with_resources(
                                    &path,
                                    &bytes,
                                    expected,
                                    resources.as_ref(),
                                    save_as,
                                )?;
                                let provenance_warning = if let Some(revision) = revision {
                                    crate::provenance::write(
                                        &crate::provenance::sidecar(&path),
                                        &bytes,
                                        &revision,
                                        None,
                                    )
                                    .err()
                                    .map(|e| format!("版本记录写入失败：{e:#}"))
                                } else {
                                    None
                                };
                                let warning = [provenance_warning]
                                    .into_iter()
                                    .flatten()
                                    .collect::<Vec<_>>();
                                Ok(Saved {
                                    stamp,
                                    warning: (!warning.is_empty()).then(|| warning.join(" / ")),
                                })
                            })(),
                            path,
                        },
                        Job::Recover {
                            snapshot,
                            path,
                            mut meta,
                        } => {
                            let result = (|| {
                                meta.reads_ares = Some(snapshot.base.reads_ares);
                                let bytes = snapshot.bytes()?;
                                write_recovery(&path, &meta, &bytes)
                            })();
                            Done::Recover {
                                document: snapshot.document_id,
                                state: snapshot.state_id,
                                path,
                                result,
                            }
                        }
                        Job::Restore(path) => Done::Restore {
                            result: (|| {
                                let (meta, bytes) = read_recovery(&path)?;
                                let ares = meta
                                    .reads_ares
                                    .unwrap_or_else(ra2_map_document::source::game_reads_ares);
                                let _dialect =
                                    vera20k_core::rules::ares_switch::read_ares_way(ares);
                                let map = validate(&bytes)?;
                                Ok((
                                    Baseline::parse_with_ares(Arc::from(bytes), &map, ares)?,
                                    meta,
                                ))
                            })(),
                            path,
                        },
                        Job::Discard(path) => Done::Discard(match fs::remove_file(path) {
                            Ok(()) => Ok(()),
                            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
                            Err(e) => Err(e.into()),
                        }),
                        Job::ExportPreview { bytes, path } => Done::Export {
                            result: crate::publication::preview_png(&bytes, &path),
                            path,
                        },
                        Job::ExportHdPreview {
                            snapshot,
                            directory,
                            source,
                            exe,
                            path,
                        } => Done::Export {
                            result: (|| {
                                let resources = SessionResources::read(
                                    &directory,
                                    source.as_deref(),
                                    snapshot.resource_profile.as_deref(),
                                )?;
                                crate::map_picture::export_with_resources(
                                    &snapshot.bytes()?,
                                    &exe,
                                    &resources,
                                    &work_root,
                                    &path,
                                )
                            })(),
                            path,
                        },
                        Job::Export {
                            snapshot,
                            directory,
                            source,
                            path,
                            max_edge,
                            options,
                        } => Done::Export {
                            result: (|| {
                                let resources = source
                                    .as_deref()
                                    .map(MapResources::read)
                                    .transpose()?
                                    .flatten();
                                publisher.check_profile_with_resources(
                                    &directory,
                                    resources.as_ref(),
                                    snapshot.resource_profile.as_deref(),
                                )?;
                                let scene = publisher.scene_with_resources(
                                    snapshot.bytes()?,
                                    &directory,
                                    resources,
                                )?;
                                crate::publication::scene_png(&scene, &path, max_edge, options)
                            })(),
                            path,
                        },
                        Job::Play {
                            options,
                            snapshot,
                            exe,
                            directory,
                            source,
                            root,
                        } => Done::Play((|| {
                            let resources = SessionResources::read(
                                &directory,
                                source.as_deref(),
                                snapshot.resource_profile.as_deref(),
                            )?;
                            let bytes = snapshot.bytes()?;
                            launch_with_resources(&bytes, &exe, &resources, &root, &options)
                        })()),
                    };
                    if tx.send(done).is_err() {
                        break;
                    }
                    ctx.request_repaint();
                }
            })?;
        Ok(Self {
            sender,
            results,
            root,
        })
    }
    pub fn send(&self, job: Job) -> Result<()> {
        self.sender
            .send(job)
            .map_err(|_| anyhow::anyhow!("后台保存线程已退出"))
    }
}
pub fn data_root() -> PathBuf {
    std::env::var_os("LOCALAPPDATA")
        .or_else(|| std::env::var_os("XDG_DATA_HOME"))
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
        .join("ra2-map-editor")
}
pub(crate) fn unique() -> String {
    format!(
        "{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    )
}
pub fn recovery_path(root: &Path, document: u64) -> PathBuf {
    root.join("recovery")
        .join(format!("{}-{document}.yrrec", unique()))
}
pub fn validate(bytes: &[u8]) -> Result<vera20k_core::map::map_file::MapFile> {
    ensure!(bytes.len() <= 32 * 1024 * 1024, "输出地图超过 32 MiB 上限");
    vera20k_core::map::map_file::load_from_bytes(bytes.to_vec()).context("写入前重开校验失败")
}
/// A failed replacement leaves the original and the complete temporary file intact.
pub fn replace_map_with_resources(
    path: &Path,
    bytes: &[u8],
    expected: Option<Stamp>,
    resources: Option<&MapResources>,
    carry: bool,
) -> Result<Stamp> {
    if !carry {
        return replace_map(path, bytes, expected);
    }
    validate(bytes)?;
    ensure!(Stamp::file(path)? == expected, "地图已被外部修改");
    let sidecar = MapResources::sidecar(path);
    let previous = match fs::read(&sidecar) {
        Ok(bytes) => Some(bytes),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => return Err(e.into()),
    };
    if let Some(resources) = resources {
        resources.write(path)?;
    } else {
        MapResources::remove(path)?;
    }
    match replace_map(path, bytes, expected) {
        Ok(stamp) => Ok(stamp),
        Err(error) => {
            let rollback = if let Some(bytes) = previous {
                let temp = sidecar.with_extension(format!("{}.rollback", unique()));
                fs::write(&temp, bytes).and_then(|_| fs::rename(temp, &sidecar))
            } else {
                fs::remove_file(&sidecar).or_else(|e| {
                    if e.kind() == std::io::ErrorKind::NotFound {
                        Ok(())
                    } else {
                        Err(e)
                    }
                })
            };
            rollback.context(format!("地图保存失败且素材配置回滚失败：{error:#}"))?;
            Err(error)
        }
    }
}
pub fn replace_map(path: &Path, bytes: &[u8], expected: Option<Stamp>) -> Result<Stamp> {
    ensure!(
        Stamp::file(path)? == expected,
        "磁盘文件已被外部修改，请重载或另存为：{}",
        path.display()
    );
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let temp = parent.join(format!(".yr-map-{}.tmp", unique()));
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temp)
        .context("无法创建同目录临时文件")?;
    let result = (|| -> Result<Stamp> {
        file.write_all(bytes)?;
        file.sync_all()?;
        drop(file);
        validate(&fs::read(&temp)?)?;
        ensure!(
            Stamp::file(path)? == expected,
            "写入期间磁盘文件发生变化，已保留临时文件"
        );
        if expected.is_some() {
            let old = fs::read(path)?;
            ensure!(
                Some(Stamp::bytes(&old)) == expected,
                "备份前磁盘文件发生变化"
            );
            // Keep the latest three independent, exact original-byte backups.
            let backup = parent.join(format!(
                "{}.yr-editor-{}.bak",
                path.file_name().unwrap_or_default().to_string_lossy(),
                unique()
            ));
            let mut backup_file = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&backup)?;
            backup_file.write_all(&old)?;
            backup_file.sync_all()?;
        }
        ensure!(Stamp::file(path)? == expected, "替换前磁盘文件发生变化");
        fs::rename(&temp, path).context("原子替换失败")?;
        prune_backups(path);
        Ok(Stamp::bytes(bytes))
    })();
    result.with_context(|| {
        format!(
            "保存 {}；失败时可恢复临时文件：{}",
            path.display(),
            temp.display()
        )
    })
}
fn prune_backups(path: &Path) {
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let prefix = format!(
        "{}.yr-editor-",
        path.file_name().unwrap_or_default().to_string_lossy()
    );
    let Ok(entries) = fs::read_dir(parent) else {
        return;
    };
    let mut backups: Vec<_> = entries
        .flatten()
        .filter(|e| {
            let name = e.file_name();
            let name = name.to_string_lossy();
            name.starts_with(&prefix)
                && name.ends_with(".bak")
                && name[prefix.len()..name.len() - 4].split('-').count() == 2
                && name[prefix.len()..name.len() - 4]
                    .bytes()
                    .all(|b| b.is_ascii_digit() || b == b'-')
        })
        .collect();
    backups.sort_by_key(|e| e.metadata().and_then(|m| m.modified()).ok());
    for entry in backups.iter().take(backups.len().saturating_sub(3)) {
        let _ = fs::remove_file(entry.path());
    }
}
pub fn write_recovery(path: &Path, meta: &RecoveryMeta, bytes: &[u8]) -> Result<()> {
    let _dialect = vera20k_core::rules::ares_switch::read_ares_way(
        meta.reads_ares
            .unwrap_or_else(ra2_map_document::source::game_reads_ares),
    );
    validate(bytes)?;
    let header = serde_json::to_vec(meta)?;
    ensure!(header.len() <= 65536, "恢复元数据过大");
    fs::create_dir_all(path.parent().context("恢复文件目录无效")?)?;
    let temp = path.with_extension(format!("{}.tmp", unique()));
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temp)?;
    file.write_all(b"YRREC01\n")?;
    file.write_all(&(header.len() as u32).to_le_bytes())?;
    file.write_all(&header)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    drop(file);
    fs::rename(temp, path).context("替换恢复快照失败")
}
pub fn read_recovery(path: &Path) -> Result<(RecoveryMeta, Vec<u8>)> {
    let file = fs::File::open(path)?;
    ensure!(file.metadata()?.len() <= 33 * 1024 * 1024, "恢复文件过大");
    let mut bytes = Vec::new();
    file.take(33 * 1024 * 1024).read_to_end(&mut bytes)?;
    ensure!(
        bytes.len() >= 12 && &bytes[..8] == b"YRREC01\n",
        "恢复文件头无效"
    );
    let len = u32::from_le_bytes(bytes[8..12].try_into()?) as usize;
    ensure!(len <= 65536 && 12 + len <= bytes.len(), "恢复文件截断");
    let meta: RecoveryMeta = serde_json::from_slice(&bytes[12..12 + len])?;
    ensure!(meta.version == 1, "恢复版本不支持");
    Ok((meta, bytes[12 + len..].to_vec()))
}
fn scan(root: &Path) -> Result<Vec<RecoveryEntry>> {
    let directory = root.join("recovery");
    fs::create_dir_all(&directory)?;
    let mut entries: Vec<_> = fs::read_dir(directory)?
        .flatten()
        .filter(|e| e.path().extension().is_some_and(|e| e == "yrrec"))
        .collect();
    entries.sort_by_key(|e| std::cmp::Reverse(e.metadata().and_then(|m| m.modified()).ok()));
    Ok(entries
        .into_iter()
        .take(32)
        .filter_map(|e| {
            read_recovery(&e.path())
                .ok()
                .map(|(meta, _)| RecoveryEntry {
                    path: e.path(),
                    meta,
                })
        })
        .collect())
}
pub fn launch(
    bytes: &[u8],
    exe: &Path,
    directory: &Path,
    root: &Path,
    options: &crate::playtest::PlayOptions,
) -> Result<PlayProcess> {
    let resources = SessionResources::read(directory, None, None)?;
    launch_with_resources(bytes, exe, &resources, root, options)
}

fn launch_with_resources(
    bytes: &[u8],
    exe: &Path,
    resources: &SessionResources,
    root: &Path,
    options: &crate::playtest::PlayOptions,
) -> Result<PlayProcess> {
    let exe = fs::canonicalize(exe).context("VERA20k 可执行文件不存在")?;
    let folder = root.join("playtest").join(unique());
    fs::create_dir_all(&folder)?;
    let folder = fs::canonicalize(folder)?;
    let game_dir = resources.prepare(bytes, &folder, options)?;
    let spawn = options.spawn_ini_with_ares(&game_dir.map, game_dir.ares)?;
    fs::write(folder.join("spawnmap.ini"), bytes)?;
    fs::write(folder.join("spawn.ini"), spawn)?;
    crate::provenance::write(
        &folder.join("editor-session.json"),
        bytes,
        &resources.revision,
        Some(&exe),
    )?;
    let out = fs::File::create(folder.join("stdout.log"))?;
    let err = fs::File::create(folder.join("stderr.log"))?;
    let mut command = Command::new(exe);
    command
        .arg("-SPAWN")
        .current_dir(&folder)
        .env("RA2_DIR", game_dir.directory)
        .stdin(Stdio::null())
        .stdout(out)
        .stderr(err);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    let child = command.spawn().context("启动 VERA20k 试玩失败")?;
    Ok(PlayProcess { child, folder })
}

pub(crate) struct SessionResources {
    directory: PathBuf,
    map: Option<MapResources>,
    base_revision: ra2_map_assets::profile::Revision,
    revision: ra2_map_assets::profile::Revision,
}
pub(crate) struct PreparedSession {
    pub directory: PathBuf,
    pub ares: vera20k_core::util::ares_launch::AresSetting,
    pub map: vera20k_core::map::map_file::MapFile,
}

impl SessionResources {
    pub(crate) fn read(
        directory: &Path,
        source: Option<&Path>,
        expected: Option<&str>,
    ) -> Result<Self> {
        let directory = fs::canonicalize(directory).context("游戏资源目录不存在")?;
        let map = source.map(MapResources::read).transpose()?.flatten();
        let base_revision =
            ra2_map_assets::profile::Revision::scan_controlled(&directory, &|| false)?;
        let revision = ra2_map_assets::profile::Revision::scan_with_resources_from_base(
            &directory,
            map.as_ref(),
            base_revision.clone(),
            &|| false,
        )?;
        revision.check(expected)?;
        Ok(Self {
            directory,
            map,
            base_revision,
            revision,
        })
    }

    pub(crate) fn prepare(
        &self,
        bytes: &[u8],
        folder: &Path,
        options: &crate::playtest::PlayOptions,
    ) -> Result<PreparedSession> {
        let view = ra2_map_assets::resource_view::ResourceView::with_revisions(
            &self.directory,
            self.map.as_ref(),
            self.base_revision.clone(),
            self.revision.clone(),
            &|| false,
        )?;
        // ResourceView resolves Auto with a mounted Ares archive to On.
        // Another editor tab may have changed the process launch default.
        let reads_ares = view.ares == vera20k_core::util::ares_launch::AresSetting::On;
        let _dialect = vera20k_core::rules::ares_switch::read_ares_way(reads_ares);
        let map = validate(bytes)?;
        crate::wasm_mission::verify_map(&map.ini)?;
        let root = view.materialize(&map.header.theater, &|| false)?;
        let payload = std::collections::BTreeMap::new();
        let directory = prepare_playtest_files(
            folder,
            &self.directory,
            (root != self.directory).then_some(root.as_path()),
            payload,
            options,
        )?;
        let settings = folder.join("settings/VERA.INI");
        let mut text = fs::read_to_string(&settings)?;
        use std::fmt::Write as _;
        writeln!(text, "Ares={}", view.ares.ini_value())?;
        fs::write(settings, text)?;
        let ini = view.ini_config();
        let config = folder.join("config.toml");
        let mut text = fs::read_to_string(&config)?;
        use vera20k_core::util::ini_roots::IniProfile;
        writeln!(text, "[ini]")?;
        writeln!(
            text,
            "profile = {}",
            serde_json::to_string(if ini.profile == IniProfile::Custom {
                "custom"
            } else {
                "yr"
            })?
        )?;
        for (key, value) in [
            ("rules", ini.rules),
            ("art", ini.art),
            ("ai", ini.ai),
            ("sound", ini.sound),
            ("eva", ini.eva),
            ("theater_suffix", ini.theater_suffix),
        ] {
            if let Some(value) = value {
                writeln!(text, "{key} = {}", serde_json::to_string(&value)?)?;
            }
        }
        fs::write(config, text)?;
        Ok(PreparedSession {
            directory,
            ares: view.ares,
            map,
        })
    }
}

pub(crate) fn prepare_playtest_files(
    folder: &Path,
    directory: &Path,
    additional: Option<&Path>,
    resources: std::collections::BTreeMap<String, Vec<u8>>,
    options: &crate::playtest::PlayOptions,
) -> Result<PathBuf> {
    let game_dir = if resources.is_empty() && additional.is_none() {
        directory.to_path_buf()
    } else {
        let assets = folder.join("assets");
        fs::create_dir(&assets)?;
        // AssetManager reads loose files only under ra2_dir, not cwd. Keep the
        // original resources alongside mission overrides in this session's root.
        let mut files = std::collections::BTreeMap::new();
        for root in [Some(directory), additional].into_iter().flatten() {
            collect_session_assets(root, &mut files)?;
        }
        for (name, source) in files {
            if resources.contains_key(&name) {
                continue;
            }
            let target = assets.join(&name);
            if let Some(parent) = target.parent() {
                fs::create_dir_all(parent)?;
            }
            if name.ends_with(".mix") || name.ends_with(".bag") {
                ra2_map_assets::resource_view::stage_file(&source, &target)?;
            } else {
                fs::copy(&source, &target)?;
            }
        }
        for (name, value) in resources {
            fs::write(assets.join(name), value)?;
        }
        assets
    };
    let config =
        vera20k_core::util::config::set_paths_key("", "ra2_dir", &game_dir.to_string_lossy());
    let config = vera20k_core::util::config::set_paths_key(
        &config,
        "settings_dir",
        &folder.join("settings").to_string_lossy(),
    );
    fs::write(folder.join("config.toml"), config)?;
    options.write_settings(&folder.join("settings"))?;
    Ok(game_dir)
}

fn collect_session_assets(
    directory: &Path,
    files: &mut std::collections::BTreeMap<String, PathBuf>,
) -> Result<()> {
    let mut collected = std::collections::BTreeMap::new();
    ra2_map_assets::resource_view::collect_tree(
        directory,
        Path::new(""),
        &mut collected,
        true,
        &mut Vec::new(),
        &|| false,
    )?;
    files.extend(collected);
    Ok(())
}

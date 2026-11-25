//! Editor resource search policy using the unmodified engine API.
//! Each theater gets its own validated view; original INI names remain readable.
use crate::{
    map_resources::{AresMode, MapResources, ResolvedPaths},
    profile::Revision,
};
use anyhow::{Context, Result, ensure};
use std::{
    collections::BTreeMap,
    fs,
    io::{BufWriter, Write},
    path::{Path, PathBuf},
};
use vera20k_core::{
    assets::{
        asset_manager::{AssetManager, MediaArchiveMode},
        mix_archive::MixArchive,
    },
    util::{
        ares_launch::AresSetting,
        ini_roots::{IniProfile, IniRootConfig},
    },
};

#[path = "resource_cache.rs"]
mod cache;

pub struct ResourceView {
    /// Source installation, never a generated cache path.
    pub directory: PathBuf,
    pub ares: AresSetting,
    pub revision: Revision,
    /// Verified installation-only fingerprint, before map-specific sources.
    pub base_revision: Revision,
    config: MapResources,
    paths: ResolvedPaths,
    leases: std::sync::Mutex<BTreeMap<PathBuf, fs::File>>,
}
impl ResourceView {
    pub fn open(
        game: &Path,
        config: Option<&MapResources>,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<Self> {
        let base_revision = Revision::scan_controlled(game, cancelled)?;
        Self::with_base_revision(game, config, base_revision, cancelled)
    }
    pub fn with_revisions(
        game: &Path,
        config: Option<&MapResources>,
        base_revision: Revision,
        revision: Revision,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<Self> {
        ensure!(
            Revision::scan_with_resources_from_base(
                game,
                config,
                base_revision.clone(),
                cancelled,
            )? == revision,
            "资源在准备试玩时发生变化，请刷新资源后重试"
        );
        Self::from_revisions(game, config, revision, base_revision, cancelled)
    }
    pub fn with_base_revision(
        game: &Path,
        config: Option<&MapResources>,
        base_revision: Revision,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<Self> {
        let revision = Revision::scan_with_resources_from_base(
            game,
            config,
            base_revision.clone(),
            cancelled,
        )?;
        Self::from_revisions(game, config, revision, base_revision, cancelled)
    }
    fn from_revisions(
        game: &Path,
        config: Option<&MapResources>,
        revision: Revision,
        base_revision: Revision,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<Self> {
        ensure!(!cancelled(), "resource load cancelled");
        let directory = game.canonicalize()?;
        let config = config.cloned().unwrap_or_default();
        let paths = config.paths(&directory)?;
        let ares = effective_ares(&directory, &config, &paths);
        Ok(Self {
            directory,
            ares,
            revision,
            base_revision,
            config,
            paths,
            leases: Default::default(),
        })
    }
    pub fn manager(&self) -> Result<AssetManager> {
        self.manager_for_theater("TEMPERATE")
    }
    pub fn manager_for_theater(&self, theater: &str) -> Result<AssetManager> {
        self.manager_with_theater(theater, false)
            .map(|(manager, _)| manager)
    }
    pub fn manager_and_theater(
        &self,
        theater: &str,
    ) -> Result<(AssetManager, vera20k_core::map::theater::TheaterData)> {
        self.manager_and_map_theater(theater, false)
    }
    pub(crate) fn manager_and_map_theater(
        &self,
        theater: &str,
        bundled_ice: bool,
    ) -> Result<(AssetManager, vera20k_core::map::theater::TheaterData)> {
        let (manager, loaded) = self.manager_with_theater(theater, bundled_ice)?;
        let data = loaded.with_context(|| format!("无法加载 {theater} 环境资源"))?;
        Ok((manager, data))
    }
    fn manager_with_theater(
        &self,
        theater: &str,
        bundled_ice: bool,
    ) -> Result<(
        AssetManager,
        Option<vera20k_core::map::theater::TheaterData>,
    )> {
        let root = match self.direct_mod_directory()? {
            Some(root) => root,
            None => self.materialize(theater, &|| false)?,
        };
        let roots = self
            .ini_config()
            .resolve(|_| true)
            .map_err(anyhow::Error::msg)?;
        let overlay = (root != self.directory).then_some(root.as_path());
        let mut manager =
            native_manager(&self.directory, overlay, self.ares)?.with_selected_ini_roots(roots);
        let loaded = vera20k_core::map::theater::load_theater_with_bundled_ice(
            &mut manager,
            theater,
            bundled_ice,
            false,
        )?;
        for name in self.config.ini.values() {
            ensure!(manager.get_ref(name).is_some(), "找不到 INI 入口：{name}");
        }
        Ok((manager, loaded))
    }
    /// A single loose-resource directory has exactly the precedence of the
    /// engine's existing mod root. Let the engine read it directly instead of
    /// constructing and validating a file-by-file overlay on every refresh.
    /// Mixed sources, fallback directories and generated INI aliases retain
    /// the verified overlay path below.
    fn direct_mod_directory(&self) -> Result<Option<PathBuf>> {
        if self.paths.high.len() + self.paths.extra.len() != 1
            || !self.paths.low.is_empty()
            || !self.paths.mixes.is_empty()
            || self.config.ini.keys().any(|key| {
                key.eq_ignore_ascii_case("theme")
                    || THEATERS
                        .iter()
                        .any(|(_, theater_key, _)| key.eq_ignore_ascii_case(theater_key))
            })
        {
            return Ok(None);
        }
        let root = self
            .paths
            .high
            .iter()
            .chain(&self.paths.extra)
            .next()
            .context("单一资源目录不存在")?;
        let mut terrain_files = 0;
        for entry in fs::read_dir(root)? {
            let entry = entry?;
            let path = entry.path();
            let kind = entry.file_type()?;
            let (directory, file) = if kind.is_symlink() {
                let metadata = fs::metadata(&path)?;
                (metadata.is_dir(), metadata.is_file())
            } else {
                (kind.is_dir(), kind.is_file())
            };
            if directory {
                return Ok(None);
            }
            if !file {
                continue;
            }
            let name = path
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .to_ascii_lowercase();
            let map_file = path
                .extension()
                .is_some_and(|extension| extension.eq_ignore_ascii_case("yrm"));
            let supported = map_file || resource_file(&path);
            let unsupported_mix = path
                .extension()
                .is_some_and(|extension| extension.eq_ignore_ascii_case("mix"))
                && name != "ares.mix";
            if !supported
                || unsupported_mix
                || matches!(
                    name.as_str(),
                    "spawn.ini"
                        | "spawnmap.ini"
                        | "ra2.ini"
                        | "ra2md.ini"
                        | "ddraw.ini"
                        | "vera.ini"
                )
            {
                return Ok(None);
            }
            if path
                .extension()
                .is_some_and(|extension| extension.eq_ignore_ascii_case("tem"))
            {
                terrain_files += 1;
                if terrain_files > 256 {
                    return Ok(None);
                }
            }
        }
        Ok(Some(root.clone()))
    }
    /// Use the engine's existing root-file policy, not a copy renamed over artmd.ini.
    pub fn ini_config(&self) -> IniRootConfig {
        let mut ini = IniRootConfig::default();
        let chosen = |key: &str, default: &str| {
            self.config
                .ini
                .iter()
                .find(|(k, _)| k.eq_ignore_ascii_case(key))
                .map_or_else(|| default.into(), |(_, v)| v.clone())
        };
        if self.config.ini.keys().any(|key| {
            ["rules", "art", "ai"]
                .iter()
                .any(|name| key.eq_ignore_ascii_case(name))
        }) {
            ini.profile = IniProfile::Custom;
            ini.rules = Some(chosen("rules", "rulesmd.ini"));
            ini.art = Some(chosen("art", "artmd.ini"));
            ini.ai = Some(chosen("ai", "aimd.ini"));
        }
        for (key, target) in [("sound", &mut ini.sound), ("eva", &mut ini.eva)] {
            *target = self
                .config
                .ini
                .iter()
                .find(|(k, _)| k.eq_ignore_ascii_case(key))
                .map(|(_, v)| v.clone());
        }
        if self
            .config
            .ini
            .keys()
            .any(|key| THEATERS.iter().any(|(_, k, _)| key.eq_ignore_ascii_case(k)))
        {
            ini.theater_suffix = Some("editorres".into());
        }
        ini
    }
    pub fn materialize(&self, theater: &str, cancelled: &dyn Fn() -> bool) -> Result<PathBuf> {
        ensure!(
            THEATERS
                .iter()
                .any(|(name, _, _)| name.eq_ignore_ascii_case(theater)),
            "未知地图环境：{theater}"
        );
        if !self.paths.needs_view() && self.config.ini.is_empty() {
            return Ok(self.directory.clone());
        }
        let root = cache::materialize(self, theater, cancelled)?;
        let mut leases = self.leases.lock().unwrap_or_else(|e| e.into_inner());
        if !leases.contains_key(&root) {
            let file = fs::OpenOptions::new()
                .read(true)
                .write(true)
                .create(true)
                .truncate(false)
                .open(root.join(".lease"))?;
            file.lock_shared().context("无法持有资源缓存")?;
            leases.insert(root.clone(), file);
        }
        Ok(root)
    }
    fn build(&self, root: &Path, theater: &str, cancelled: &dyn Fn() -> bool) -> Result<()> {
        // The engine already supports a mod root layered over the real game
        // directory. Materialize only editor-provided overrides there; copying
        // the 1.9 GB base installation made a simple source refresh take tens
        // of seconds and duplicated every stock archive.
        let mut files = BTreeMap::new();
        for dir in self.paths.high.iter().chain(&self.paths.extra) {
            collect_tree(
                dir,
                Path::new(""),
                &mut files,
                false,
                &mut Vec::new(),
                cancelled,
            )?;
        }
        // Thousands of loose TMP terrain files make AssetManager's normal
        // mod-root enumeration needlessly expensive. Keep loose overrides for
        // names that shadow an installed loose file, and put the rest in one
        // generated expansion archive which the engine already mounts.
        let game_loose = fs::read_dir(&self.directory)?
            .filter_map(Result::ok)
            .filter_map(|entry| {
                entry
                    .file_type()
                    .ok()
                    .filter(|kind| kind.is_file())
                    .and_then(|_| entry.file_name().to_str().map(str::to_ascii_lowercase))
            })
            .collect::<std::collections::HashSet<_>>();
        let mut terrain = Vec::new();
        let mut staged = Vec::new();
        for (name, path) in &files {
            let top_level = !name.contains('/');
            if top_level
                && path
                    .extension()
                    .is_some_and(|extension| extension.eq_ignore_ascii_case("tem"))
                && !game_loose.contains(name.rsplit('/').next().unwrap_or(name.as_str()))
            {
                terrain.push((name.clone(), path.clone()));
            } else {
                staged.push((name.clone(), path.clone()));
            }
        }
        if terrain.len() <= 256 {
            staged.append(&mut terrain);
        }
        for (name, path) in &staged {
            ensure!(!cancelled(), "resource load cancelled");
            stage_named(path, &root.join(name))?;
        }
        if !self.paths.mixes.is_empty() {
            let archives = self
                .paths
                .mixes
                .iter()
                .map(|p| {
                    MixArchive::load(p).with_context(|| format!("读取额外 MIX：{}", p.display()))
                })
                .collect::<Result<Vec<_>>>()?;
            overlay_mix(root, &self.directory, "langmd.mix", &archives, cancelled)?;
        }
        if !terrain.is_empty() {
            let terrain_path = root.join(".editor-terrain-stage.mix");
            write_mix_files(&terrain_path, &terrain, cancelled)?;
            let terrain_archive = MixArchive::load(&terrain_path)?;
            fs::remove_file(terrain_path)?;
            let mut archives = vec![terrain_archive];
            for path in &self.paths.mixes {
                archives.push(
                    MixArchive::load(path)
                        .with_context(|| format!("读取额外 MIX：{}", path.display()))?,
                );
            }
            overlay_mix(
                root,
                &self.directory,
                "expandmd99.mix",
                &archives,
                cancelled,
            )?;
        } else if !self.paths.mixes.is_empty() {
            let archives = self
                .paths
                .mixes
                .iter()
                .map(|path| {
                    MixArchive::load(path)
                        .with_context(|| format!("读取额外 MIX：{}", path.display()))
                })
                .collect::<Result<Vec<_>>>()?;
            overlay_mix(
                root,
                &self.directory,
                "expandmd99.mix",
                &archives,
                cancelled,
            )?;
        }
        let mut low = BTreeMap::new();
        for dir in &self.paths.low {
            collect_tree(
                dir,
                Path::new(""),
                &mut low,
                false,
                &mut Vec::new(),
                cancelled,
            )?;
        }
        let needs_ini_products = self.config.ini.keys().any(|key| {
            key.eq_ignore_ascii_case("theme")
                || THEATERS
                    .iter()
                    .any(|(_, theater_key, _)| key.eq_ignore_ascii_case(theater_key))
        });
        let mut manager = if !low.is_empty() || needs_ini_products {
            let mut manager = native_manager(&self.directory, Some(root), self.ares)?;
            let _ = vera20k_core::map::theater::load_theater(&mut manager, theater);
            Some(manager)
        } else {
            None
        };
        // Activation happens before fallback checks. Only live archives suppress a fallback.
        let mut staged_low = false;
        for (name, path) in low {
            if files.contains_key(&name) {
                continue;
            }
            let manager = manager.as_ref().context("回退目录检查需要游戏资源管理器")?;
            let exists = if name.contains('/') {
                manager.read_install_file(&name).is_some()
            } else {
                manager.get_ref(&name).is_some()
            };
            if !exists {
                stage_named(&path, &root.join(name))?;
                staged_low = true;
            }
        }
        if needs_ini_products && staged_low {
            let mut refreshed = native_manager(&self.directory, Some(root), self.ares)?;
            let _ = vera20k_core::map::theater::load_theater(&mut refreshed, theater);
            manager = Some(refreshed);
        }
        // Read every product before writing any aliases, and expand includes using
        // the actual source name. Base rules/art/AI files stay independently addressable.
        let mut products = Vec::new();
        for (key, source) in &self.config.ini {
            ensure!(
                manager
                    .as_ref()
                    .is_some_and(|manager| manager.get_ref(source).is_some())
                    || !needs_ini_products,
                "找不到 INI 入口 {key}={source}"
            );
        }
        if self.ini_config().theater_suffix.is_some() {
            let manager = manager.as_ref().context("地形 INI 需要游戏资源管理器")?;
            for (name, key, control) in THEATERS {
                let selected = self
                    .config
                    .ini
                    .iter()
                    .find(|(k, _)| k.eq_ignore_ascii_case(key));
                let default = format!("{control}md.ini");
                let source = selected.map_or(default.as_str(), |(_, name)| name.as_str());
                if selected.is_some() || manager.get_ref(source).is_some() {
                    let ini = crate::assets::read_ini(manager, source)?;
                    products.push((format!("{control}editorres.ini"), encode_ini(&ini)));
                } else if name.eq_ignore_ascii_case(theater) {
                    anyhow::bail!("缺少当前环境 INI：{source}");
                }
            }
        }
        if let Some((_, source)) = self
            .config
            .ini
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case("theme"))
        {
            let manager = manager.as_ref().context("音乐 INI 需要游戏资源管理器")?;
            products.push((
                "thememd.ini".into(),
                encode_ini(&crate::assets::read_ini(manager, source)?),
            ));
        }
        for (target, bytes) in products {
            fs::write(root.join(target), bytes)?;
        }
        Ok(())
    }
}
const THEATERS: &[(&str, &str, &str)] = &[
    ("TEMPERATE", "temperate", "temperat"),
    ("SNOW", "snow", "snow"),
    ("URBAN", "urban", "urban"),
    ("NEWURBAN", "newurban", "urbann"),
    ("DESERT", "desert", "desert"),
    ("LUNAR", "lunar", "lunar"),
];
fn native_manager(game: &Path, overlay: Option<&Path>, ares: AresSetting) -> Result<AssetManager> {
    AssetManager::with_mod_and_ares(
        game,
        overlay,
        MediaArchiveMode::STOCK_DIGITAL,
        ares,
        &Default::default(),
    )
    .context("无法打开 YR 资源归档")
}
fn effective_ares(game: &Path, config: &MapResources, paths: &ResolvedPaths) -> AresSetting {
    if config.ares != AresMode::Auto {
        return config.ares.setting();
    }
    let selected = !paths.high.is_empty() || !paths.extra.is_empty() || !paths.mixes.is_empty();
    let has = paths
        .high
        .iter()
        .chain(&paths.extra)
        .any(|dir| find_file(dir, "ares.mix").is_some())
        || find_file(game, "ares.mix").is_some()
        || paths.mixes.iter().any(|p| {
            p.file_name()
                .is_some_and(|n| n.eq_ignore_ascii_case("ares.mix"))
        });
    if selected && has {
        AresSetting::On
    } else {
        AresSetting::Auto
    }
}
fn find_file(root: &Path, name: &str) -> Option<PathBuf> {
    fs::read_dir(root)
        .ok()?
        .filter_map(Result::ok)
        .map(|e| e.path())
        .find(|p| p.is_file() && p.file_name().is_some_and(|n| n.eq_ignore_ascii_case(name)))
}
pub fn resource_file(path: &Path) -> bool {
    let name = path
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .to_ascii_lowercase();
    !matches!(
        name.as_str(),
        "spawn.ini" | "spawnmap.ini" | "ra2.ini" | "ra2md.ini" | "ddraw.ini" | "vera.ini"
    ) && matches!(
        path.extension()
            .unwrap_or_default()
            .to_string_lossy()
            .to_ascii_lowercase()
            .as_str(),
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
            | "txt"
    )
}
/// Preserve install-relative subdirectories while detecting symlink loops.
pub fn collect_tree(
    root: &Path,
    relative: &Path,
    files: &mut BTreeMap<String, PathBuf>,
    mix: bool,
    ancestors: &mut Vec<PathBuf>,
    cancelled: &dyn Fn() -> bool,
) -> Result<()> {
    ensure!(!cancelled(), "resource load cancelled");
    let current = root.join(relative);
    let canonical = current.canonicalize()?;
    ensure!(
        !ancestors.contains(&canonical),
        "资源目录存在循环链接：{}",
        current.display()
    );
    ancestors.push(canonical);
    let mut entries = fs::read_dir(&current)?.collect::<std::io::Result<Vec<_>>>()?;
    entries.sort_by_key(|entry| entry.file_name());
    let mut seen = std::collections::HashSet::new();
    for entry in entries {
        let path = entry.path();
        let next = relative.join(entry.file_name());
        let name = next
            .to_string_lossy()
            .replace(char::from(92), "/")
            .to_ascii_lowercase();
        ensure!(
            seen.insert(name.clone()),
            "资源文件有大小写别名冲突：{name}"
        );
        let kind = entry.file_type()?;
        let (directory, file) = if kind.is_symlink() {
            let metadata = fs::metadata(&path)?;
            (metadata.is_dir(), metadata.is_file())
        } else {
            (kind.is_dir(), kind.is_file())
        };
        if directory {
            collect_tree(root, &next, files, mix, ancestors, cancelled)?;
        } else if file && resource_file(&path) {
            if !mix && name.ends_with(".mix") && name != "ares.mix" {
                continue;
            }
            files.entry(name).or_insert(path);
        }
    }
    ancestors.pop();
    Ok(())
}
fn stage_named(source: &Path, target: &Path) -> Result<()> {
    fs::create_dir_all(target.parent().context("资源目标无父目录")?)?;
    match stage_file_with_method(source, target)? {
        StageMethod::HardLink | StageMethod::Symlink => {
            crate::profile::remember_linked_staged_file(source, target);
            Ok(())
        }
        StageMethod::Copy => crate::profile::remember_staged_file(source, target),
    }
}
/// Terrain tiles and archives are read-only inputs and can share their source
/// storage. Other loose assets stay isolated because previews may outlive edits.
pub fn stage_file(source: &Path, target: &Path) -> Result<()> {
    stage_file_with_method(source, target).map(|_| ())
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum StageMethod {
    HardLink,
    Symlink,
    Copy,
}
fn stage_file_with_method(source: &Path, target: &Path) -> Result<StageMethod> {
    if source.extension().is_some_and(|extension| {
        extension.eq_ignore_ascii_case("tem")
            || extension.eq_ignore_ascii_case("mix")
            || extension.eq_ignore_ascii_case("bag")
    }) {
        if fs::hard_link(source, target).is_ok() {
            return Ok(StageMethod::HardLink);
        }
        #[cfg(windows)]
        if std::os::windows::fs::symlink_file(source, target).is_ok() {
            return Ok(StageMethod::Symlink);
        }
        #[cfg(unix)]
        if std::os::unix::fs::symlink(source, target).is_ok() {
            return Ok(StageMethod::Symlink);
        }
    }
    fs::copy(source, target).with_context(|| format!("装配素材 {}", source.display()))?;
    Ok(StageMethod::Copy)
}
fn overlay_mix(
    root: &Path,
    game: &Path,
    name: &str,
    extras: &[MixArchive],
    cancelled: &dyn Fn() -> bool,
) -> Result<()> {
    let target = root.join(name);
    let game_target = game.join(name);
    let original_path = [target.as_path(), game_target.as_path()]
        .into_iter()
        .find(|path| path.is_file());
    let original = original_path.map(MixArchive::load).transpose()?;
    let archives = extras.iter().chain(original.as_ref()).collect::<Vec<_>>();
    let temp = root.join(format!("{name}.tmp"));
    write_mix(&temp, &archives, cancelled)?;
    fs::rename(temp, target)?;
    Ok(())
}
pub fn write_mix(
    path: &Path,
    archives: &[&MixArchive],
    cancelled: &dyn Fn() -> bool,
) -> Result<()> {
    let mut entries = BTreeMap::new();
    for (index, archive) in archives.iter().enumerate() {
        for entry in archive.entries() {
            entries.entry(entry.id).or_insert((index, entry.size));
        }
    }
    ensure!(
        entries.len() <= u16::MAX as usize,
        "额外 MIX 合并超过 65535 个文件"
    );
    let bytes = entries
        .values()
        .map(|(_, size)| u64::from(*size))
        .sum::<u64>();
    ensure!(bytes <= u32::MAX as u64, "额外 MIX 超过 4 GiB");
    let mut file = BufWriter::new(fs::File::create(path)?);
    if entries.is_empty() {
        file.write_all(&[0; 4])?;
    }
    file.write_all(&(entries.len() as u16).to_le_bytes())?;
    file.write_all(&(bytes as u32).to_le_bytes())?;
    let mut offset = 0u32;
    for (id, (_, size)) in &entries {
        file.write_all(&id.to_le_bytes())?;
        file.write_all(&offset.to_le_bytes())?;
        file.write_all(&size.to_le_bytes())?;
        offset += size;
    }
    for (id, (index, _)) in entries {
        ensure!(!cancelled(), "resource load cancelled");
        file.write_all(archives[index].get_by_id(id).context("MIX 条目不可读")?)?;
    }
    file.flush()?;
    Ok(())
}

fn write_mix_files(
    path: &Path,
    files: &[(String, PathBuf)],
    cancelled: &dyn Fn() -> bool,
) -> Result<()> {
    let mut entries = BTreeMap::new();
    for (name, source) in files {
        ensure!(!cancelled(), "resource load cancelled");
        let size = u32::try_from(fs::metadata(source)?.len())
            .with_context(|| format!("MIX 条目过大：{}", source.display()))?;
        entries
            .entry(vera20k_core::assets::mix_hash::mix_hash(name))
            .or_insert((source, size));
    }
    ensure!(
        entries.len() <= u16::MAX as usize,
        "地形素材 MIX 超过 65535 个文件"
    );
    let bytes = entries
        .values()
        .map(|(_, size)| u64::from(*size))
        .sum::<u64>();
    ensure!(bytes <= u32::MAX as u64, "地形素材 MIX 超过 4 GiB");
    let mut file = BufWriter::new(fs::File::create(path)?);
    if entries.is_empty() {
        file.write_all(&[0; 4])?;
    }
    file.write_all(&(entries.len() as u16).to_le_bytes())?;
    file.write_all(&(bytes as u32).to_le_bytes())?;
    let mut offset = 0u32;
    for (id, (_, size)) in &entries {
        file.write_all(&id.to_le_bytes())?;
        file.write_all(&offset.to_le_bytes())?;
        file.write_all(&size.to_le_bytes())?;
        offset += size;
    }
    let mut buffer = [0; 64 * 1024];
    for (source, expected_size) in entries.values() {
        ensure!(!cancelled(), "resource load cancelled");
        let mut source_file = fs::File::open(source)?;
        let mut written = 0u32;
        loop {
            ensure!(!cancelled(), "resource load cancelled");
            let count = std::io::Read::read(&mut source_file, &mut buffer)?;
            if count == 0 {
                break;
            }
            file.write_all(&buffer[..count])?;
            written = written.saturating_add(count as u32);
        }
        ensure!(
            written == *expected_size,
            "素材在装配时发生变化：{}",
            source.display()
        );
    }
    file.flush()?;
    Ok(())
}

pub fn encode_ini(ini: &vera20k_core::rules::ini_parser::IniFile) -> Vec<u8> {
    use std::fmt::Write as _;
    let mut text = String::new();
    let mut seen = std::collections::HashSet::new();
    for name in ini.section_names() {
        if seen.insert(name) {
            let _ = writeln!(text, "[{name}]");
            for (key, value) in ini.section_or_empty(name).raw_entries() {
                let _ = writeln!(text, "{key}={value}");
            }
        }
    }
    vera20k_core::util::native_string::narrow_widened(&text)
}
#[cfg(test)]
#[path = "resource_view_tests.rs"]
mod tests;

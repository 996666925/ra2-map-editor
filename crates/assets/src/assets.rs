//! Read-only P0 adapter. Format parsing and palette semantics belong to core.

use crate::map_resources::MapResources;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, Sender};

use anyhow::{Context, Result, bail, ensure};

use serde::{Deserialize, Serialize};
use vera20k_core::assets::{asset_manager::AssetManager, shp_file::ShpFile, tmp_file::TmpFile};
use vera20k_core::map::theater::{self, TheaterData};

pub fn open_manager(directory: &Path, resources: Option<&MapResources>) -> Result<AssetManager> {
    crate::resource_view::ResourceView::open(directory, resources, &|| false)?.manager()
}

/// Read through the engine's selected INI dialect and include traversal.
pub fn read_ini(
    manager: &AssetManager,
    name: &str,
) -> Result<vera20k_core::rules::ini_parser::IniFile> {
    use vera20k_core::rules::ini_parser::{
        IniFile,
        directives::{DirectiveOptions, process_directives},
    };
    let _dialect = vera20k_core::rules::ares_switch::read_ares_way(manager.ships_ares());
    let bytes = manager
        .get_ref(name)
        .with_context(|| format!("缺少 {name}"))?;
    let ini = IniFile::from_bytes(bytes)?;
    if manager.ships_ares() {
        let expanded = process_directives(
            ini,
            name,
            |file| Ok(manager.get_ref(file).map(<[u8]>::to_vec)),
            DirectiveOptions {
                strict: true,
                ..DirectiveOptions::production()
            },
        )
        .map_err(|error| anyhow::anyhow!("{}", error.message))?;
        Ok(expanded.ini)
    } else {
        Ok(ini)
    }
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AssetSelection {
    pub directory: PathBuf,
    pub tmp_name: String,
    pub sub_tile: usize,
    pub shp_name: String,
    pub frame: usize,
}

impl Default for AssetSelection {
    fn default() -> Self {
        Self {
            directory: PathBuf::new(),
            tmp_name: "clear01.tem".into(),
            sub_tile: 0,
            shp_name: "gi.shp".into(),
            frame: 0,
        }
    }
}

#[derive(Clone, Serialize)]
pub struct AssetInfo {
    pub name: String,
    pub source: String,
    pub palette: String,
    pub palette_source: String,
    pub index: usize,
    pub count: usize,
    pub offset: [i32; 2],
    pub width: u32,
    pub height: u32,
}

#[derive(Clone)]
pub struct DecodedImage {
    /// Original palette indices; retained for exact 16..31 house remap.
    pub indices: Vec<u8>,
    pub info: AssetInfo,
    pub rgba: Vec<u8>,
}

pub struct PreviewAssets {
    pub tile: DecodedImage,
    pub sprite: DecodedImage,
    pub archive_count: usize,
    pub tileset_slots: usize,
}

pub struct Install {
    pub revision: std::sync::Arc<crate::profile::Revision>,
    base_revision: crate::profile::Revision,
    directory: PathBuf,
    resources: Option<MapResources>,
    view: Option<std::sync::Arc<crate::resource_view::ResourceView>>,
    pub manager: AssetManager,
    pub theater: TheaterData,
    pub theater_name: String,
    pub catalog: std::sync::Arc<crate::catalog::Catalog>,
    palette: std::sync::OnceLock<std::sync::Arc<crate::catalog::TerrainPalette>>,
    sprites: std::sync::OnceLock<std::sync::Arc<crate::sprites::SpritePalette>>,
    mission_resources:
        std::sync::OnceLock<std::sync::Arc<crate::mission_resources::MissionResources>>,
    fixed_teams: std::sync::OnceLock<
        Result<std::sync::Arc<vera20k_core::rules::team_ai_ini::TeamAiIniRegistry>, String>,
    >,
}

impl Install {
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn from_probe_root(directory: &Path) -> Result<Self> {
        let mut manager =
            AssetManager::from_ini_probe_root(directory).context("无法打开合成资源目录")?;
        let theater =
            theater::load_theater(&mut manager, "TEMPERATE").context("合成目录缺少温带资源")?;
        let revision = crate::profile::Revision::scan(directory)?;
        Ok(Self {
            revision: std::sync::Arc::new(revision.clone()),
            base_revision: revision,
            directory: directory.to_owned(),
            resources: None,
            view: None,
            manager,
            theater,
            theater_name: "TEMPERATE".into(),
            catalog: Default::default(),
            palette: Default::default(),
            sprites: Default::default(),
            mission_resources: Default::default(),
            fixed_teams: Default::default(),
        })
    }

    pub fn open(directory: &Path) -> Result<Self> {
        Self::open_controlled(directory, &|| false)
    }
    pub fn open_controlled(directory: &Path, cancelled: &dyn Fn() -> bool) -> Result<Self> {
        Self::open_with_resources(directory, None, cancelled)
    }
    pub fn open_with_resources(
        directory: &Path,
        resources: Option<MapResources>,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<Self> {
        Self::open_with_resource_view(directory, resources, None, None, cancelled)
    }

    /// Rebuild a map-specific resource view while reusing the already verified
    /// installation fingerprint. This is used when the map sidecar changes;
    /// it avoids rereading every stock archive before the new view is built.
    pub fn open_with_resources_from_revision(
        directory: &Path,
        resources: Option<MapResources>,
        base_revision: crate::profile::Revision,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<Self> {
        Self::open_with_resource_view(directory, resources, Some(base_revision), None, cancelled)
    }

    fn open_with_resource_view(
        directory: &Path,
        resources: Option<MapResources>,
        base_revision: Option<crate::profile::Revision>,
        reuse: Option<&Self>,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<Self> {
        ensure!(!cancelled(), "resource load cancelled");
        ensure!(
            directory.is_dir(),
            "游戏目录不存在：{}",
            directory.display()
        );
        let view = std::sync::Arc::new(if let Some(base_revision) = base_revision {
            crate::resource_view::ResourceView::with_base_revision(
                directory,
                resources.as_ref(),
                base_revision,
                cancelled,
            )?
        } else {
            crate::resource_view::ResourceView::open(directory, resources.as_ref(), cancelled)?
        });
        let (manager, theater) = view.manager_and_theater("TEMPERATE").map_err(|error| {
            error.context(
                "无法加载 TEMPERATE；请检查 temperatmd.ini、isotem.pal、unittem.pal 和对应 MIX",
            )
        })?;
        ensure!(!cancelled(), "resource load cancelled");
        let reusable =
            reuse.filter(|old| catalog_inputs_equal(&old.manager, &manager, &old.theater_name));
        let catalog = if let Some(old) = reusable {
            old.catalog.clone()
        } else {
            crate::catalog::Catalog::load_with_art(&manager, &theater, None)?
        };
        Ok(Self {
            revision: std::sync::Arc::new(view.revision.clone()),
            base_revision: view.base_revision.clone(),
            directory: directory.to_owned(),
            resources,
            view: Some(view),
            manager,
            theater,
            theater_name: "TEMPERATE".into(),
            catalog,
            palette: Default::default(),
            sprites: Default::default(),
            mission_resources: Default::default(),
            fixed_teams: Default::default(),
        })
    }
    pub fn map_resources(&self) -> Option<&MapResources> {
        self.resources.as_ref()
    }
    pub fn base_revision(&self) -> &crate::profile::Revision {
        &self.base_revision
    }
    pub fn reconfigure_map_resources(
        &self,
        resources: Option<MapResources>,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<Self> {
        let base_revision = crate::profile::Revision::scan_controlled(&self.directory, cancelled)?;
        Self::open_with_resource_view(
            &self.directory,
            resources,
            Some(base_revision),
            Some(self),
            cancelled,
        )
    }
    /// Reconfigure only the map-local source selection. The caller already
    /// owns the current installation revision, so avoid rescanning the game
    /// directory; a full resource refresh remains available above when the
    /// installation itself may have changed.
    pub fn reconfigure_map_resources_reusing_base_revision(
        &self,
        resources: Option<MapResources>,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<Self> {
        Self::open_with_resource_view(
            &self.directory,
            resources,
            Some(self.base_revision.clone()),
            Some(self),
            cancelled,
        )
    }
    pub fn mission_resources(&self) -> std::sync::Arc<crate::mission_resources::MissionResources> {
        self.mission_resources
            .get_or_init(|| std::sync::Arc::new(crate::mission_resources::load(&self.manager)))
            .clone()
    }
    pub fn fixed_team_definitions(
        &self,
    ) -> Result<std::sync::Arc<vera20k_core::rules::team_ai_ini::TeamAiIniRegistry>, String> {
        self.fixed_teams
            .get_or_init(|| {
                vera20k_core::rules::team_ai_ini::TeamAiIniRegistry::load_retail(
                    &self.manager,
                    &vera20k_core::rules::ini_parser::IniFile::empty(),
                    vera20k_core::rules::rule_policy::RulePolicy::stock_campaign(),
                )
                .map(std::sync::Arc::new)
            })
            .clone()
    }
    pub fn terrain_palette(&self) -> std::sync::Arc<crate::catalog::TerrainPalette> {
        self.palette
            .get_or_init(|| {
                std::sync::Arc::new(self.catalog.terrain_palette(&self.manager, &self.theater))
            })
            .clone()
    }
    pub fn activate_theater(&mut self, name: &str) -> Result<()> {
        if self.theater_name.eq_ignore_ascii_case(name) {
            return Ok(());
        }
        let (manager, theater) = if let Some(view) = &self.view {
            view.manager_and_theater(name)?
        } else {
            let mut manager = open_manager(&self.directory, self.resources.as_ref())?;
            let theater = theater::load_theater(&mut manager, name)
                .with_context(|| format!("无法加载 {name} 环境资源"))?;
            (manager, theater)
        };
        self.catalog = crate::catalog::Catalog::load_with_art(&manager, &theater, None)?;
        self.manager = manager;
        self.fixed_teams = Default::default();
        self.mission_resources.take();
        self.theater = theater;
        self.theater_name = name.to_ascii_uppercase();
        self.palette.take();
        self.sprites.take();
        Ok(())
    }
    pub fn sprite_palette(
        &self,
        catalog: &std::sync::Arc<crate::catalog::Catalog>,
    ) -> std::sync::Arc<crate::sprites::SpritePalette> {
        if std::sync::Arc::ptr_eq(catalog, &self.catalog) {
            self.sprites
                .get_or_init(|| {
                    std::sync::Arc::new(crate::sprites::SpritePalette::load(self, catalog))
                })
                .clone()
        } else {
            std::sync::Arc::new(crate::sprites::SpritePalette::load(self, catalog))
        }
    }

    fn decode(&self, selection: &AssetSelection) -> Result<PreviewAssets> {
        let tmp_name = selection.tmp_name.trim();
        let shp_name = selection.shp_name.trim();
        ensure!(
            !tmp_name.is_empty() && !shp_name.is_empty(),
            "请输入 TMP 和 SHP 文件名"
        );
        let (tmp_bytes, tmp_source) = self
            .manager
            .get_with_source_ref(tmp_name)
            .with_context(|| format!("找不到地块：{tmp_name}"))?;
        let tmp =
            TmpFile::from_bytes(tmp_bytes).with_context(|| format!("解析 {tmp_name} 失败"))?;
        let tile = tmp
            .tiles
            .get(selection.sub_tile)
            .and_then(Option::as_ref)
            .with_context(|| {
                format!(
                    "{tmp_name} 子块 {} 不存在或为空（共 {} 个槽位）",
                    selection.sub_tile,
                    tmp.tiles.len()
                )
            })?;
        let tile_rgba = tmp.tile_to_rgba(selection.sub_tile, &self.theater.iso_palette)?;
        let (shp_bytes, shp_source) = self
            .manager
            .get_with_source_ref(shp_name)
            .with_context(|| format!("找不到精灵：{shp_name}"))?;
        let shp =
            ShpFile::from_bytes(shp_bytes).with_context(|| format!("解析 {shp_name} 失败"))?;
        let frame = shp.frames.get(selection.frame).with_context(|| {
            format!(
                "{shp_name} 帧 {} 越界（共 {} 帧）",
                selection.frame,
                shp.frames.len()
            )
        })?;
        let sprite_rgba = shp.frame_to_rgba(selection.frame, &self.theater.unit_palette)?;
        let palette_source = |name: &str| -> Result<String> {
            self.manager
                .resolve_ref(name)
                .map(|r| r.source_archive.to_owned())
                .with_context(|| format!("找不到调色板：{name}"))
        };
        Ok(PreviewAssets {
            tile: DecodedImage {
                indices: Vec::new(),
                info: AssetInfo {
                    name: tmp_name.into(),
                    source: tmp_source.into(),
                    palette: "isotem.pal".into(),
                    palette_source: palette_source("isotem.pal")?,
                    index: selection.sub_tile,
                    count: tmp.tiles.len(),
                    offset: [tile.offset_x, tile.offset_y],
                    width: tile.pixel_width,
                    height: tile.pixel_height,
                },
                rgba: tile_rgba,
            },
            sprite: DecodedImage {
                indices: Vec::new(),
                info: AssetInfo {
                    name: shp_name.into(),
                    source: shp_source.into(),
                    palette: "unittem.pal".into(),
                    palette_source: palette_source("unittem.pal")?,
                    index: selection.frame,
                    count: shp.frames.len(),
                    offset: [i32::from(frame.frame_x), i32::from(frame.frame_y)],
                    width: u32::from(frame.frame_width),
                    height: u32::from(frame.frame_height),
                },
                rgba: sprite_rgba,
            },
            archive_count: self.manager.registered_archive_names().len(),
            tileset_slots: self.theater.lookup.len(),
        })
    }
}

fn catalog_inputs_equal(old: &AssetManager, new: &AssetManager, theater_name: &str) -> bool {
    if old.ini_roots() != new.ini_roots() || old.ships_ares() != new.ships_ares() {
        return false;
    }
    let names = [
        old.ini_roots().rules(),
        old.ini_roots().art(),
        old.ini_roots().ai(),
        old.ini_roots().sound(),
        old.ini_roots().eva(),
        "ra2md.csf",
    ];
    let control = match theater_name.to_ascii_uppercase().as_str() {
        "TEMPERATE" => "temperat",
        "SNOW" => "snow",
        "URBAN" => "urban",
        "NEWURBAN" => "urbann",
        "DESERT" => "desert",
        "LUNAR" => "lunar",
        _ => return false,
    };
    let theater_ini = old.ini_roots().theater_ini(control);
    let palettes = match control {
        "temperat" => ["isotem.pal", "unittem.pal", "temperat.pal"],
        "snow" => ["isosno.pal", "unitsno.pal", "snow.pal"],
        "urban" => ["isourb.pal", "uniturb.pal", "urban.pal"],
        "urbann" => ["isoubn.pal", "unitubn.pal", "urbann.pal"],
        "desert" => ["isodes.pal", "unitdes.pal", "desert.pal"],
        _ => ["isolun.pal", "unitlun.pal", "lunar.pal"],
    };
    names
        .iter()
        .all(|name| old.get_ref(name) == new.get_ref(name))
        && old.get_ref(&theater_ini) == new.get_ref(&theater_ini)
        && palettes
            .iter()
            .all(|name| old.get_ref(name) == new.get_ref(name))
}

pub struct LoadResult {
    pub generation: u64,
    pub result: Result<PreviewAssets>,
}

/// A single worker owns the archive mappings. Only decoded images cross into UI.
pub struct AssetLoader {
    requests: Sender<(u64, AssetSelection, bool)>,
    pub results: Receiver<LoadResult>,
}

impl AssetLoader {
    pub fn new(ctx: egui::Context) -> Result<Self> {
        let (requests, incoming) = mpsc::channel::<(u64, AssetSelection, bool)>();
        let (outgoing, results) = mpsc::channel();
        std::thread::Builder::new()
            .name("asset-loader".into())
            .spawn(move || {
                let mut install: Option<Install> = None;
                while let Ok(mut request) = incoming.recv() {
                    // Superseded sliders/paths need no decoding or GPU upload.
                    for newer in incoming.try_iter() {
                        request = newer;
                    }
                    let (generation, selection, refresh) = request;
                    let result = (|| {
                        if refresh
                            || install
                                .as_ref()
                                .is_none_or(|i| i.directory != selection.directory)
                        {
                            // Drop the old mapping before opening a different installation.
                            install = None;
                            install = Some(Install::open(&selection.directory)?);
                        }
                        install
                            .as_ref()
                            .context("资源目录未加载")?
                            .decode(&selection)
                    })();
                    if outgoing.send(LoadResult { generation, result }).is_err() {
                        break;
                    }
                    ctx.request_repaint();
                }
            })
            .context("无法启动资源加载线程")?;
        Ok(Self { requests, results })
    }

    pub fn request(&self, generation: u64, selection: AssetSelection, refresh: bool) -> Result<()> {
        if self
            .requests
            .send((generation, selection, refresh))
            .is_err()
        {
            bail!("资源加载线程已退出");
        }
        Ok(())
    }
}

#[cfg(test)]
#[path = "assets_tests.rs"]
pub(crate) mod tests;

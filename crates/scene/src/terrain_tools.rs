//! Terrain authoring commands. Work from cached geometry and write only through
//! MapDocument, leaving commit/cancel ownership to the caller's stroke transaction.
use crate::map::MapScene;
use anyhow::{Context, Result, ensure};
use ra2_map_assets::catalog::{TerrainBrush, TerrainTemplate};
use ra2_map_document::document::{Cell, MapDocument, Target, TerrainValue, Value};
use std::collections::{BTreeSet, HashMap};
use vera20k_core::map::cell_index::MAX_CELL_SIDE;
use vera20k_core::map::{
    corner_heights::{CornerCells, CornerGrid, FinalTiles},
    lat::{LatConfig, SlopeFixupConfig, apply_load_recalc_sweeps},
    map_file::MapCell,
    playfield::size_diamond_contains,
    rmg::phases::hills_corners::RMG_HEIGHTS,
};

struct AuthoredCorners<'a> {
    size: (i32, i32),
    cells: &'a HashMap<(i32, i32), (i32, u8, u8)>,
    protected: &'a BTreeSet<(i32, i32)>,
    morphable: &'a dyn Fn(i32) -> bool,
}

impl CornerCells for AuthoredCorners<'_> {
    fn in_diamond(&self, x: i32, y: i32) -> bool {
        size_diamond_contains(self.size.0, self.size.1, (x as i16, y as i16))
    }

    fn level_slope(&self, x: i32, y: i32) -> (i32, u8) {
        self.cells
            .get(&(x, y))
            .map_or((0, 0), |&(_, level, slope)| (level.into(), slope))
    }

    fn editable(&self, x: i32, y: i32) -> bool {
        !self.protected.contains(&(x, y))
            && self
                .cells
                .get(&(x, y))
                .is_some_and(|&(tile, _, _)| (self.morphable)(tile))
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TerrainMode {
    #[default]
    Subtile,
    Template,
    Fill,
    Raise,
    Lower,
    Flatten,
    HeightFill,
    CliffHeightRepair,
    Plateau,
    CliffOutline,
    CliffPolyline,
    CliffFront,
    CliffBack,
    PlateauCut,
    RampEntrance,
    Bridge,
    Water,
    WaterOutline,
    WaterPolyline,
    LandFill,
    ShoreRepair,
    Road,
    RoadPolyline,
    RoadCut,
    CoastalCliff,
}
impl TerrainMode {
    pub const ALL: [Self; 25] = [
        Self::Subtile,
        Self::Template,
        Self::Fill,
        Self::Raise,
        Self::Lower,
        Self::Flatten,
        Self::HeightFill,
        Self::CliffHeightRepair,
        Self::Plateau,
        Self::CliffOutline,
        Self::CliffPolyline,
        Self::CliffFront,
        Self::CliffBack,
        Self::PlateauCut,
        Self::RampEntrance,
        Self::Bridge,
        Self::Water,
        Self::WaterOutline,
        Self::WaterPolyline,
        Self::LandFill,
        Self::ShoreRepair,
        Self::Road,
        Self::RoadPolyline,
        Self::RoadCut,
        Self::CoastalCliff,
    ];
    pub fn name(self) -> &'static str {
        match self {
            Self::Subtile => "子块画笔",
            Self::Template => "完整模板",
            Self::Fill => "框选铺地",
            Self::Raise => "升高",
            Self::Lower => "降低",
            Self::Flatten => "平整",
            Self::HeightFill => "填高地（保留悬崖）",
            Self::CliffHeightRepair => "修正悬崖高度（保留子块）",
            Self::Plateau => "矩形高台（自动悬崖）",
            Self::CliffOutline => "围合高台（自动拐角）",
            Self::CliffPolyline => "折线悬崖",
            Self::CliffFront => "悬崖线段（正面）",
            Self::CliffBack => "悬崖线段（背面）",
            Self::PlateauCut => "切除高台（矩形）",
            Self::RampEntrance => "插入高台坡道",
            Self::Bridge => "桥梁（拖动连接两端）",
            Self::Water => "矩形水域（自动岸线）",
            Self::WaterOutline => "围合水域（自动岸线）",
            Self::WaterPolyline => "折线水域（自动岸线）",
            Self::LandFill => "回填陆地（修补岸线）",
            Self::ShoreRepair => "修复岸线（矩形）",
            Self::Road => "道路（自动转角 / 路口）",
            Self::RoadPolyline => "连续画路",
            Self::RoadCut => "切除道路（修补端点）",
            Self::CoastalCliff => "匹配临水悬崖（矩形）",
        }
    }
    pub fn edits_height(self) -> bool {
        matches!(self, Self::Raise | Self::Lower | Self::Flatten)
    }
    pub fn planned(self) -> bool {
        self.joining()
            || matches!(
                self,
                Self::HeightFill
                    | Self::CliffHeightRepair
                    | Self::Plateau
                    | Self::CliffOutline
                    | Self::CliffPolyline
                    | Self::CliffFront
                    | Self::CliffBack
                    | Self::PlateauCut
                    | Self::RampEntrance
                    | Self::Bridge
            )
    }
    pub fn joining(self) -> bool {
        matches!(
            self,
            Self::Water
                | Self::WaterOutline
                | Self::WaterPolyline
                | Self::LandFill
                | Self::ShoreRepair
                | Self::Road
                | Self::RoadPolyline
                | Self::RoadCut
                | Self::CoastalCliff
        )
    }
}

pub fn value(doc: &MapDocument, scene: &MapScene, at: Cell) -> TerrainValue {
    doc.terrain(at).unwrap_or(TerrainValue {
        tile: scene
            .palette
            .tile_ids
            .map_or(0, |ids| raw(scene, ids.clear).unwrap_or(0)),
        sub: 0,
        level: scene.source.header.level as u8,
        ice: 0,
    })
}

/// Exact floor at an authored object's cell centre, including prospective
/// terrain and overlays that have not reached the resolved scene yet.
pub fn preview_object_world_z(
    scene: &MapScene,
    at: Cell,
    on_bridge: bool,
    terrain_at: impl Fn(Cell) -> TerrainValue,
    overlay_at: impl Fn(Cell) -> u8,
) -> i32 {
    let deck = on_bridge
        .then(|| {
            crate::bridge_tools::authored_deck_level(scene, at, |c| terrain_at(c).level, overlay_at)
        })
        .flatten();
    preview_floor(scene, at, terrain_at(at), on_bridge, deck)
}

/// Resolve the object floor after the pending region's terrain and overlays
/// have been applied. `terrain_at` excludes cells outside the map.
pub fn preview_region_object_world_z(
    scene: &MapScene,
    at: Cell,
    on_bridge: bool,
    terrain_at: impl Fn(Cell) -> Option<TerrainValue>,
    overlay_at: impl Fn(Cell) -> u8,
) -> i32 {
    let deck = on_bridge
        .then(|| crate::bridge_tools::prospective_deck_level(scene, at, &terrain_at, overlay_at))
        .flatten();
    let value = terrain_at(at).expect("region object placement must be within the map");
    preview_floor(scene, at, value, on_bridge, deck)
}

fn preview_floor(
    scene: &MapScene,
    at: Cell,
    value: TerrainValue,
    on_bridge: bool,
    deck: Option<i16>,
) -> i32 {
    use vera20k_core::util::lepton::{BRIDGE_DECK_HEIGHT_LEPTONS, GROUND_LEVEL_HEIGHT_LEPTONS};
    if let Some(level) = deck {
        return i32::from(level) * GROUND_LEVEL_HEIGHT_LEPTONS;
    }
    let tile = scene
        .palette
        .raw_ids
        .get(&value.tile)
        .copied()
        .or_else(|| u16::try_from(value.tile).ok());
    let slope = tile
        .and_then(|tile| scene.palette.templates.get(&(tile, 0)))
        .and_then(|t| t.cells.get(usize::from(value.sub)))
        .and_then(|s| *s)
        .map_or(0, |s| s.slope);
    ground_world_z(at, value.level, slope)
        + if on_bridge {
            BRIDGE_DECK_HEIGHT_LEPTONS
        } else {
            0
        }
}

pub(crate) fn ground_world_z(at: Cell, level: u8, slope: u8) -> i32 {
    use vera20k_core::util::lepton::{GROUND_LEVEL_HEIGHT_LEPTONS, ground_height_leptons};
    ground_height_leptons(
        level,
        slope,
        i32::from(at[0]) * 256 + 128,
        i32::from(at[1]) * 256 + 128,
    )
    .unwrap_or(i32::from(level as i8) * GROUND_LEVEL_HEIGHT_LEPTONS)
}

fn raw(scene: &MapScene, tile: i32) -> Option<i32> {
    scene
        .catalog
        .raw_tiles
        .get(&u16::try_from(tile).ok()?)
        .copied()
}

fn translated(scene: &MapScene) -> HashMap<i32, i32> {
    scene
        .catalog
        .raw_tiles
        .iter()
        .map(|(&tile, &raw)| (raw, i32::from(tile)))
        .collect()
}

fn tile_id(ids: &HashMap<i32, i32>, v: TerrainValue) -> i32 {
    ids.get(&v.tile).copied().unwrap_or(v.tile)
}

fn template<'a>(scene: &'a MapScene, brush: &TerrainBrush) -> Result<&'a TerrainTemplate> {
    scene
        .palette
        .templates
        .get(&(brush.key.tile_id, brush.key.variant))
        .context("模板几何不可用")
}

pub fn footprint(scene: &MapScene, brush: &TerrainBrush, at: Cell) -> Result<Vec<(Cell, u8, u8)>> {
    let template = template(scene, brush)?;
    ensure!(template.width > 0, "模板宽度无效");
    ensure!(
        template
            .cells
            .get(usize::from(brush.key.sub_tile))
            .is_some_and(Option::is_some),
        "所选子块为空"
    );
    let anchor = [
        i32::from(at[0]) - i32::from(brush.key.sub_tile) % i32::from(template.width),
        i32::from(at[1]) - i32::from(brush.key.sub_tile) / i32::from(template.width),
    ];
    template
        .cells
        .iter()
        .enumerate()
        .filter_map(|(sub, metadata)| metadata.map(|metadata| (sub, metadata)))
        .map(|(sub, metadata)| {
            let x = anchor[0] + sub as i32 % i32::from(template.width);
            let y = anchor[1] + sub as i32 / i32::from(template.width);
            ensure!(
                (0..i32::from(MAX_CELL_SIDE)).contains(&x)
                    && (0..i32::from(MAX_CELL_SIDE)).contains(&y),
                "完整模板越过地图边界"
            );
            Ok(([x as u16, y as u16], sub as u8, metadata.height))
        })
        .collect()
}

/// Keep the clicked subtile under the cursor; TMP height bytes are relative to
/// the shared template base level. Validate the whole placement before writing.
pub fn stamp(
    doc: &mut MapDocument,
    scene: &MapScene,
    brush: &TerrainBrush,
    at: Cell,
    whole: bool,
) -> Result<Vec<Cell>> {
    stamp_at_height(doc, scene, brush, at, whole, None)
}

/// Shared preflight for both the canvas ghost and the committed template.
/// An explicit level refers to the template base, not the selected anchor.
pub fn template_placement(
    doc: &MapDocument,
    scene: &MapScene,
    brush: &TerrainBrush,
    at: Cell,
    base_level: Option<u8>,
) -> Result<Vec<(Cell, TerrainValue)>> {
    doc.check_cell(at)?;
    let t = template(scene, brush)?;
    let selected_height = t
        .cells
        .get(usize::from(brush.key.sub_tile))
        .and_then(|cell| *cell)
        .context("所选子块为空")?
        .height;
    let base = base_level.map_or_else(
        || i16::from(value(doc, scene, at).level) - i16::from(selected_height),
        i16::from,
    );
    let cliff = crate::height_tools::cliff_geometry(
        scene,
        TerrainValue {
            tile: brush.raw,
            sub: brush.key.sub_tile,
            level: 0,
            ice: 0,
        },
    )?;
    // A back-face template may contain only high-side cells. On low ground
    // its default anchor must not infer a negative cliff base.
    let base = if base_level.is_none() && cliff.is_some() && base < 0 {
        i16::from(value(doc, scene, at).level)
    } else {
        base
    };
    if let Some((_, top)) = cliff {
        ensure!(
            base >= 0 && base + i16::from(top) <= 127,
            "悬崖基准或崖顶高度越过 0～127，请调整放置高度"
        );
    }
    // Map format records no visual variant. Geometry must match the
    // pristine template which the engine uses to interpret saved subtiles.
    let pristine_template = scene.palette.templates.get(&(brush.key.tile_id, 0));
    ensure!(
        pristine_template.is_some_and(|p| p.width == t.width
            && p.height == t.height
            && p.cells.len() == t.cells.len()
            && p.cells
                .iter()
                .zip(&t.cells)
                .all(|(a, b)| a.is_some() == b.is_some())),
        "此变体的几何与基础模板不同，不能保存为完整地形"
    );
    let mut edits = Vec::new();
    for (cell, sub, height) in footprint(scene, brush, at)? {
        doc.check_cell(cell)?;
        let key = vera20k_core::map::theater::TileKey {
            sub_tile: sub,
            ..brush.key
        };
        ensure!(
            scene.palette.images.contains_key(&key),
            "模板子块资源缺失，未修改地图"
        );
        let pristine = pristine_template
            .and_then(|t| t.cells.get(usize::from(sub)))
            .and_then(|s| *s);
        ensure!(
            pristine.is_some_and(
                |s| s.height == height && s.slope == t.cells[usize::from(sub)].unwrap().slope
            ),
            "此变体的几何与基础模板不同，不能保存为完整地形"
        );
        let level = base + i16::from(height);
        ensure!(
            (0..=127).contains(&level),
            "模板高度越过 0～127，请调整基准高度或放置位置"
        );
        edits.push((
            cell,
            TerrainValue {
                tile: brush.raw,
                sub,
                level: level as u8,
                ..value(doc, scene, cell)
            },
        ));
    }
    Ok(edits)
}

pub fn stamp_at_height(
    doc: &mut MapDocument,
    scene: &MapScene,
    brush: &TerrainBrush,
    at: Cell,
    whole: bool,
    base_level: Option<u8>,
) -> Result<Vec<Cell>> {
    let mut edits = Vec::new();
    if whole {
        edits = template_placement(doc, scene, brush, at, base_level)?;
    } else {
        edits.push((
            at,
            subtile_placement_at_height(doc, scene, brush, at, base_level)?,
        ));
    }
    apply(doc, edits)
}

/// A cliff subtile carries a height relative to its low ground. Keep that
/// relative height when painting, and recover the old base when repainting.
pub fn subtile_placement(
    doc: &MapDocument,
    scene: &MapScene,
    brush: &TerrainBrush,
    at: Cell,
) -> Result<TerrainValue> {
    subtile_placement_at_height(doc, scene, brush, at, None)
}

pub fn subtile_placement_at_height(
    doc: &MapDocument,
    scene: &MapScene,
    brush: &TerrainBrush,
    at: Cell,
    base_level: Option<u8>,
) -> Result<TerrainValue> {
    doc.check_cell(at)?;
    let old = value(doc, scene, at);
    let geometry = template(scene, brush)?
        .cells
        .get(usize::from(brush.key.sub_tile))
        .and_then(|s| *s)
        .context("所选子块为空")?;
    ensure!(
        scene.palette.images.contains_key(&brush.key),
        "子块资源缺失，未修改地形"
    );
    let pristine = scene
        .palette
        .templates
        .get(&(brush.key.tile_id, 0))
        .and_then(|t| t.cells.get(usize::from(brush.key.sub_tile)))
        .and_then(|s| *s);
    ensure!(
        pristine.is_some_and(|s| s.height == geometry.height && s.slope == geometry.slope),
        "此变体的几何与基础模板不同，不能保存为地形子块"
    );
    let mut next = TerrainValue {
        tile: brush.raw,
        sub: brush.key.sub_tile,
        ..old
    };
    if let Some((relative, top)) = crate::height_tools::cliff_geometry(scene, next)? {
        let base = if let Some(base) = base_level {
            base
        } else if let Some((height, _)) = crate::height_tools::cliff_geometry(scene, old)? {
            old.level
                .checked_sub(height)
                .context("原悬崖子块高度无效，请先修正悬崖高度")?
        } else {
            old.level
        };
        next.level = base
            .checked_add(relative)
            .filter(|&level| level <= 127)
            .context("悬崖子块高度越过 0～127，未修改地形")?;
        ensure!(
            u16::from(base) + u16::from(top) <= 127,
            "悬崖崖顶高度越过 0～127，未修改地形"
        );
    } else if let Some(base) = base_level {
        next.level = base
            .checked_add(geometry.height)
            .filter(|&level| level <= 127)
            .context("子块高度越过 0～127，未修改地形")?;
    }
    Ok(next)
}

fn apply(doc: &mut MapDocument, edits: Vec<(Cell, TerrainValue)>) -> Result<Vec<Cell>> {
    for (at, _) in &edits {
        doc.check_cell(*at)?;
    }
    let mut changed = Vec::new();
    for (at, terrain) in edits {
        if doc.terrain(at) != Some(terrain) {
            doc.set(Target::Terrain(at), Some(Value::Terrain(terrain)))?;
            changed.push(at);
        }
    }
    Ok(changed)
}

pub fn neighbors(at: Cell) -> impl Iterator<Item = Cell> {
    [(0, -1), (1, 0), (0, 1), (-1, 0)]
        .into_iter()
        .filter_map(move |(dx, dy)| {
            let (x, y) = (i32::from(at[0]) + dx, i32::from(at[1]) + dy);
            ((0..i32::from(MAX_CELL_SIDE)).contains(&x)
                && (0..i32::from(MAX_CELL_SIDE)).contains(&y))
            .then_some([x as u16, y as u16])
        })
}

/// Reuse the engine's two LAT/slope sweeps with a padded read context. Only
/// the stroke and its two-cell halo are persisted; distant map art stays intact.
pub fn recalculate(
    doc: &mut MapDocument,
    scene: &MapScene,
    touched: &BTreeSet<Cell>,
) -> Result<Vec<Cell>> {
    let ids = scene.palette.tile_ids.context("当前环境缺少地形连接配置")?;
    let mut write = touched.clone();
    for _ in 0..2 {
        write.extend(write.clone().into_iter().flat_map(neighbors));
    }
    let mut read = write.clone();
    for _ in 0..3 {
        read.extend(read.clone().into_iter().flat_map(neighbors));
    }
    let translated = translated(scene);
    let mut cells: Vec<_> = read
        .into_iter()
        .filter(|at| doc.check_cell(*at).is_ok())
        .map(|at| {
            let v = value(doc, scene, at);
            MapCell {
                rx: at[0],
                ry: at[1],
                tile_index: tile_id(&translated, v),
                sub_tile: v.sub,
                z: v.level,
                legacy_ice_byte: v.ice,
            }
        })
        .collect();
    // Match the native cell iterator's stable sum/x order.
    cells.sort_by_key(|c| (u32::from(c.rx) + u32::from(c.ry), c.rx));
    apply_load_recalc_sweeps(
        &mut cells,
        &LatConfig::from_tile_ids(&ids),
        SlopeFixupConfig::from_tile_ids(&ids),
        &mut |tile, sub| {
            u16::try_from(tile)
                .ok()
                .and_then(|tile| scene.palette.templates.get(&(tile, 0)))
                .and_then(|t| t.cells.get(usize::from(sub)))
                .and_then(|s| *s)
                .map_or(0, |s| s.slope)
        },
    );
    let edits = cells
        .into_iter()
        .filter(|c| write.contains(&[c.rx, c.ry]))
        .filter_map(|c| {
            let key = vera20k_core::map::theater::TileKey {
                tile_id: u16::try_from(c.tile_index).ok()?,
                sub_tile: c.sub_tile,
                variant: 0,
            };
            scene.palette.images.contains_key(&key).then_some(())?;
            let at = [c.rx, c.ry];
            Some((
                at,
                TerrainValue {
                    tile: raw(scene, c.tile_index)?,
                    ..value(doc, scene, at)
                },
            ))
        })
        .collect();
    apply(doc, edits)
}

/// Relative steps or a directional absolute target for raise/lower tools.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HeightChange {
    Step(u8),
    To(u8),
}
impl HeightChange {
    pub fn target(self, mode: TerrainMode, old: u8) -> u8 {
        match (self, mode) {
            (Self::Step(amount), TerrainMode::Raise) => old.saturating_add(amount),
            (Self::Step(amount), TerrainMode::Lower) => old.saturating_sub(amount),
            (Self::To(level), TerrainMode::Raise) => old.max(level),
            (Self::To(level), TerrainMode::Lower) => old.min(level),
            _ => old,
        }
    }
}

/// Height editing uses the game's corner grid, with smooth levels 0..12 and
/// raw levels 0..127. This is not FA2's editor slope algorithm.
pub fn height(
    doc: &mut MapDocument,
    scene: &MapScene,
    selected: &BTreeSet<Cell>,
    mode: TerrainMode,
    level: u8,
    smooth: bool,
) -> Result<Vec<Cell>> {
    height_with_change(
        doc,
        scene,
        selected,
        mode,
        level,
        smooth,
        HeightChange::Step(1),
    )
}

pub fn height_with_change(
    doc: &mut MapDocument,
    scene: &MapScene,
    selected: &BTreeSet<Cell>,
    mode: TerrainMode,
    level: u8,
    smooth: bool,
    change: HeightChange,
) -> Result<Vec<Cell>> {
    ensure!(mode.edits_height(), "不是高度工具");
    ensure!(
        match change {
            HeightChange::Step(amount) => (1..=127).contains(&amount),
            HeightChange::To(level) => level <= 127,
        },
        "升降层数须为 1～127，目标高度须为 0～127"
    );
    if selected.is_empty() {
        return Ok(Vec::new());
    }
    let target = |old: u8| match mode {
        TerrainMode::Raise | TerrainMode::Lower => change.target(mode, old),
        _ => level,
    };
    if !smooth {
        let edits = crate::height_tools::raw(doc, scene, selected, mode, level, change)?
            .into_iter()
            .collect();
        return apply(doc, edits);
    }
    let ids = scene.palette.tile_ids.context("当前环境没有坡面配置")?;
    ensure!(
        ids.clear >= 0 && ids.ramp_base >= 0,
        "当前环境没有可用的坡面模板；可关闭自动坡面直接编辑高度"
    );
    let (width, height) = (doc.base.header.width as i32, doc.base.header.height as i32);
    let span = width + height;
    let domain = || {
        (width + 1..=width + 2 * height).flat_map(|row| {
            let first = (row - width) / 2 + 1;
            (first..row - first + 1).map(move |x| (x, row - x))
        })
    };
    let translated = translated(scene);
    let mut grid = HashMap::new();
    let mut protected = BTreeSet::new();
    for (x, y) in domain() {
        let old = value(doc, scene, [x as u16, y as u16]);
        let tile = tile_id(&translated, old);
        let slope = u16::try_from(tile)
            .ok()
            .and_then(|tile| scene.palette.templates.get(&(tile, 0)))
            .and_then(|t| t.cells.get(usize::from(old.sub)))
            .and_then(|s| *s)
            .map_or(0, |s| s.slope);
        grid.insert((x, y), (tile, old.level, slope));
        if old.level > 12
            || !crate::bridge_tools::height_editable_overlay(doc, scene, [x as u16, y as u16])
        {
            protected.insert((x, y));
        }
    }
    for record in doc
        .records()
        .values()
        .filter(|r| r.section != "Waypoints" && r.section != "Smudge")
    {
        if let Some(at) = record.cell() {
            for (dx, dy) in crate::placement::offsets(scene, &record.section, &record.type_id()) {
                protected.insert((
                    i32::from(at[0]) + i32::from(dx),
                    i32::from(at[1]) + i32::from(dy),
                ));
            }
        }
    }
    let morphable = |tile: i32| {
        u16::try_from(tile)
            .ok()
            .is_some_and(|tile| scene.palette.morphable.contains(&tile))
            || tile == ids.clear
            || (ids.ramp_base..ids.ramp_base + 20).contains(&tile)
            || ids.ramp_smooth >= 0 && (ids.ramp_smooth..ids.ramp_smooth + 12).contains(&tile)
    };
    let cells = AuthoredCorners {
        size: (width, height),
        cells: &grid,
        protected: &protected,
        morphable: &morphable,
    };
    let mut corners = CornerGrid::build(&cells, (1, 1), span, RMG_HEIGHTS);
    for &at in selected {
        doc.check_cell(at)?;
        let (x, y) = (i32::from(at[0]), i32::from(at[1]));
        let &(_, level, _) = grid.get(&(x, y)).context("格子在 Size 之外")?;
        let goal = target(level);
        if mode != TerrainMode::Flatten && goal == level {
            continue;
        }
        ensure!(
            goal <= 12,
            "自动坡面支持 0～12 层；可关闭自动坡面编辑到 127 层"
        );
        ensure!(
            cells.editable(x, y),
            "自动坡面不能修改水面、道路、建筑或覆盖物占用的格子"
        );
        for _ in 0..28 {
            let current = corners.current_level(&cells, x, y);
            // Equal base height alone does not flatten an existing slope.
            let flat = corners.pick_mask(x, y, 1) == corners.pick_mask(x, y, -1);
            if current == i32::from(goal) && (mode != TerrainMode::Flatten || flat) {
                break;
            }
            let direction = if current < i32::from(goal) { 1 } else { -1 };
            let mask = corners.pick_mask(x, y, direction);
            ensure!(
                mask != 0 && corners.apply(direction, x, y, mask),
                "坡面扩展被边界或不可变地形阻挡"
            );
        }
        ensure!(
            corners.current_level(&cells, x, y) == i32::from(goal),
            "无法构造连续坡面"
        );
    }
    let finals = corners.finalize(
        &cells,
        domain(),
        FinalTiles {
            clear: ids.clear,
            ramp_base: ids.ramp_base,
        },
    );
    let mut edits = Vec::new();
    let mut final_levels = HashMap::new();
    for final_cell in finals {
        let (x, y) = (final_cell.x, final_cell.y);
        let at = [x as u16, y as u16];
        let old = value(doc, scene, at);
        let old_tile = tile_id(&translated, old);
        let tile = final_cell.slope_tile.map_or(old_tile, |(_, tile)| tile);
        let level = final_cell.level as u8;
        final_levels.insert(at, level);
        if level == old.level && tile == old_tile {
            continue;
        }
        let sub = if tile != old_tile { 0 } else { old.sub };
        let key = vera20k_core::map::theater::TileKey {
            tile_id: u16::try_from(tile)?,
            sub_tile: sub,
            variant: 0,
        };
        ensure!(
            scene.palette.images.contains_key(&key),
            "坡面资源缺失，未修改地图"
        );
        edits.push((
            at,
            TerrainValue {
                tile: raw(scene, tile).context("坡面无法转换为地图编号")?,
                sub,
                level,
                ..old
            },
        ));
    }
    // A later corner adjustment can change an earlier cell, including a
    // selected high cell that an absolute raise deliberately skipped. Check
    // every requested goal after finalization, before writing any terrain.
    if mode != TerrainMode::Flatten {
        for &at in selected {
            let expected = target(value(doc, scene, at).level);
            let actual = final_levels
                .get(&at)
                .copied()
                .unwrap_or_else(|| value(doc, scene, at).level);
            ensure!(
                actual == expected,
                "{},{} 的目标高度 {} 与邻接坡面冲突；请缩小范围或关闭自动坡面，未修改地形",
                at[0],
                at[1],
                expected
            );
        }
    }
    apply(doc, edits)
}

/// Wall masks use the engine's N/E/S/W bit order and connect same types only.
/// Recalculate both placed and deleted wall neighbours in the same transaction.
pub fn connect_walls(
    doc: &mut MapDocument,
    scene: &MapScene,
    touched: &BTreeSet<Cell>,
) -> Result<Vec<Cell>> {
    let walls: BTreeSet<_> = scene
        .catalog
        .overlays
        .iter()
        .enumerate()
        .filter(|(_, id)| {
            scene
                .catalog
                .rules
                .section_or_empty(id)
                .read_bool("Wall", false)
        })
        .filter_map(|(i, _)| u8::try_from(i).ok())
        .collect();
    let mut area = touched.clone();
    area.extend(touched.iter().copied().flat_map(neighbors));
    let mut changed = Vec::new();
    for at in area {
        if doc.check_cell(at).is_err() {
            continue;
        }
        let (id, frame) = doc.overlay(at);
        if !walls.contains(&id) {
            continue;
        }
        let mut mask = 0;
        for (bit, (dx, dy)) in [(0, -1), (1, 0), (0, 1), (-1, 0)].into_iter().enumerate() {
            let (x, y) = (i32::from(at[0]) + dx, i32::from(at[1]) + dy);
            if (0..i32::from(MAX_CELL_SIDE)).contains(&x)
                && (0..i32::from(MAX_CELL_SIDE)).contains(&y)
                && doc.overlay([x as u16, y as u16]).0 == id
            {
                mask |= 1 << bit;
            }
        }
        let connected = (frame & 0xf0) | mask;
        if connected != frame {
            doc.set(Target::Overlay(at), Some(Value::Overlay(id, connected)))?;
            changed.push(at);
        }
    }
    Ok(changed)
}

#[cfg(test)]
#[path = "terrain_tools_tests.rs"]
mod tests;

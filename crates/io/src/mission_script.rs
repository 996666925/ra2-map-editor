//! Mission source IO. Script evaluation never receives filesystem access.
use anyhow::{Context, Result, ensure};
use ra2_map_document::document::MapDocument;
use ra2_map_mission_script::{
    catalog::ResourceKind,
    native::{Build, BuildContext, CampaignBuildContext, Catalog, Mission, prepare_with_context},
    typescript::Compiler,
};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    sync::mpsc,
};

pub fn catalog(doc: &MapDocument, scene: &ra2_map_scene::map::MapScene) -> Catalog {
    let units = scene
        .catalog
        .object_types
        .iter()
        .filter(|(section, _)| matches!(section.as_str(), "Infantry" | "Units" | "Aircraft"))
        .flat_map(|(_, ids)| ids.iter().cloned())
        .collect();
    let strings = scene
        .mission_resources
        .choices
        .get("Csf")
        .into_iter()
        .flatten()
        .map(|(id, _)| id.clone())
        .collect();
    let mut catalog = Catalog::from_document(doc, units, strings);
    catalog.profile = scene.revision.id();
    catalog.type_projection =
        ra2_map_mission_script::native::registry_fingerprints(scene.source.bytes())
            .unwrap_or_default();
    catalog
        .type_projection
        .retain(|section, _| !matches!(section.as_str(), "ScriptTypes" | "Houses" | "Waypoints"));
    if scene.mission_definitions.error.is_none() {
        catalog.fixed_scripts = Some(scene.mission_definitions.script_order.clone());
        catalog.fixed_team_scripts = scene.mission_definitions.team_script_references.clone();
        catalog.native_definitions.insert(
            "team".into(),
            scene.mission_definitions.teams.iter().cloned().collect(),
        );
        catalog.native_definitions.insert(
            "script".into(),
            scene.mission_definitions.scripts.iter().cloned().collect(),
        );
        catalog.native_definitions.insert(
            "taskForce".into(),
            scene
                .mission_definitions
                .task_forces
                .iter()
                .cloned()
                .collect(),
        );
    }
    populate_resource_catalog(&mut catalog, &scene.catalog, &scene.mission_resources);
    catalog.vehicle_templates = scene.catalog.mission_vehicle_templates.clone();
    let mut unit_names = BTreeMap::new();
    for (section, category) in [
        ("Infantry", "步兵"),
        ("Units", "车辆"),
        ("Aircraft", "飞机"),
    ] {
        if let Some(ids) = scene.catalog.object_types.get(section) {
            for id in ids {
                unit_names.insert(
                    id.clone(),
                    format!("{category}：{}（{id}）", scene.catalog.object_name_zh(id)),
                );
            }
        }
    }
    catalog.descriptions.insert("units".into(), unit_names);
    let mut houses = BTreeMap::new();
    for id in &catalog.houses {
        let country = doc.ini_text(id, "Country");
        let name = match id.as_str() {
            "Neutral" => "中立方",
            "Special" => "特殊方",
            _ => scene
                .catalog
                .object_name_zh(if country.is_empty() { id } else { &country }),
        };
        houses.insert(id.clone(), format!("所属方：{name}（{id}）"));
    }
    catalog.descriptions.insert("houses".into(), houses);
    let mut objects = BTreeMap::new();
    let mut waypoints = BTreeMap::new();
    for record in doc.records().values() {
        if record.section == "Waypoints" {
            if let Some([x, y]) = record.cell() {
                waypoints.insert(
                    record.key.clone(),
                    format!("航点 {}；坐标（{x}, {y}）", record.key),
                );
            }
        } else {
            let key = format!("{}:{}", record.section, record.key);
            if catalog.objects.contains(&key) {
                let id = record.type_id();
                let fields = record.fields();
                let owner = fields.first().map_or("未知", String::as_str);
                let position = record
                    .cell()
                    .map_or_else(String::new, |[x, y]| format!("；坐标（{x}, {y}）"));
                objects.insert(
                    key.clone(),
                    format!(
                        "地图对象：{}（{id}）；记录 {key}；所属方 {owner}{position}",
                        scene.catalog.object_name_zh(&id)
                    ),
                );
            }
        }
    }
    catalog.descriptions.insert("objects".into(), objects);
    catalog.descriptions.insert("waypoints".into(), waypoints);
    catalog.descriptions.insert(
        "strings".into(),
        scene
            .mission_resources
            .choices
            .get("Csf")
            .into_iter()
            .flatten()
            .map(|(id, text)| (id.clone(), format!("文本标签：{text}")))
            .collect(),
    );
    catalog
}

/// Read saved declarations through the same manifest validation as rebuild.
pub fn declared_texts(bytes: &[u8]) -> Result<BTreeMap<String, String>> {
    use ra2_map_document::document::Baseline;
    let map = crate::editor_io::validate(bytes)?;
    let doc = MapDocument::new(Baseline::parse(std::sync::Arc::from(bytes), &map)?);
    ra2_map_mission_script::native::declared_texts(&doc)
}

pub fn text_payload(
    bytes: &[u8],
    install: &ra2_map_assets::assets::Install,
) -> Result<Option<Vec<u8>>> {
    let strings = declared_texts(bytes)?;
    if strings.is_empty() {
        return Ok(None);
    }
    let empty: Vec<u8> = [0x43534620u32, 3, 0, 0, 0, 9]
        .into_iter()
        .flat_map(u32::to_le_bytes)
        .collect();
    Ok(Some(crate::campaign::merge_csf(
        install.manager.get_ref("ra2md.csf").unwrap_or(&empty),
        &strings,
    )?))
}

fn saved_document(bytes: &[u8]) -> Result<MapDocument> {
    let map = crate::editor_io::validate(bytes)?;
    Ok(MapDocument::new(
        ra2_map_document::document::Baseline::parse(std::sync::Arc::from(bytes), &map)?,
    ))
}

/// Read only files contained in the declared project root, including through links.
fn dependency(root: &Path, relative: &str) -> Result<PathBuf> {
    let path = fs::canonicalize(root.join(relative))
        .with_context(|| format!("缺少任务依赖：{relative}"))?;
    ensure!(
        path.starts_with(root) && path.is_file(),
        "任务依赖越过目录或不是文件：{relative}"
    );
    Ok(path)
}

pub fn saved_entry(doc: &MapDocument) -> Result<Option<String>> {
    Ok(ra2_map_mission_script::native::build_context(doc)?.map(|c| c.entry))
}

fn source_context(doc: &MapDocument, entry: &Path) -> Result<Option<BuildContext>> {
    let entry = fs::canonicalize(entry)?;
    Ok(ra2_map_mission_script::native::build_context(doc)?
        .filter(|context| fs::canonicalize(&context.entry).ok().as_ref() == Some(&entry)))
}

/// Refresh the directory used by the saved project, including nested missions.
pub fn refresh_document_types(doc: &MapDocument, entry: &Path, catalog: &Catalog) -> Result<()> {
    if let Some(context) = source_context(doc, entry)? {
        refresh_types_in(Path::new(&context.root), catalog)
    } else {
        refresh_types(entry, catalog)
    }
}

fn apply_transition(
    plan: &mut Mission,
    transition: &ra2_map_mission_script::campaign::Transition,
) -> Result<()> {
    let basic = plan
        .basic
        .get_or_insert(serde_json::from_value(serde_json::json!({}))?);
    basic.end_of_game = Some(transition.end_of_game);
    basic.one_time_only = Some(false);
    basic.skip_map_select = Some(true);
    basic.next_scenario = transition.next.clone();
    basic.alt_next_scenario = transition
        .alternate
        .clone()
        .or_else(|| transition.next.clone());
    Ok(())
}

/// In-memory campaign build shared by the GUI and CLI. No intermediate maps.
pub fn prepare_campaign(entry: &Path, directory: &Path) -> Result<crate::campaign::Prepared> {
    let mut sources = Sources::read(entry)?;
    let root = sources.root.clone();
    let mut compiler = Compiler::new()?;
    let campaign =
        compiler.build_campaign_at(sources.virtual_files.clone(), &sources.entry_module)?;
    let mut install = ra2_map_assets::assets::Install::open(directory)?;
    let mut overrides = BTreeMap::new();
    let mut snapshots = Vec::new();
    for mission in &campaign.project.missions {
        let path = dependency(&root, &mission.source)?;
        let original = fs::read(&path)?;
        ensure!(original.len() <= 256 * 1024 * 1024, "战役地图过大");
        sources.originals.insert(path.clone(), original.clone());
        let bytes = if let Some(script) = campaign.scripts.get(&mission.file) {
            let script = dependency(&root, script)?;
            let scene = ra2_map_scene::map::load_scene(
                &mut install,
                path.to_str().context("地图路径须为 Unicode")?,
            )?;
            let mut doc = MapDocument::new(
                scene
                    .source
                    .edit_base
                    .as_ref()
                    .map_err(|e| anyhow::anyhow!(e.clone()))?
                    .clone(),
            );
            let mut catalog = catalog(&doc, &scene);
            let mut mission_sources = Sources::read_in(&script, &root)?;
            mission_sources.campaign = Some(CampaignBuildContext {
                entry: sources.entry_module.trim_start_matches('/').into(),
                mission: mission.file.clone(),
            });
            let mut plan = compiler.build_at(
                mission_sources.virtual_files.clone(),
                &mission_sources.entry_module,
            )?;
            mission_sources.normalize_assets(&mut plan)?;
            if let Some(t) = campaign.transitions.get(&mission.file) {
                apply_transition(&mut plan, t)?;
            }
            ensure!(
                plan.mode == ra2_map_mission_script::native::MissionMode::SinglePlayer,
                "战役关卡必须为单人任务：{}",
                mission.file
            );
            let context = mission_sources.context(&plan.assets, catalog.profile.clone())?;
            mission_sources.media_catalog(&plan.assets, &mut catalog)?;
            prepare_with_context(&doc, &plan, &catalog, Some(context))?.apply(&mut doc)?;
            snapshots.push(mission_sources);
            doc.snapshot().bytes()?
        } else {
            original
        };
        let bytes = if let Some(t) = campaign
            .transitions
            .get(&mission.file)
            .filter(|_| !campaign.scripts.contains_key(&mission.file))
        {
            let mut fields = vec![
                (
                    "Basic".into(),
                    "EndOfGame".into(),
                    if t.end_of_game { "yes" } else { "no" }.into(),
                ),
                ("Basic".into(), "OneTimeOnly".into(), "no".into()),
                ("Basic".into(), "SkipMapSelect".into(), "yes".into()),
            ];
            fields.push((
                "Basic".into(),
                "NextScenario".into(),
                t.next.clone().unwrap_or_default(),
            ));
            fields.push((
                "Basic".into(),
                "AltNextScenario".into(),
                t.alternate
                    .clone()
                    .or_else(|| t.next.clone())
                    .unwrap_or_default(),
            ));
            crate::campaign::patch_ini(&bytes, &fields)?
        } else {
            bytes
        };
        overrides.insert(mission.file.to_ascii_lowercase(), bytes);
    }
    for asset in &campaign.project.assets {
        let path = dependency(&root, &asset.source)?;
        ensure!(
            fs::metadata(&path)?.len() <= 256 * 1024 * 1024,
            "战役资源过大"
        );
        sources.originals.insert(path.clone(), fs::read(path)?);
    }
    let result =
        crate::campaign::prepare_with_maps(&campaign.project, &root, directory, &overrides)?;
    sources.unchanged()?;
    for snapshot in snapshots {
        snapshot.unchanged()?;
    }
    Ok(result)
}

pub fn run_campaign_cli(args: &[std::ffi::OsString]) -> Result<i32> {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!(
            "ra2-map-editor campaign-build --game-dir DIR --script campaign.ts --output NEW_DIRECTORY"
        );
        return Ok(0);
    }
    let mut game = std::env::var_os("RA2_DIR").map(PathBuf::from);
    let mut script = None;
    let mut output = None;
    ensure!((args.len() - 1).is_multiple_of(2), "参数须成对指定");
    for pair in args[1..].as_chunks::<2>().0 {
        let value = PathBuf::from(&pair[1]);
        match pair[0].to_str() {
            Some("--game-dir") => game = Some(value),
            Some("--script") => script = Some(value),
            Some("--output") => output = Some(value),
            _ => anyhow::bail!("未知战役构建参数"),
        }
    }
    let result = prepare_campaign(
        &script.context("缺少 --script")?,
        &game.context("缺少 --game-dir 或 RA2_DIR")?,
    )?;
    ensure!(
        result.report.passed,
        "战役存在缺失依赖：{}",
        serde_json::to_string(&result.report)?
    );
    let report = crate::campaign::export(result, &output.context("缺少 --output")?)?;
    println!("{}", serde_json::to_string_pretty(&report)?);
    Ok(0)
}

/// Called on IO workers before play/export; all checks finish before any output.
pub fn resource_payload(bytes: &[u8], profile: &str) -> Result<BTreeMap<String, Vec<u8>>> {
    use ra2_map_assets::profile::{file_hash, hash};
    let doc = saved_document(bytes)?;
    let assets = ra2_map_mission_script::native::declared_assets(&doc)?;
    let context = ra2_map_mission_script::native::build_context(&doc)?;
    let Some(context) = context else {
        ensure!(assets.is_empty(), "任务缺少资源构建记录，请重新构建");
        return Ok(BTreeMap::new());
    };
    ra2_map_mission_script::native::check_product(&doc)?;
    ensure!(
        context.registries.is_empty()
            || context.registries == ra2_map_mission_script::native::registry_fingerprints(bytes)?,
        "地图原生注册顺序或航点已变化，请重新构建任务"
    );
    ensure!(
        context.sdk == hash(ra2_map_mission_script::SDK.as_bytes()),
        "任务 SDK 已变化，请重新构建"
    );
    ensure!(
        context.profile.is_empty() || context.profile == profile,
        "任务资源配置已变化，请重新构建"
    );
    let root = fs::canonicalize(&context.root).context("任务源码目录不存在，请重新构建")?;
    for (relative, expected) in &context.inputs {
        let path = dependency(&root, relative)?;
        ensure!(
            file_hash(&path)? == *expected,
            "任务依赖 {relative} 已变化，请重新构建"
        );
    }
    let mut result = BTreeMap::new();
    let mut total = 0usize;
    for asset in assets {
        let path = dependency(&root, &asset.source)?;
        ensure!(
            fs::metadata(&path)?.len() <= 256 * 1024 * 1024,
            "资源超过 256 MiB：{}",
            asset.source
        );
        let value = fs::read(path)?;
        ensure!(
            context.inputs.get(&asset.source) == Some(&hash(&value)),
            "任务资源 {} 未构建或已变化，请重新构建",
            asset.source
        );
        total = total.checked_add(value.len()).context("资源容量溢出")?;
        ensure!(total <= 512 * 1024 * 1024, "任务资源总量超过 512 MiB");
        result.insert(asset.name.to_ascii_lowercase(), value);
    }
    Ok(result)
}

pub fn playtest_payload(
    bytes: &[u8],
    directory: &Path,
    profile: &str,
) -> Result<BTreeMap<String, Vec<u8>>> {
    let payload = resource_payload(bytes, profile)?;
    let doc = saved_document(bytes)?;
    let texts = ra2_map_mission_script::native::declared_texts(&doc)?;
    if texts.is_empty() && ra2_map_mission_script::native::build_context(&doc)?.is_none() {
        return Ok(payload);
    }
    let install = ra2_map_assets::assets::Install::open(directory)?;
    prepare_playtest_payload(bytes, payload, &install)
}

fn prepare_playtest_payload(
    bytes: &[u8],
    mut payload: BTreeMap<String, Vec<u8>>,
    install: &ra2_map_assets::assets::Install,
) -> Result<BTreeMap<String, Vec<u8>>> {
    if let Some(csf) = text_payload(bytes, install)? {
        payload.insert("ra2md.csf".into(), csf);
    }
    let project = ra2_map_document::campaign::Project {
        version: 1,
        id: "SCRIPT_PLAYTEST".into(),
        entry: "spawnmap.map".into(),
        missions: vec![ra2_map_document::campaign::Mission {
            file: "spawnmap.map".into(),
            source: "spawnmap.map".into(),
            side: "GDI".into(),
            ..Default::default()
        }],
        ..Default::default()
    };
    let map = crate::editor_io::validate(bytes)?;
    // The campaign audit reads managed fields from the actual map bytes.
    // The launcher writes its map separately, so keep this entry audit-only.
    let existing = payload.insert("spawnmap.map".into(), bytes.to_vec());
    let report = crate::campaign::audit_resources(&project, &[map], &payload, install)?;
    payload.remove("spawnmap.map");
    if let Some(existing) = existing {
        payload.insert("spawnmap.map".into(), existing);
    }
    ensure!(
        report.passed,
        "试玩任务依赖未通过：{}",
        serde_json::to_string(&report)?
    );
    Ok(payload)
}

fn populate_resource_catalog(
    catalog: &mut Catalog,
    assets: &ra2_map_assets::catalog::Catalog,
    resources: &ra2_map_assets::mission_resources::MissionResources,
) {
    catalog.fixed_country_indices = assets.mission_country_indices.clone();
    catalog.country_names = assets.mission_country_names.clone();
    catalog.house_colors = assets
        .rules
        .section_or_empty("Colors")
        .keys()
        .map(str::to_owned)
        .collect();
    catalog.house_colors.sort();
    catalog.house_colors.dedup();
    for (kind, section) in [
        (ResourceKind::Building, "Structures"),
        (ResourceKind::Animation, "Animations"),
    ] {
        catalog.register(
            kind,
            assets
                .object_types
                .get(section)
                .cloned()
                .unwrap_or_default(),
        );
        // These are declared authoring choices. Implicit type allocation during
        // Rules processing cannot be reconstructed from their display lists.
        catalog.registry_order.remove(kind.collection());
    }
    let technos = ["Structures", "Infantry", "Units", "Aircraft"]
        .into_iter()
        .flat_map(|section| {
            assets
                .object_types
                .get(section)
                .into_iter()
                .flatten()
                .cloned()
        })
        .collect();
    catalog.register(ResourceKind::Techno, technos);
    catalog
        .registry_order
        .remove(ResourceKind::Techno.collection());
    for (kind, registry) in [
        (ResourceKind::Country, "Countries"),
        (ResourceKind::SuperWeapon, "SuperWeaponTypes"),
        (ResourceKind::Warhead, "Warheads"),
    ] {
        catalog.register(
            kind,
            assets
                .mission_registries
                .get(registry)
                .cloned()
                .unwrap_or_default(),
        );
        // Do not claim a complete process Type array from explicit declarations.
        // A later numeric-operand interface must supply its authoritative order.
        catalog.registry_order.remove(kind.collection());
    }
    for (kind, choice) in [
        (ResourceKind::Sound, "Sound"),
        (ResourceKind::Eva, "Eva"),
        (ResourceKind::Theme, "Theme"),
        (ResourceKind::Movie, "Movie"),
    ] {
        catalog.register(
            kind,
            resources
                .choices
                .get(choice)
                .into_iter()
                .flatten()
                .map(|(id, _)| id.clone())
                .collect(),
        );
    }
    catalog
        .countries
        .extend(catalog.fixed_country_indices.keys().cloned());
    catalog.countries.sort();
    catalog.countries.dedup();
    for (collection, order) in &assets.mission_type_orders {
        catalog
            .registry_order
            .insert(collection.clone(), order.clone());
        match collection.as_str() {
            "buildings" => catalog.buildings.extend(order.iter().cloned()),
            "superWeapons" => catalog.super_weapons.extend(order.iter().cloned()),
            "animations" => catalog.animations.extend(order.iter().cloned()),
            "voxelAnims" => catalog.voxel_anims.extend(order.iter().cloned()),
            "weapons" => catalog
                .weapons
                .extend(order.iter().filter(|id| !id.is_empty()).cloned()),
            "particleSystems" => catalog.particle_systems.extend(order.iter().cloned()),
            _ => catalog.units.extend(order.iter().cloned()),
        }
        if matches!(
            collection.as_str(),
            "buildings" | "vehicles" | "aircraft" | "infantry"
        ) {
            catalog.technos.extend(order.iter().cloned());
        }
    }
    catalog.units.sort();
    catalog.units.dedup();
    catalog.technos.sort();
    catalog.technos.dedup();
    catalog.buildings.sort();
    catalog.buildings.dedup();
    catalog.super_weapons.sort();
    catalog.super_weapons.dedup();
    catalog.animations.sort();
    catalog.animations.dedup();
    catalog.voxel_anims.sort();
    catalog.voxel_anims.dedup();
    catalog.weapons.sort();
    catalog.weapons.dedup();
    catalog.particle_systems.sort();
    catalog.particle_systems.dedup();
    for kind in ResourceKind::ALL {
        let descriptions = catalog
            .resources(kind)
            .iter()
            .map(|id| {
                let label = assets.object_name_zh(id);
                (id.clone(), format!("{}：{label}（{id}）", kind.kind()))
            })
            .collect();
        catalog
            .descriptions
            .insert(kind.collection().into(), descriptions);
    }
    catalog.descriptions.insert(
        "houseColors".into(),
        catalog
            .house_colors
            .iter()
            .map(|id| (id.clone(), format!("House 颜色方案：{id}")))
            .collect(),
    );
}

fn generated(path: &Path, content: &str, marker: &str) -> Result<()> {
    if path.exists() {
        let old = fs::read_to_string(path)?;
        ensure!(
            old.starts_with(marker),
            "{} 已存在且不是编辑器生成的文件",
            path.display()
        );
        if old == content {
            return Ok(());
        }
    }
    fs::write(path, content)?;
    Ok(())
}
pub fn refresh_types(entry: &Path, catalog: &Catalog) -> Result<()> {
    let parent = entry.parent().context("任务路径缺少目录")?;
    refresh_types_in(parent, catalog)
}
fn refresh_types_in(parent: &Path, catalog: &Catalog) -> Result<()> {
    let config = parent.join("tsconfig.json");
    // Existing author-owned configs keep their settings. New projects receive
    // strict IDE diagnostics, with isolatedModules matching SWC's transpilation.
    if !config.exists() {
        use std::io::Write;
        let content = serde_json::to_string_pretty(&serde_json::json!({
            "compilerOptions": {
                "target": "ES2015", "lib": ["ES2015"],
                "module": "Node16", "moduleResolution": "Node16",
                "strict": true, "noUncheckedIndexedAccess": true,
                "exactOptionalPropertyTypes": true, "isolatedModules": true,
                "noEmit": true, "allowImportingTsExtensions": true, "types": []
            },
            "include": ["**/*.ts"], "exclude": ["node_modules", "vendor", "target"]
        }))?;
        // create_new also prevents overwriting a config created concurrently.
        match fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&config)
        {
            Ok(mut file) => file.write_all(content.as_bytes())?,
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => (),
            Err(error) => return Err(error.into()),
        }
    }
    generated(
        &parent.join("sdk.ts"),
        ra2_map_mission_script::SDK,
        "/** Mission SDK v",
    )?;
    generated(
        &parent.join("map.generated.ts"),
        &catalog.typescript()?,
        "// Generated from the current map",
    )?;
    Ok(())
}
pub fn create(entry: &Path, catalog: &Catalog, player: &str) -> Result<String> {
    ensure!(!entry.exists(), "任务源码已存在，请使用打开");
    refresh_types(entry, catalog)?;
    let player = if catalog.houses.iter().any(|id| id == player) {
        player
    } else {
        catalog
            .houses
            .iter()
            .find(|s| s.as_str() != "Neutral" && s.as_str() != "Special")
            .map_or("Neutral", String::as_str)
    };
    let source = format!(
        "import {{ defineMission, event, action }} from './sdk';\nimport {{ map, House }} from './map.generated';\n\n// 游戏逻辑时间按每秒 15 帧计算，不是现实时间。\nexport default defineMission(map, {{ id: 'mission', player: House.{} }}, m => {{\n  m.rule('victory', {{\n    when: event.sinceScenarioStart(300),\n    once: true,\n    actions: [action.winPlayer()],\n  }});\n}});\n",
        catalog.enum_member("houses", player)?
    );
    use std::io::Write;
    fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(entry)?
        .write_all(source.as_bytes())?;
    Ok(source)
}

pub struct Sources {
    pub virtual_files: BTreeMap<String, String>,
    originals: BTreeMap<PathBuf, Vec<u8>>,
    root: PathBuf,
    entry: PathBuf,
    entry_module: String,
    campaign: Option<CampaignBuildContext>,
}
impl Sources {
    fn media_catalog(
        &self,
        assets: &[ra2_map_document::campaign::Asset],
        catalog: &mut Catalog,
    ) -> Result<()> {
        use vera20k_core::rules::ini_parser::IniFile;
        for asset in assets {
            let name = asset.name.to_ascii_lowercase();
            let kind = match name.as_str() {
                "soundmd.ini" => ResourceKind::Sound,
                "evamd.ini" => ResourceKind::Eva,
                "thememd.ini" => ResourceKind::Theme,
                _ => continue,
            };
            let ini =
                IniFile::from_bytes(&self.originals[&dependency(&self.root, &asset.source)?])?;
            let order = match kind {
                ResourceKind::Sound => {
                    ra2_map_assets::mission_resources::audio_names("Sound", &ini)
                }
                ResourceKind::Eva => ra2_map_assets::mission_resources::audio_names("Eva", &ini),
                _ => vera20k_core::rules::theme_ini::catalog_from_ini(&ini)
                    .into_iter()
                    .map(|t| t.key)
                    .collect(),
            };
            catalog.register(kind, order);
        }
        Ok(())
    }
    pub fn read(entry: &Path) -> Result<Self> {
        let entry = fs::canonicalize(entry)?;
        let root = entry.parent().context("任务入口缺少目录")?;
        Self::read_in(&entry, root)
    }
    pub fn read_for_document(doc: &MapDocument, entry: &Path) -> Result<Self> {
        if let Some(context) = source_context(doc, entry)? {
            let mut sources = Self::read_in(entry, Path::new(&context.root))?;
            sources.campaign = context.campaign;
            Ok(sources)
        } else {
            Self::read(entry)
        }
    }
    pub fn read_in(entry: &Path, root: &Path) -> Result<Self> {
        let entry = fs::canonicalize(entry)?;
        let root = fs::canonicalize(root)?;
        ensure!(
            entry.extension().is_some_and(|s| s == "ts"),
            "任务入口须为 .ts 文件"
        );
        let entry_module = format!(
            "/{}",
            entry
                .strip_prefix(&root)
                .context("任务入口超出项目目录")?
                .to_str()
                .context("入口路径须为 Unicode")?
                .replace('\\', "/")
        );
        let mut result = Self {
            virtual_files: BTreeMap::new(),
            originals: BTreeMap::new(),
            root: root.to_path_buf(),
            entry: entry.clone(),
            entry_module,
            campaign: None,
        };
        result.visit(&root, &root)?;
        Ok(result)
    }
    fn normalize_assets(&self, plan: &mut Mission) -> Result<()> {
        ra2_map_mission_script::native::validate_assets(&plan.assets)?;
        let prefix = self
            .entry
            .parent()
            .unwrap()
            .strip_prefix(&self.root)?
            .to_string_lossy()
            .replace('\\', "/");
        if !prefix.is_empty() {
            for asset in &mut plan.assets {
                asset.source = format!("{prefix}/{}", asset.source);
            }
        }
        Ok(())
    }
    fn restore_campaign_transition(
        &self,
        compiler: &mut Compiler,
        plan: &mut Mission,
    ) -> Result<()> {
        let Some(context) = &self.campaign else {
            return Ok(());
        };
        ensure!(
            plan.mode == ra2_map_mission_script::native::MissionMode::SinglePlayer,
            "战役关卡必须为单人任务：{}",
            context.mission
        );
        let entry = dependency(&self.root, &context.entry)?;
        let module = format!(
            "/{}",
            entry
                .strip_prefix(&self.root)?
                .to_string_lossy()
                .replace('\\', "/")
        );
        let campaign = compiler.build_campaign_at(self.virtual_files.clone(), &module)?;
        let script = campaign
            .scripts
            .get(&context.mission)
            .context("战役已移除当前关卡或脚本关联，请重新构建整个战役")?;
        ensure!(
            dependency(&self.root, script)? == self.entry,
            "战役关卡已更换脚本，请重新构建整个战役"
        );
        if let Some(transition) = campaign.transitions.get(&context.mission) {
            apply_transition(plan, transition)?;
        }
        Ok(())
    }
    fn visit(&mut self, root: &Path, directory: &Path) -> Result<()> {
        for item in fs::read_dir(directory)? {
            let item = item?;
            let ty = item.file_type()?;
            if ty.is_symlink() {
                continue;
            }
            let path = item.path();
            if ty.is_dir() {
                let name = item.file_name();
                let name = name.to_string_lossy();
                if !name.starts_with('.')
                    && !matches!(name.as_ref(), "target" | "node_modules" | "vendor")
                {
                    self.visit(root, &path)?;
                }
            } else if path.extension().is_some_and(|s| s == "ts") {
                ensure!(
                    self.originals.len() < 128 && item.metadata()?.len() <= 1024 * 1024,
                    "任务文件数量或大小超限"
                );
                let bytes = fs::read(&path)?;
                let source = String::from_utf8(bytes.clone()).context("TS 源码须为 UTF-8")?;
                let relative = path
                    .strip_prefix(root)?
                    .to_str()
                    .context("任务路径须为 Unicode")?
                    .replace('\\', "/");
                self.virtual_files.insert(format!("/{relative}"), source);
                self.originals.insert(path, bytes);
            }
        }
        ensure!(
            self.originals.values().map(Vec::len).sum::<usize>() <= 4 * 1024 * 1024,
            "任务源码总量超过 4 MiB"
        );
        Ok(())
    }
    pub fn unchanged(&self) -> Result<()> {
        for (path, bytes) in &self.originals {
            ensure!(
                fs::read(path).ok().as_ref() == Some(bytes),
                "源码 {} 已变化，请重新构建",
                path.display()
            );
        }
        Ok(())
    }
    fn context(
        &mut self,
        assets: &[ra2_map_document::campaign::Asset],
        profile: String,
    ) -> Result<BuildContext> {
        ra2_map_mission_script::native::validate_assets(assets)?;
        let mut total = 0usize;
        for asset in assets {
            let path = dependency(&self.root, &asset.source)?;
            ensure!(
                fs::metadata(&path)?.len() <= 256 * 1024 * 1024,
                "资源超过 256 MiB：{}",
                asset.source
            );
            let bytes = fs::read(&path)?;
            total += bytes.len();
            ensure!(total <= 512 * 1024 * 1024, "任务资源总量超过 512 MiB");
            self.originals.insert(path, bytes);
        }
        self.unchanged()?;
        let mut inputs = BTreeMap::new();
        for (path, bytes) in &self.originals {
            inputs.insert(
                path.strip_prefix(&self.root)?
                    .to_str()
                    .context("依赖路径须为 Unicode")?
                    .replace('\\', "/"),
                ra2_map_assets::profile::hash(bytes),
            );
        }
        // Keep declared spellings so aliases through links are verified too.
        for asset in assets {
            inputs.insert(
                asset.source.clone(),
                ra2_map_assets::profile::hash(
                    &self.originals[&dependency(&self.root, &asset.source)?],
                ),
            );
        }
        Ok(BuildContext {
            root: self.root.to_string_lossy().into_owned(),
            entry: self.entry.to_string_lossy().into_owned(),
            sdk: ra2_map_assets::profile::hash(ra2_map_mission_script::SDK.as_bytes()),
            profile,
            inputs,
            registries: BTreeMap::new(),
            campaign: self.campaign.clone(),
        })
    }
}

pub struct Built {
    pub build: Build,
    pub sources: Sources,
}
struct Job {
    document: MapDocument,
    input_revision: (u64, u64),
    catalog: Catalog,
    sources: Sources,
    reply: mpsc::Sender<Result<Built, String>>,
}
/// The compiler stays on one worker thread and is reused between builds.
pub struct Worker(mpsc::Sender<Job>);
impl Worker {
    pub fn new() -> Result<Self> {
        let (send, receive) = mpsc::channel::<Job>();
        std::thread::Builder::new()
            .name("mission-typescript".into())
            .stack_size(16 * 1024 * 1024)
            .spawn(move || {
                // Transpile the trusted SDK once. No TypeScript checker or
                // child process is initialized; jobs reuse emitted modules.
                let mut compiler = Compiler::new().ok();
                for mut job in receive {
                    let result = (|| -> Result<Built> {
                        if compiler.is_none() {
                            compiler = Some(Compiler::new()?);
                        }
                        let compiler = compiler.as_mut().unwrap();
                        let mut plan = compiler.build_at(
                            job.sources.virtual_files.clone(),
                            &job.sources.entry_module,
                        )?;
                        job.sources.normalize_assets(&mut plan)?;
                        job.sources
                            .restore_campaign_transition(compiler, &mut plan)?;
                        let context = job
                            .sources
                            .context(&plan.assets, job.catalog.profile.clone())?;
                        job.sources.media_catalog(&plan.assets, &mut job.catalog)?;
                        let mut build = prepare_with_context(
                            &job.document,
                            &plan,
                            &job.catalog,
                            Some(context),
                        )?;
                        (build.document, build.state) = job.input_revision;
                        Ok(Built {
                            build,
                            sources: job.sources,
                        })
                    })();
                    let _ = job.reply.send(result.map_err(|e| format!("{e:#}")));
                }
            })?;
        Ok(Self(send))
    }
    pub fn build(
        &self,
        document: &MapDocument,
        catalog: Catalog,
        sources: Sources,
    ) -> Result<mpsc::Receiver<Result<Built, String>>> {
        let (reply, receive) = mpsc::channel();
        self.0
            .send(Job {
                document: document.fork(),
                input_revision: (document.id, document.state_id),
                catalog,
                sources,
                reply,
            })
            .map_err(|_| anyhow::anyhow!("任务编译线程已退出"))?;
        Ok(receive)
    }
}

pub const HELP: &str = "TypeScript missions (embedded compiler; no Node.js/Deno required):
  ra2-map-editor mission-init  --game-dir DIR --map MAP --script mission.ts
  ra2-map-editor mission-build --game-dir DIR --map MAP --script mission.ts --output OUT.map
  --overwrite    Allow replacing an existing output map; input maps are never overwritten.
Import './sdk' and './map.generated'; use project-relative imports without extensions.";

pub fn run_cli(args: &[std::ffi::OsString]) -> Result<i32> {
    use std::io::Write;
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("{HELP}");
        return Ok(0);
    }
    let init = args[0] == "mission-init";
    let mut game = std::env::var_os("RA2_DIR").map(PathBuf::from);
    let mut map = None;
    let mut script = None;
    let mut output = None;
    let mut overwrite = false;
    let mut i = 1;
    while i < args.len() {
        let name = args[i].to_str().context("参数名称无效")?;
        if name == "--overwrite" {
            overwrite = true;
            i += 1;
            continue;
        }
        ensure!(
            matches!(name, "--game-dir" | "--map" | "--script" | "--output"),
            "未知参数：{name}"
        );
        i += 1;
        let value = PathBuf::from(args.get(i).context("参数缺少值")?);
        match name {
            "--game-dir" => game = Some(value),
            "--map" => map = Some(value),
            "--script" => script = Some(value),
            "--output" => output = Some(value),
            _ => unreachable!(),
        }
        i += 1;
    }
    let map = fs::canonicalize(map.context("缺少 --map")?)?;
    let script = std::path::absolute(script.context("缺少 --script")?)?;
    ensure!(
        script.extension().is_some_and(|s| s == "ts"),
        "任务入口须为 .ts 文件"
    );
    if !init {
        let out = output.as_ref().context("缺少 --output")?;
        ensure!(
            out.extension()
                .is_some_and(|s| s.eq_ignore_ascii_case("map")),
            "输出须为 .map 文件"
        );
        ensure!(
            !out.exists() || overwrite,
            "输出已存在；使用 --overwrite 允许替换"
        );
        let canonical = if out.exists() {
            fs::canonicalize(out)?
        } else {
            fs::canonicalize(
                out.parent()
                    .filter(|p| !p.as_os_str().is_empty())
                    .unwrap_or(Path::new(".")),
            )?
            .join(out.file_name().context("输出路径无效")?)
        };
        ensure!(canonical != map, "不能覆盖输入地图");
    }
    let mut install =
        ra2_map_assets::assets::Install::open(&game.context("缺少 --game-dir 或 RA2_DIR")?)?;
    let scene = ra2_map_scene::map::load_scene(
        &mut install,
        map.to_str().context("地图路径不是 Unicode")?,
    )?;
    let mut doc = MapDocument::new(
        scene
            .source
            .edit_base
            .as_ref()
            .map_err(|e| anyhow::anyhow!(e.clone()))?
            .clone(),
    );
    let catalog = catalog(&doc, &scene);
    if init {
        create(&script, &catalog, &doc.ini_text("Basic", "Player"))?;
        println!("Created {} (sdk.ts + map.generated.ts)", script.display());
    } else {
        refresh_document_types(&doc, &script, &catalog)?;
        let worker = Worker::new()?;
        let built = worker
            .build(&doc, catalog, Sources::read_for_document(&doc, &script)?)?
            .recv()
            .context("任务编译线程退出")?
            .map_err(anyhow::Error::msg)?;
        built.sources.unchanged()?;
        built.build.apply(&mut doc)?;
        let output = output.unwrap();
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(overwrite)
            .create_new(!overwrite)
            .open(&output)?;
        file.write_all(&doc.snapshot().bytes()?)?;
        println!(
            "Built {}: {} rules, {} teams, {} fields",
            output.display(),
            built.build.rule_count,
            built.build.team_count,
            built.build.field_count
        );
    }
    Ok(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ra2_map_mission_script::native::prepare;
    use std::sync::atomic::{AtomicUsize, Ordering};
    static NEXT: AtomicUsize = AtomicUsize::new(0);

    #[test]
    fn clean_install_retains_base_font_and_prefers_declared_font() {
        let temp = Temporary::new();
        let base = temp.0.join("base");
        let package = temp.0.join("package");
        fs::create_dir(&base).unwrap();
        fs::create_dir(&package).unwrap();
        fs::write(base.join("game.FNT"), b"base font").unwrap();
        fs::write(base.join("VERA.INI"), b"user settings").unwrap();
        fs::write(package.join("manifest.json"), br#"{"files_sha256":{}}"#).unwrap();
        fs::write(
            package.join("dependencies.json"),
            br#"{"passed":true,"dependencies":[]}"#,
        )
        .unwrap();
        let clean = temp.0.join("clean");
        crate::campaign::install_clean(&package, &base, &clean).unwrap();
        assert_eq!(fs::read(clean.join("game.fnt")).unwrap(), b"base font");
        assert!(!clean.join("vera.ini").exists());
        fs::write(package.join("game.fnt"), b"declared font").unwrap();
        let hash = vera20k_core::util::sha256::sha256_hex(b"declared font");
        fs::write(
            package.join("manifest.json"),
            serde_json::to_vec(&serde_json::json!({"files_sha256":{"game.fnt":hash}})).unwrap(),
        )
        .unwrap();
        let declared = temp.0.join("declared");
        crate::campaign::install_clean(&package, &base, &declared).unwrap();
        assert_eq!(
            fs::read(declared.join("game.fnt")).unwrap(),
            b"declared font"
        );
        assert_eq!(fs::read(base.join("game.FNT")).unwrap(), b"base font");
    }

    #[test]
    fn playtest_audit_uses_map_snapshot_and_returns_only_resources() {
        let fixture = ra2_map_assets::test_support::Fixture::new();
        fs::write(fixture.0.join("artmd.ini"), b"[Movies]\n").unwrap();
        let install = fixture.install();
        let plan = Compiler::new()
            .unwrap()
            .build(BTreeMap::from([(
                "/mission.ts".into(),
                r#"import {defineMission,event,action} from './sdk';
export default defineMission({houses:['Neutral']},{id:'play',player:'Neutral'},m=>{
 const text=m.defineText('hello','开始任务');
 m.rule('notice',{when:event.sinceEnabled(1),once:true,actions:[action.message(text)]});
});"#
                    .into(),
            )]))
            .unwrap();
        let mut doc = ra2_map_document::test_support::document();
        let catalog = Catalog::from_document(&doc, vec![], vec![]);
        prepare(&doc, &plan, &catalog)
            .unwrap()
            .apply(&mut doc)
            .unwrap();
        let bytes = doc.snapshot().bytes().unwrap();
        let payload = prepare_playtest_payload(&bytes, BTreeMap::new(), &install).unwrap();
        assert_eq!(
            payload.keys().map(String::as_str).collect::<Vec<_>>(),
            ["ra2md.csf"]
        );
        let csf =
            vera20k_core::assets::csf_file::CsfFile::from_bytes(&payload["ra2md.csf"]).unwrap();
        let label = declared_texts(&bytes).unwrap().into_keys().next().unwrap();
        assert_eq!(csf.get(&label), Some("开始任务"));
        // A missing reference must still fail the same dependency audit.
        let trigger =
            ra2_map_document::mission::ids(&doc, ra2_map_document::mission::Kind::Trigger)
                .into_iter()
                .next()
                .unwrap();
        doc.set_ini(
            "Actions",
            &trigger,
            Some(b"1,11,4,MISSING,0,0,0,0,A".to_vec()),
        )
        .unwrap();
        assert!(
            prepare_playtest_payload(&doc.snapshot().bytes().unwrap(), BTreeMap::new(), &install)
                .is_err()
        );
    }

    #[test]
    fn reopened_campaign_mission_keeps_root_media_and_current_transitions() {
        let temp = Temporary::new();
        let entry = temp.0.join("first/mission.ts");
        fs::create_dir(entry.parent().unwrap()).unwrap();
        fs::write(temp.0.join("shared.ts"), "export const delay=30;").unwrap();
        fs::write(entry.parent().unwrap().join("brief.wav"), b"wave").unwrap();
        fs::write(
            &entry,
            r#"import {defineMission,event,action} from '../sdk';
import {delay} from '../shared';
export default defineMission({houses:['Neutral']},{id:'nested',player:'Neutral'},m=>{
 m.asset('brief.wav','brief.wav');
 m.rule('notice',{when:event.sinceEnabled(delay),once:true,actions:[action.setCountdown(30)]});
});"#,
        )
        .unwrap();
        let campaign_source = r#"import {defineCampaign} from './sdk';
export default defineCampaign({id:'nested',entry:'first.map',missions:[
 {file:'first.map',source:'first.map',script:'first/mission.ts',next:'second.map'},
 {file:'second.map',source:'second.map',endOfGame:true}
]});"#;
        fs::write(temp.0.join("campaign.ts"), campaign_source).unwrap();
        let mut doc = ra2_map_document::test_support::document();
        let catalog = Catalog::from_document(&doc, vec![], vec![]);
        refresh_types(&temp.0.join("campaign.ts"), &catalog).unwrap();
        let mut sources = Sources::read_in(&entry, &temp.0).unwrap();
        sources.campaign = Some(CampaignBuildContext {
            entry: "campaign.ts".into(),
            mission: "first.map".into(),
        });
        let worker = Worker::new().unwrap();
        let built = worker
            .build(&doc, catalog.clone(), sources)
            .unwrap()
            .recv()
            .unwrap()
            .unwrap();
        built.build.apply(&mut doc).unwrap();
        let bytes = doc.snapshot().bytes().unwrap();
        assert_eq!(doc.ini_text("Basic", "NextScenario"), "second.map");
        assert_eq!(resource_payload(&bytes, "").unwrap()["brief.wav"], b"wave");

        let mut reopened = saved_document(&bytes).unwrap();
        refresh_document_types(&reopened, &entry, &catalog).unwrap();
        assert!(!entry.parent().unwrap().join("sdk.ts").exists());
        let sources = Sources::read_for_document(&reopened, &entry).unwrap();
        assert!(sources.virtual_files.contains_key("/shared.ts"));
        let built = worker
            .build(&reopened, catalog.clone(), sources)
            .unwrap()
            .recv()
            .unwrap()
            .unwrap();
        assert!(!built.build.apply(&mut reopened).unwrap());

        fs::write(
            temp.0.join("campaign.ts"),
            campaign_source.replace("next:'second.map'", "endOfGame:true"),
        )
        .unwrap();
        assert!(resource_payload(&bytes, "").is_err());
        let built = worker
            .build(
                &reopened,
                catalog.clone(),
                Sources::read_for_document(&reopened, &entry).unwrap(),
            )
            .unwrap()
            .recv()
            .unwrap()
            .unwrap();
        assert!(built.build.apply(&mut reopened).unwrap());
        assert_eq!(reopened.ini_text("Basic", "EndOfGame"), "yes");
        assert_eq!(reopened.ini_text("Basic", "NextScenario"), "");
        assert_eq!(reopened.ini_text("Basic", "AltNextScenario"), "");
        assert_eq!(
            resource_payload(&reopened.snapshot().bytes().unwrap(), "").unwrap()["brief.wav"],
            b"wave"
        );

        let mission_source = fs::read_to_string(&entry).unwrap();
        fs::write(
            &entry,
            mission_source.replace("id:'nested',", "id:'nested',mode:'multiplayer',"),
        )
        .unwrap();
        let failure = worker
            .build(
                &reopened,
                catalog.clone(),
                Sources::read_for_document(&reopened, &entry).unwrap(),
            )
            .unwrap()
            .recv()
            .unwrap();
        assert!(failure.err().unwrap().contains("战役关卡必须为单人任务"));
        fs::write(&entry, mission_source).unwrap();

        fs::write(
            temp.0.join("campaign.ts"),
            campaign_source.replace("script:'first/mission.ts',", ""),
        )
        .unwrap();
        let failure = worker
            .build(
                &reopened,
                catalog,
                Sources::read_for_document(&reopened, &entry).unwrap(),
            )
            .unwrap()
            .recv()
            .unwrap();
        assert!(failure.err().unwrap().contains("重新构建整个战役"));
    }

    #[test]
    fn text_assets_and_persistent_inputs_survive_save_and_refuse_stale_sources() {
        let temp = Temporary::new();
        let entry = temp.0.join("mission.ts");
        fs::write(
            &entry,
            r#"import {defineMission,event,action} from './sdk';
export default defineMission({houses:['Neutral']},{id:'localized',player:'Neutral'},m=>{
 const title=m.defineText('title','下一波增援：中文🚀\n准备');
 m.asset('brief.wav','brief.wav');
 m.rule('notice',{when:event.sinceEnabled(30),once:false,actions:[action.message(title)]});
});"#,
        )
        .unwrap();
        fs::write(temp.0.join("brief.wav"), b"wave").unwrap();
        let mut sources = Sources::read(&entry).unwrap();
        let plan = Compiler::new()
            .unwrap()
            .build(sources.virtual_files.clone())
            .unwrap();
        let mut doc = ra2_map_document::test_support::document();
        let original = doc.snapshot().bytes().unwrap();
        let catalog = Catalog::from_document(&doc, vec![], vec![]);
        let context = sources.context(&plan.assets, "profile".into()).unwrap();
        prepare_with_context(&doc, &plan, &catalog, Some(context))
            .unwrap()
            .apply(&mut doc)
            .unwrap();
        let bytes = doc.snapshot().bytes().unwrap();
        let strings = declared_texts(&bytes).unwrap();
        assert_eq!(strings.values().next().unwrap(), "下一波增援：中文🚀\n准备");
        let empty: Vec<_> = [0x43534620u32, 3, 0, 0, 0, 9]
            .into_iter()
            .flat_map(u32::to_le_bytes)
            .collect();
        let csf = crate::campaign::merge_csf(&empty, &strings).unwrap();
        let parsed = vera20k_core::assets::csf_file::CsfFile::from_bytes(&csf).unwrap();
        assert_eq!(
            parsed.get(strings.keys().next().unwrap()),
            strings.values().next().map(String::as_str)
        );
        assert_eq!(
            resource_payload(&bytes, "profile").unwrap()["brief.wav"],
            b"wave"
        );
        let altered =
            crate::campaign::patch_ini(&bytes, &[("Waypoints".into(), "1".into(), "2002".into())])
                .unwrap();
        assert!(
            resource_payload(&altered, "profile")
                .unwrap_err()
                .to_string()
                .contains("注册顺序或航点")
        );
        assert!(resource_payload(&bytes, "other").is_err());
        fs::write(temp.0.join("brief.wav"), b"changed").unwrap();
        assert!(
            resource_payload(&bytes, "profile")
                .unwrap_err()
                .to_string()
                .contains("重新构建")
        );
        assert!(sources.unchanged().is_err());
        assert!(doc.undo());
        assert_eq!(doc.snapshot().bytes().unwrap(), original);
        assert!(
            declared_texts(&doc.snapshot().bytes().unwrap())
                .unwrap()
                .is_empty()
        );
        for source in [
            "../brief.wav",
            "/brief.wav",
            "C:/brief.wav",
            "a/./brief.wav",
        ] {
            assert!(
                ra2_map_mission_script::native::validate_assets(&[
                    ra2_map_document::campaign::Asset {
                        name: "a.wav".into(),
                        source: source.into()
                    }
                ])
                .is_err()
            );
        }
    }
    struct Temporary(PathBuf);
    impl Temporary {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!(
                "yr-ts-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir_all(&path).unwrap();
            Self(path)
        }
    }
    impl Drop for Temporary {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn resource_catalog_preserves_media_indices_and_keeps_type_names_separate() {
        let mut assets = ra2_map_assets::catalog::Catalog::default();
        assets.mission_type_orders.insert(
            "weapons".into(),
            vec!["Z_DAMAGE".into(), String::new(), "A_DAMAGE".into()],
        );
        assets
            .object_types
            .insert("Structures".into(), vec!["BUILDING".into()]);
        assets
            .object_types
            .insert("Units".into(), vec!["TANK".into()]);
        assets.mission_registries.insert(
            "Countries".into(),
            vec!["ZCOUNTRY".into(), "ACOUNTRY".into()],
        );
        assets.mission_type_orders.insert(
            "voxelAnims".into(),
            vec!["Z_DEBRIS".into(), "A_DEBRIS".into()],
        );
        let mut resources = ra2_map_assets::mission_resources::MissionResources::default();
        resources.choices.insert(
            "Sound".into(),
            vec![
                ("ZFirst".into(), "First".into()),
                ("ASecond".into(), "Second".into()),
            ],
        );
        let mut catalog = Catalog::default();
        populate_resource_catalog(&mut catalog, &assets, &resources);
        assert_eq!(catalog.buildings, ["BUILDING"]);
        assert_eq!(catalog.technos, ["BUILDING", "TANK"]);
        assert!(catalog.units.is_empty());
        assert_eq!(catalog.weapons, ["A_DAMAGE", "Z_DAMAGE"]);
        assert_eq!(catalog.voxel_anims, ["A_DEBRIS", "Z_DEBRIS"]);
        assert_eq!(
            catalog
                .resource_index(
                    &ra2_map_mission_script::native::Reference {
                        kind: "voxelAnim".into(),
                        id: "A_DEBRIS".into(),
                    },
                    ResourceKind::VoxelAnim
                )
                .unwrap(),
            1
        );
        assert!(
            catalog
                .typescript()
                .unwrap()
                .contains("export enum VoxelAnim")
        );
        let weapon = ra2_map_mission_script::native::Reference {
            kind: "weapon".into(),
            id: "A_DAMAGE".into(),
        };
        assert_eq!(
            catalog
                .resource_index(&weapon, ResourceKind::Weapon)
                .unwrap(),
            2
        );
        assert_eq!(catalog.countries, ["ACOUNTRY", "ZCOUNTRY"]);
        let sound = ra2_map_mission_script::native::Reference {
            kind: "sound".into(),
            id: "ASecond".into(),
        };
        assert_eq!(
            catalog.resource_index(&sound, ResourceKind::Sound).unwrap(),
            1
        );
        let country = ra2_map_mission_script::native::Reference {
            kind: "country".into(),
            id: "ACOUNTRY".into(),
        };
        assert_eq!(
            catalog
                .resource_id(&country, ResourceKind::Country)
                .unwrap(),
            "ACOUNTRY"
        );
        assert!(
            catalog
                .resource_index(&country, ResourceKind::Country)
                .is_err()
        );
        assert!(
            catalog
                .typescript()
                .unwrap()
                .contains("export enum Building")
        );
    }

    fn scripted_runtime(source: &str, strings: &[&str]) -> vera20k_core::sim::runtime::SimRuntime {
        use vera20k_core::{
            map::map_file,
            sim::{
                runtime::{SimResources, SimRuntime},
                world::Simulation,
            },
        };
        let mut doc = ra2_map_document::test_support::document();
        let catalog =
            Catalog::from_document(&doc, vec![], strings.iter().map(|s| (*s).into()).collect());
        let plan = Compiler::new()
            .unwrap()
            .build(BTreeMap::from([
                ("/mission.ts".into(), source.into()),
                ("/map.generated.ts".into(), catalog.typescript().unwrap()),
            ]))
            .unwrap();
        prepare(&doc, &plan, &catalog)
            .unwrap()
            .apply(&mut doc)
            .unwrap();
        let map = map_file::load_from_bytes(doc.snapshot().bytes().unwrap()).unwrap();
        let mut runtime = SimRuntime {
            simulation: Simulation::with_seed(0),
            resources: SimResources::empty().into(),
        };
        runtime.simulation.initialize_trigger_definitions(
            &map,
            &Default::default(),
            None,
            None,
            None,
        );
        runtime
            .simulation
            .classify_map_triggers(&runtime.resources.rules);
        runtime
    }

    #[test]
    fn ts_random_timer_repeats_and_resamples_on_ordinary_frames() {
        use vera20k_core::sim::{
            trigger_runtime::TriggerEffect,
            world::{FrameEffects, TickLane},
        };
        let source = r#"
import {defineMission,event,action} from './sdk';
import {map,House,Text} from './map.generated';
export default defineMission(map,{id:'random_timer',player:House.Neutral},m=>{
  m.rule('clock',{when:event.randomTime(2),once:false,actions:[action.message(m.text(Text.MISSION_RANDOM))]});
});"#;
        let mut runtime = scripted_runtime(source, &["MISSION:RANDOM"]);
        let mut frames = Vec::new();
        for frame in 0..1000 {
            let output = runtime
                .advance_frame(&[], 67, TickLane::Ordinary, FrameEffects::default())
                .unwrap();
            if output.trigger_effects.iter().any(|effect| matches!(effect,TriggerEffect::LocalizedMessage {csf_label} if csf_label == "MISSION:RANDOM")) {
                frames.push(frame);
            }
        }
        assert!(frames.len() > 20, "{frames:?}");
        assert!((15..=45).contains(&frames[0]));
        let intervals: std::collections::BTreeSet<_> =
            frames.windows(2).map(|pair| pair[1] - pair[0]).collect();
        assert!(
            intervals.iter().all(|gap| (15..=45).contains(gap)),
            "{intervals:?}"
        );
        assert!(
            intervals.len() > 1,
            "timer must resample, not keep the first draw: {frames:?}"
        );
    }

    #[test]
    fn ts_repeating_any_resets_branch_timers_and_stage_enables_its_successor() {
        use vera20k_core::sim::{
            trigger_runtime::TriggerEffect,
            world::{FrameEffects, TickLane},
        };
        let source = r#"
import {defineMission,event,action} from './sdk';
import {map,House,Text} from './map.generated';
export default defineMission(map,{id:'repeat_any',player:House.Neutral},m=>{
  m.rule('repeat',{when:event.any(event.sinceEnabled(2),event.sinceEnabled(2)),once:false,
    actions:[action.message(m.text(Text.MISSION_REPEAT))]});
  m.stage('first',{enabled:true,when:event.sinceEnabled(1),actions:[action.message(m.text(Text.MISSION_FIRST))],next:m.ruleRef('second')});
  m.stage('second',{when:event.sinceEnabled(1),actions:[action.message(m.text(Text.MISSION_SECOND))]});
});"#;
        let mut runtime = scripted_runtime(
            source,
            &["MISSION:REPEAT", "MISSION:FIRST", "MISSION:SECOND"],
        );
        let mut repeated = Vec::new();
        let mut stages = Vec::new();
        for frame in 0..125 {
            let output = runtime
                .advance_frame(&[], 67, TickLane::Ordinary, FrameEffects::default())
                .unwrap();
            for effect in output.trigger_effects {
                if let TriggerEffect::LocalizedMessage { csf_label } = effect {
                    if csf_label == "MISSION:REPEAT" {
                        repeated.push(frame);
                    } else {
                        stages.push((csf_label, frame));
                    }
                }
            }
        }
        assert_eq!(repeated.len(), 4, "{repeated:?}");
        assert!((30..=31).contains(&repeated[0]));
        assert!(
            repeated
                .windows(2)
                .all(|frames| (30..=31).contains(&(frames[1] - frames[0]))),
            "{repeated:?}"
        );
        assert_eq!(
            stages,
            vec![("MISSION:FIRST".into(), 15), ("MISSION:SECOND".into(), 30)]
        );
    }

    #[test]
    fn ts_any_deduplicates_same_frame_branches_and_propagates_enable() {
        use vera20k_core::sim::{
            trigger_runtime::TriggerEffect,
            world::{FrameEffects, TickLane},
        };
        let source = r#"
import {defineMission, event, action} from './sdk';
import {map, House, Text} from './map.generated';
export default defineMission(map, {id:'branches', player:House.Neutral}, m => {
    const global = m.globalFlag(3);
    m.rule('setup', {when:event.sinceScenarioStart(0), once:true,
      actions:[action.setGlobal(global), action.enableRule(m.ruleRef('choice'))]});
    m.rule('choice', {when:event.any(event.sinceEnabled(2), event.all(event.sinceEnabled(2), event.globalSet(global))),
      once:true, enabled:false, actions:[action.message(m.text(Text.MISSION_CHOICE))]});
});"#;
        let mut runtime = scripted_runtime(source, &["MISSION:CHOICE"]);
        let mut frames = Vec::new();
        for frame in 0..150 {
            let output = runtime
                .advance_frame(&[], 67, TickLane::Ordinary, FrameEffects::default())
                .unwrap();
            for effect in output.trigger_effects {
                if let TriggerEffect::LocalizedMessage { csf_label } = effect {
                    assert_eq!(csf_label, "MISSION:CHOICE");
                    frames.push(frame);
                }
            }
        }
        assert_eq!(
            frames.len(),
            1,
            "same-frame OR branches execute once: {frames:?}"
        );
        assert!((30..=31).contains(&frames[0]));
    }

    #[test]
    fn ts_rule_lifecycle_and_global_all_conditions_run_in_ordinary_frames() {
        use vera20k_core::sim::{
            trigger_runtime::TriggerEffect,
            world::{FrameEffects, TickLane},
        };
        let source = r#"
import {defineMission, event, action} from './sdk';
import {map, House, Text} from './map.generated';
export default defineMission(map, {id:'lifecycle', player:House.Neutral}, m => {
    const shared = m.globalFlag(49), target = m.ruleRef('target'), forced = m.ruleRef('forced');
    m.rule('init', {when:event.sinceScenarioStart(0), once:true,
      actions:[action.forceRule(target), action.enableRule(target), action.setGlobal(shared), action.forceRule(forced)]});
    m.rule('stop', {when:event.sinceScenarioStart(1), once:true, actions:[action.disableRule(target)]});
    m.rule('resume', {when:event.sinceScenarioStart(3), once:true, actions:[action.enableRule(target)]});
    m.rule('target', {when:event.all(event.sinceEnabled(2), event.globalSet(shared)), once:true, enabled:false,
      actions:[action.message(m.text(Text.MISSION_TARGET)), action.clearGlobal(shared)]});
    m.rule('forced', {when:event.sinceScenarioStart(4), once:true,
      actions:[action.message(m.text(Text.MISSION_FORCED))]});
});"#;
        let mut runtime = scripted_runtime(source, &["MISSION:TARGET", "MISSION:FORCED"]);
        let mut target = Vec::new();
        let mut forced = Vec::new();
        for frame in 0..150 {
            let output = runtime
                .advance_frame(&[], 67, TickLane::Ordinary, FrameEffects::default())
                .unwrap();
            for effect in output.trigger_effects {
                if let TriggerEffect::LocalizedMessage { csf_label } = effect {
                    match csf_label.as_str() {
                        "MISSION:TARGET" => target.push(frame),
                        "MISSION:FORCED" => forced.push(frame),
                        _ => panic!("{csf_label}"),
                    }
                }
            }
        }
        assert_eq!(
            target,
            vec![75],
            "reenabling at 3s resets the independent 2s timer"
        );
        assert_eq!(
            forced,
            vec![0, 60],
            "Force22 does not consume a one-shot Tag"
        );
        assert!(!runtime.simulation.global_variables()[49]);
    }

    #[test]
    fn repeating_ts_timers_fire_every_thirty_seconds_in_ordinary_frames() {
        use vera20k_core::sim::{
            trigger_runtime::TriggerEffect,
            world::{FrameEffects, TickLane},
        };
        let source = r#"
            import {defineMission, event, action} from './sdk';
            import {map, House, Text} from './map.generated';
            export default defineMission(map, {id:'timers', player:House.Neutral}, m => {
                m.rule('repeat', {
                    when:event.sinceEnabled(30), once:false,
                    actions:[action.message(m.text(Text.MISSION_REPEAT))],
                });
                m.rule('once', {
                    when:event.sinceEnabled(30), once:true,
                    actions:[action.message(m.text(Text.MISSION_ONCE))],
                });
            });
        "#;
        let mut runtime = scripted_runtime(source, &["MISSION:ONCE", "MISSION:REPEAT"]);
        let mut repeated = Vec::new();
        let mut once = Vec::new();
        // Advance normal production frames, without manually raising events
        // or moving timers. Observe externally published message actions.
        for frame in 0..1400 {
            let output = runtime
                .advance_frame(&[], 67, TickLane::Ordinary, FrameEffects::default())
                .unwrap();
            for effect in output.trigger_effects {
                if let TriggerEffect::LocalizedMessage { csf_label } = effect {
                    match csf_label.as_str() {
                        "MISSION:REPEAT" => repeated.push(frame),
                        "MISSION:ONCE" => once.push(frame),
                        _ => panic!("unexpected message {csf_label}"),
                    }
                }
            }
        }
        assert_eq!(repeated.len(), 3, "{repeated:?}");
        assert!((449..=451).contains(&repeated[0]), "{repeated:?}");
        assert_eq!(repeated[1] - repeated[0], 450);
        assert_eq!(repeated[2] - repeated[1], 450);
        assert_eq!(once, vec![repeated[0]]);
    }

    #[test]
    fn ts_countdown_controls_preserve_title_and_restart_on_expiry() {
        use vera20k_core::sim::{
            trigger_runtime::TriggerEffect,
            world::{FrameEffects, TickLane},
        };
        let source = r#"
            import {defineMission, event, action} from './sdk';
            import {map, House, Text} from './map.generated';
            export default defineMission(map, {id:'countdown', player:House.Neutral}, m => {
                const title = m.text(Text.MISSION_TIMER);
                const reset = [action.setCountdown(30), action.setCountdownTitle(title)];
                for (const [seconds, actions] of [
                    [0, reset],
                    [1, [action.stopCountdown()]],
                    [2, [action.extendCountdown(5)]],
                    [3, [action.shortenCountdown(2)]],
                    [4, [action.stopCountdown()]],
                    [5, [action.startCountdown()]],
                ] as const) {
                    m.rule(`control_${seconds}`, {
                        when:event.sinceScenarioStart(seconds), once:true, actions,
                    });
                }
                m.rule('wave', {
                    when:event.countdownElapsed(), once:false,
                    actions:[action.message(title), ...reset],
                });
            });
        "#;
        let mut runtime = scripted_runtime(source, &["MISSION:TIMER"]);
        let mut waves = Vec::new();
        for _ in 0..1500 {
            let frame = runtime.simulation.session().binary_frame();
            let output = runtime
                .advance_frame(&[], 67, TickLane::Ordinary, FrameEffects::default())
                .unwrap();
            let view = runtime.simulation.session().scenario_timer_view();
            match frame {
                0 | 75 | 525 | 975 | 1425 => assert_eq!(view, Some((449, "MISSION:TIMER"))),
                15 | 60 => assert_eq!(view, None),
                30 => assert_eq!(view, Some((509, "MISSION:TIMER"))),
                45 => assert_eq!(view, Some((464, "MISSION:TIMER"))),
                _ => {}
            }
            for effect in output.trigger_effects {
                if let TriggerEffect::LocalizedMessage { csf_label } = effect {
                    assert_eq!(csf_label, "MISSION:TIMER");
                    waves.push(frame);
                }
            }
        }
        assert_eq!(waves, [525, 975, 1425]);
    }

    #[test]
    fn ts_countdown_expiry_fires_once_and_hides_without_restart() {
        use vera20k_core::sim::{
            trigger_runtime::TriggerEffect,
            world::{FrameEffects, TickLane},
        };
        let source = r#"
            import {defineMission, event, action} from './sdk';
            import {map, House, Text} from './map.generated';
            export default defineMission(map, {id:'expiry', player:House.Neutral}, m => {
                m.rule('start', {when:event.sinceScenarioStart(0), once:true,
                    actions:[action.setCountdown(1)]});
                m.rule('end', {when:event.countdownElapsed(), once:false,
                    actions:[action.message(m.text(Text.MISSION_END))]});
            });
        "#;
        let mut runtime = scripted_runtime(source, &["MISSION:END"]);
        let mut messages = 0;
        for _ in 0..60 {
            let frame = runtime.simulation.session().binary_frame();
            let output = runtime
                .advance_frame(&[], 67, TickLane::Ordinary, FrameEffects::default())
                .unwrap();
            for effect in output.trigger_effects {
                if let TriggerEffect::LocalizedMessage { csf_label } = effect {
                    assert_eq!(csf_label, "MISSION:END");
                    assert_eq!(frame, 15);
                    messages += 1;
                }
            }
        }
        assert_eq!(messages, 1);
        assert_eq!(runtime.simulation.session().scenario_timer_view(), None);
    }

    #[test]
    fn source_snapshot_detects_external_module_edits_and_generated_collisions() {
        let root = Temporary::new();
        let entry = root.0.join("mission.ts");
        let doc = ra2_map_document::test_support::document();
        let catalog = Catalog::from_document(&doc, vec![], vec![]);
        create(&entry, &catalog, "Neutral").unwrap();
        let config_path = root.0.join("tsconfig.json");
        let config: serde_json::Value =
            serde_json::from_slice(&fs::read(&config_path).unwrap()).unwrap();
        assert_eq!(config["compilerOptions"]["strict"], true);
        assert_eq!(config["compilerOptions"]["noUncheckedIndexedAccess"], true);
        assert_eq!(
            config["compilerOptions"]["exactOptionalPropertyTypes"],
            true
        );
        assert_eq!(config["compilerOptions"]["isolatedModules"], true);
        assert_eq!(config["compilerOptions"]["noEmit"], true);
        // Refreshing generated types must never replace an author's config.
        let custom_config = b"{\"compilerOptions\":{\"strict\":false}}";
        fs::write(&config_path, custom_config).unwrap();
        refresh_types(&entry, &catalog).unwrap();
        assert_eq!(fs::read(&config_path).unwrap(), custom_config);
        fs::create_dir(root.0.join("lib")).unwrap();
        fs::write(root.0.join("lib/waves.ts"), "export const delay = 10;").unwrap();
        let snapshot = Sources::read(&entry).unwrap();
        assert!(snapshot.virtual_files.contains_key("/lib/waves.ts"));
        snapshot.unchanged().unwrap();
        fs::write(root.0.join("lib/waves.ts"), "export const delay = 20;").unwrap();
        assert!(snapshot.unchanged().is_err());
        assert!(create(&entry, &catalog, "Neutral").is_err());
        // SDK v2 upgrades the generated v1 file without changing ownership.
        fs::write(
            root.0.join("sdk.ts"),
            "/** Mission SDK v1.\n * old generated SDK\n */\n",
        )
        .unwrap();
        refresh_types(&entry, &catalog).unwrap();
        assert_eq!(
            fs::read_to_string(root.0.join("sdk.ts")).unwrap(),
            ra2_map_mission_script::SDK
        );
        fs::write(root.0.join("sdk.ts"), "// user-owned file").unwrap();
        assert!(refresh_types(&entry, &catalog).is_err());
        assert_eq!(
            fs::read_to_string(root.0.join("sdk.ts")).unwrap(),
            "// user-owned file"
        );
    }

    #[test]
    fn cli_never_overwrites_input_map_even_with_overwrite_flag() {
        let root = Temporary::new();
        let map = root.0.join("source.map");
        let bytes = ra2_map_document::test_support::fixture();
        fs::write(&map, &bytes).unwrap();
        let args = vec![
            "mission-build".into(),
            "--map".into(),
            map.as_os_str().to_owned(),
            "--script".into(),
            root.0.join("mission.ts").into_os_string(),
            "--output".into(),
            map.as_os_str().to_owned(),
            "--overwrite".into(),
        ];
        assert!(
            run_cli(&args)
                .unwrap_err()
                .to_string()
                .contains("不能覆盖输入地图")
        );
        assert_eq!(fs::read(&map).unwrap(), bytes);
    }

    #[test]
    fn worker_keeps_original_document_identity_and_revision() {
        let root = Temporary::new();
        let mut doc = ra2_map_document::test_support::document();
        doc.set_ini("Basic", "Name", Some(b"changed".to_vec()))
            .unwrap();
        doc.commit("edit");
        let catalog = Catalog::from_document(&doc, vec![], vec![]);
        let entry = root.0.join("mission.ts");
        create(&entry, &catalog, "Neutral").unwrap();
        let worker = Worker::new().unwrap();
        let built = worker
            .build(&doc, catalog.clone(), Sources::read(&entry).unwrap())
            .unwrap()
            .recv()
            .unwrap()
            .unwrap();
        assert_eq!(
            (built.build.document, built.build.state),
            (doc.id, doc.state_id)
        );
        built.build.apply(&mut doc).unwrap();
        assert!(
            built.build.apply(&mut doc).is_err(),
            "an applied old revision is stale"
        );
        // Identical sources may reuse the mission, never a prior map Build.
        doc.set_ini("Basic", "Name", Some(b"new revision".to_vec()))
            .unwrap();
        doc.commit("manual map edit");
        let rebuilt = worker
            .build(&doc, catalog.clone(), Sources::read(&entry).unwrap())
            .unwrap()
            .recv()
            .unwrap()
            .unwrap();
        assert_eq!(
            (rebuilt.build.document, rebuilt.build.state),
            (doc.id, doc.state_id)
        );
        assert!(!rebuilt.build.apply(&mut doc).unwrap());
        assert_eq!(doc.ini_text("Basic", "Name"), "new revision");
        let trigger =
            ra2_map_document::mission::ids(&doc, ra2_map_document::mission::Kind::Trigger)[0]
                .clone();
        doc.set_ini("Events", &trigger, Some(b"1,47,0,999".to_vec()))
            .unwrap();
        doc.commit("manual generated-record edit");
        let result = worker
            .build(&doc, catalog, Sources::read(&entry).unwrap())
            .unwrap()
            .recv()
            .unwrap();
        assert!(result.err().unwrap().contains("已被手工修改"));
    }
}

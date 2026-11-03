//! Prepare real generated types for README snippets and build complete examples.
use anyhow::{Context, Result, ensure};
use ra2_map_assets::assets::Install;
use ra2_map_document::{
    document::{Baseline, MapDocument},
    mission::{self, Kind},
    new_map::NewMapOptions,
};
use ra2_map_io::{
    campaign,
    mission_script::{self, Sources, Worker},
};
use serde_json::json;
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

const IMPORTS: &str = r#"import {defineMission,defineCampaign,event,action,order,BasePlanControl,AiTriggerCondition,AiTriggerComparison,Difficulty,MissionMode,TagRepeat,Crate,BuildingSelection,Quarry,UnloadMode,UnitMission,HouseEdge} from './sdk';
import {map,House,Unit,Waypoint,MapObject,Text,Country,HouseColor,Building,Movie,Weapon,Animation,SuperWeapon,Techno} from './map.generated';
"#;

fn isolated_install(game: &Path, root: &Path) -> Result<()> {
    std::fs::create_dir(root)?;
    for entry in std::fs::read_dir(game)? {
        let entry = entry?;
        if !entry.file_type()?.is_file() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().to_ascii_lowercase();
        let extension = entry
            .path()
            .extension()
            .map(|v| v.to_string_lossy().to_ascii_lowercase())
            .unwrap_or_default();
        if !matches!(
            extension.as_str(),
            "mix"
                | "ini"
                | "csf"
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
                | "fnt"
                | "idx"
                | "bag"
        ) {
            continue;
        }
        if extension == "mix" {
            std::fs::hard_link(entry.path(), root.join(name)).or_else(|_| {
                std::fs::copy(entry.path(), root.join(entry.file_name())).map(|_| ())
            })?;
        } else {
            std::fs::copy(entry.path(), root.join(name))?;
        }
    }
    let install = Install::open(game)?;
    let base = install.manager.get_ref("ra2md.csf").context("base CSF")?;
    let strings = BTreeMap::from([
        ("MISSION:INTRO".into(), "示例开场信息".into()),
        ("MISSION:TIMER".into(), "下一波增援".into()),
        ("MISSION:TEXT".into(), "示例目标信息".into()),
    ]);
    std::fs::write(root.join("ra2md.csf"), campaign::merge_csf(base, &strings)?)?;
    Ok(())
}

fn context(index: usize) -> &'static str {
    match index {
        3 => {
            "const entry=m.waypoint(Waypoint.WP20);const assault=m.team('assault',{house:m.house(House.Russians),waypoint:entry,units:[{type:m.unit(Unit.HTNK),count:3}],orders:[order.hunt()]});"
        }
        11 => {
            "const target=m.object(MapObject.Structures_0),ready=m.globalFlag(0),done=m.globalFlag(1);"
        }
        13 => "const target=m.object(MapObject.Structures_0),alarm=m.flag('alarm');",
        14 => "const target=m.object(MapObject.Structures_0);",
        19 | 20 => "const at=m.waypoint(Waypoint.WP20);",
        25 => "const targetWaypoint=m.waypoint(Waypoint.WP20);",
        26 => "const homeWaypoint=m.waypoint(Waypoint.WP20);",
        30 => "const start=m.waypoint(Waypoint.WP20);",
        31 => "const landing=m.waypoint(Waypoint.WP20),objective=m.waypoint(Waypoint.WP21);",
        42 => "const ready=m.globalFlag(0);",
        _ => "",
    }
}

fn main() -> Result<()> {
    let mut args = std::env::args_os().skip(1);
    let game = std::path::absolute(PathBuf::from(args.next().context("game directory")?))?;
    let root = std::path::absolute(PathBuf::from(args.next().context("new output directory")?))?;
    ensure!(
        !root.exists() && args.next().is_none(),
        "output must be new"
    );
    std::fs::create_dir_all(&root)?;
    let profile = root.join("install");
    isolated_install(&game, &profile)?;
    let mut install = Install::open(&profile)?;
    let bytes = NewMapOptions {
        width: 32,
        height: 64,
        ..Default::default()
    }
    .bytes()?;
    let map = vera20k_core::map::map_file::load_from_bytes(bytes.clone())?;
    let mut base = MapDocument::new(Baseline::parse(bytes.into(), &map)?);
    for house in ["Americans", "Russians"] {
        mission::create(&mut base, Kind::House, house, house, None)?;
        base.set_ini(house, "Country", Some(house.as_bytes().to_vec()))?;
    }
    base.set_ini("Basic", "Player", Some(b"Americans".to_vec()))?;
    base.set_ini("Basic", "MultiplayerOnly", Some(b"no".to_vec()))?;
    base.add_record("Structures", [70, 70], "Americans", "GAPOWR")?;
    base.add_record("Structures", [78, 70], "Russians", "NAPOWR")?;
    base.add_record("Infantry", [60, 60], "Americans", "ENGINEER")?;
    for (id, cell) in [(20, [40, 40]), (21, [45, 45]), (22, [50, 50])] {
        base.waypoint(id, cell)?;
    }
    mission::create(&mut base, Kind::TaskForce, "MANUAL_FORCE", "Russians", None)?;
    base.set_ini("MANUAL_FORCE", "0", Some(b"1,E1".to_vec()))?;
    mission::create(&mut base, Kind::Script, "MANUAL_SCRIPT", "Russians", None)?;
    base.set_ini("MANUAL_SCRIPT", "0", Some(b"5,60".to_vec()))?;
    base.commit("README prerequisites");
    let base_path = root.join("base.map");
    std::fs::write(&base_path, base.snapshot().bytes()?)?;
    let scene =
        ra2_map_scene::map::load_scene(&mut install, base_path.to_str().context("base map path")?)?;
    let catalog = mission_script::catalog(&base, &scene);
    let readme = std::fs::read_to_string("examples/missions/README.md")?;
    let mut blocks = Vec::new();
    let mut current = None;
    for (line, text) in readme.lines().enumerate() {
        if text == "```ts" {
            ensure!(current.is_none(), "nested TS fence");
            current = Some((line + 1, String::new()));
        } else if text == "```" {
            if let Some(block) = current.take() {
                blocks.push(block);
            }
        } else if let Some((_, body)) = &mut current {
            body.push_str(text);
            body.push('\n');
        }
    }
    ensure!(
        blocks.len() == 44 && current.is_none(),
        "README block inventory changed; review prerequisite contexts"
    );
    let worker = Worker::new()?;
    let mut entries = Vec::new();
    let mut negative_controls = Vec::new();
    for (index, (line, body)) in blocks.iter().enumerate() {
        let folder = root.join(format!("block-{index:02}"));
        std::fs::create_dir(&folder)?;
        let stripped = body
            .lines()
            .filter(|line| !line.starts_with("import "))
            .collect::<Vec<_>>()
            .join("\n");
        let complete = stripped.contains("export default");
        let source = if complete {
            body.clone()
        } else if index == 28 {
            format!(
                "{IMPORTS}export default defineMission(map,{{id:'readme_{index}',player:House.Americans}},m=>{{m.team('media',{{house:m.house(House.Americans),waypoint:m.waypoint(Waypoint.WP20),units:[{{type:m.unit(Unit.E1),count:1}}],{stripped}}});}});\n"
            )
        } else {
            format!(
                "{IMPORTS}export default defineMission(map,{{id:'readme_{index}',player:House.Americans}},m=>{{\n{}\n{stripped}\n}});\n",
                context(index)
            )
        };
        let entry = folder.join("mission.ts");
        std::fs::write(&entry, source)?;
        std::fs::write(folder.join("readme-block.ts.txt"), body)?;
        mission_script::refresh_types(&entry, &catalog)?;
        let mut checks = BTreeMap::new();
        let mut error = None;
        if complete && index != 23 {
            let mut doc = base.fork();
            let before = doc.snapshot().bytes()?;
            match worker
                .build(&doc, catalog.clone(), Sources::read(&entry)?)?
                .recv()?
                .map_err(anyhow::Error::msg)
            {
                Ok(build) => {
                    build.build.apply(&mut doc)?;
                    let after = doc.snapshot().bytes()?;
                    std::fs::write(folder.join("mission.map"), &after)?;
                    checks.insert(
                        "unchanged_rebuild",
                        !worker
                            .build(&doc, catalog.clone(), Sources::read(&entry)?)?
                            .recv()?
                            .map_err(anyhow::Error::msg)?
                            .build
                            .apply(&mut doc)?,
                    );
                    checks.insert(
                        "undo_exact",
                        doc.undo() && doc.snapshot().bytes()? == before,
                    );
                    checks.insert("redo_exact", doc.redo() && doc.snapshot().bytes()? == after);
                    checks.insert(
                        "manual_definitions_preserved",
                        doc.ini_fields("MANUAL_SCRIPT") == base.ini_fields("MANUAL_SCRIPT")
                            && doc.ini_fields("MANUAL_FORCE") == base.ini_fields("MANUAL_FORCE"),
                    );
                }
                Err(failure) => {
                    error = Some(failure.to_string());
                    checks.insert("native_build", false);
                }
            }
        }
        if index == 9 {
            let negative_folder = root.join("shared-country-control");
            std::fs::create_dir(&negative_folder)?;
            let negative_entry = negative_folder.join("mission.ts");
            let negative_source =
                std::fs::read_to_string(&entry)?.replace("Country.Alliance", "Country.Russians");
            std::fs::write(&negative_entry, negative_source)?;
            mission_script::refresh_types(&negative_entry, &catalog)?;
            let original = base.snapshot().bytes()?;
            let failure = worker
                .build(&base, catalog.clone(), Sources::read(&negative_entry)?)?
                .recv()?
                .err();
            let checks = BTreeMap::from([
                (
                    "shared_country_rejected_with_reason",
                    failure.as_ref().is_some_and(|v| v.contains("第一个 House")),
                ),
                (
                    "failed_build_keeps_map_exact",
                    base.snapshot().bytes()? == original,
                ),
            ]);
            negative_controls
                .push(json!({"passed":checks.values().all(|v|*v),"checks":checks,"error":failure}));
        }
        entries.push(json!({"block":index,"readme_line":line,"complete":complete,"campaign":index==23,"checks":checks,"error":error,"added_context":context(index),"source":entry}));
    }
    let campaign_root = root.join("campaign");
    std::fs::create_dir(&campaign_root)?;
    std::fs::create_dir(campaign_root.join("maps"))?;
    for (name, source) in [
        ("first", "examples/missions/two-stage/first.ts"),
        ("second", "examples/missions/two-stage/second.ts"),
    ] {
        let folder = campaign_root.join(name);
        std::fs::create_dir(&folder)?;
        let entry = folder.join("mission.ts");
        std::fs::copy(source, &entry)?;
        mission_script::refresh_types(&entry, &catalog)?;
        std::fs::write(
            campaign_root.join(format!("maps/{name}.map")),
            base.snapshot().bytes()?,
        )?;
    }
    let campaign_entry = campaign_root.join("campaign.ts");
    std::fs::write(&campaign_entry, &blocks[23].1)?;
    mission_script::refresh_types(&campaign_entry, &catalog)?;
    let prepared = mission_script::prepare_campaign(&campaign_entry, &profile)?;
    let published = campaign::export(prepared, &root.join("published"))?;
    ensure!(published.passed, "published campaign dependency audit");
    std::fs::write(
        root.join("campaign-report.json"),
        serde_json::to_vec_pretty(&published)?,
    )?;
    let passed = negative_controls
        .iter()
        .all(|value| value["passed"] == true)
        && entries.iter().all(|entry| {
            entry["error"].is_null()
                && entry["checks"]
                    .as_object()
                    .unwrap()
                    .values()
                    .all(|v| v == true)
        });
    std::fs::write(
        root.join("report.json"),
        serde_json::to_vec_pretty(
            &json!({"passed":passed,"entries":entries,"negative_controls":negative_controls,"campaign_published":true,"scope":"44 README TS blocks prepared against an actual generated catalog, with explicit preceding context for fragments. Five complete missions use Worker build, exact rebuild/undo/redo and manual preservation. README campaign published with checked-in two-stage mission sources and real map/resource prerequisites. Typechecking is a separate external verification step; no claim that action fragments were executed or all examples run in the client."}),
        )?,
    )?;
    ensure!(passed, "README native builds failed; see report.json");
    println!("{}", root.join("report.json").display());
    Ok(())
}

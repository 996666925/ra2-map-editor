//! Mission source IO. Script evaluation never receives filesystem access.
use anyhow::{Context, Result, ensure};
use ra2_map_document::document::MapDocument;
use ra2_map_mission_script::{
    native::{Build, Catalog, prepare},
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
        "/** Mission SDK v1.",
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
}
impl Sources {
    pub fn read(entry: &Path) -> Result<Self> {
        let entry = fs::canonicalize(entry)?;
        ensure!(
            entry.extension().is_some_and(|s| s == "ts"),
            "任务入口须为 .ts 文件"
        );
        let root = entry.parent().context("任务入口缺少目录")?;
        let mut result = Self {
            virtual_files: BTreeMap::new(),
            originals: BTreeMap::new(),
        };
        result.visit(root, root)?;
        let source = fs::read_to_string(&entry)?;
        result.virtual_files.insert("/mission.ts".into(), source);
        Ok(result)
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
                for job in receive {
                    let result = (|| -> Result<Built> {
                        if compiler.is_none() {
                            compiler = Some(Compiler::new()?);
                        }
                        let plan = compiler
                            .as_mut()
                            .unwrap()
                            .build(job.sources.virtual_files.clone())?;
                        let mut build = prepare(&job.document, &plan, &job.catalog)?;
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
        refresh_types(&script, &catalog)?;
        let worker = Worker::new()?;
        let built = worker
            .build(&doc, catalog, Sources::read(&script)?)?
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
    use std::sync::atomic::{AtomicUsize, Ordering};
    static NEXT: AtomicUsize = AtomicUsize::new(0);
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

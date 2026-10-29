//! Portable mission projects, dependency resolution and atomic directory export.
use anyhow::{Context, Result, ensure};
use ra2_map_assets::assets::Install;
use ra2_map_document::{
    campaign::{Project, destination, filename},
    document::{Baseline, MapDocument},
    mission::{self, Kind},
    raw_ini::RawIni,
};
use serde::Serialize;
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::Path,
};
use vera20k_core::{
    assets::csf_file::CsfFile,
    rules::{
        ini_parser::IniFile,
        sound_ini::{EvaRegistry, SoundRegistry},
    },
};
#[cfg(test)]
#[path = "campaign_tests.rs"]
mod tests;

#[derive(Clone, Debug, Serialize)]
pub struct Dependency {
    pub owner: String,
    pub kind: String,
    pub name: String,
    pub source: String,
    pub resolved: bool,
}
#[derive(Clone, Debug, Serialize)]
pub struct Report {
    pub passed: bool,
    pub entry: String,
    pub shell_campaign: String,
    pub dependencies: Vec<Dependency>,
    pub warnings: Vec<String>,
    pub files: Vec<String>,
}
pub fn read_project(path: &Path) -> Result<Project> {
    ensure!(fs::metadata(path)?.len() < 4 * 1024 * 1024, "项目文件过大");
    let project: Project = serde_json::from_slice(&fs::read(path)?)?;
    project.validate()?;
    Ok(project)
}
pub fn save_project(path: &Path, project: &Project) -> Result<()> {
    project.validate()?;
    let bytes = serde_json::to_vec_pretty(project)?;
    let temp = path.with_extension(format!("{}.tmp", std::process::id()));
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temp)?;
    use std::io::Write;
    file.write_all(&bytes)?;
    file.sync_all()?;
    drop(file);
    fs::rename(&temp, path).inspect_err(|_| {
        let _ = fs::remove_file(&temp);
    })?;
    Ok(())
}
pub fn patch_ini(bytes: &[u8], values: &[(String, String, String)]) -> Result<Vec<u8>> {
    let raw = RawIni::parse(bytes.to_vec().into())?;
    let mut changes = BTreeMap::new();
    let mut append: BTreeMap<String, Vec<(String, Vec<u8>)>> = BTreeMap::new();
    for (s, k, value) in values {
        ensure!(!value.contains(['\r', '\n', ';']), "INI 值含控制字符");
        let rows: Vec<_> = raw
            .entries(s)
            .filter(|(_, l)| l.key.as_deref() == Some(k))
            .collect();
        ensure!(rows.len() <= 1, "重复元数据字段 {s}/{k}");
        if let Some((line, _)) = rows.first() {
            changes.insert(*line, Some(value.as_bytes().to_vec()));
        } else {
            append
                .entry(s.clone())
                .or_default()
                .push((k.clone(), value.as_bytes().to_vec()));
        }
    }
    Ok(raw.patch(&changes, &BTreeMap::new(), &append, false))
}
/// Preserve every base CSF record including WRTS extra data; append overrides.
pub fn merge_csf(base: &[u8], strings: &BTreeMap<String, String>) -> Result<Vec<u8>> {
    CsfFile::from_bytes(base)?;
    let mut cursor = 24usize;
    fn word(bytes: &[u8], at: &mut usize) -> Result<u32> {
        let v = u32::from_le_bytes(bytes.get(*at..*at + 4).context("CSF 截断")?.try_into()?);
        *at += 4;
        Ok(v)
    }
    while cursor < base.len() {
        if base[cursor..].iter().all(|b| *b == 0) {
            break;
        }
        ensure!(word(base, &mut cursor)? == 0x4c424c20, "CSF 记录标记无效");
        let pairs = word(base, &mut cursor)?;
        let len = word(base, &mut cursor)? as usize;
        cursor = cursor
            .checked_add(len)
            .filter(|n| *n <= base.len())
            .context("CSF 标签截断")?;
        for _ in 0..pairs {
            let magic = word(base, &mut cursor)?;
            ensure!(
                matches!(magic, 0x53545220 | 0x53545257),
                "CSF 字符串标记无效"
            );
            let len = word(base, &mut cursor)? as usize;
            cursor = cursor
                .checked_add(len.checked_mul(2).context("CSF 长度溢出")?)
                .filter(|n| *n <= base.len())
                .context("CSF 文本截断")?;
            if magic == 0x53545257 {
                let len = word(base, &mut cursor)? as usize;
                cursor = cursor
                    .checked_add(len)
                    .filter(|n| *n <= base.len())
                    .context("CSF 扩展截断")?;
            }
        }
    }
    let mut out = base[..cursor].to_vec();
    for (key, text) in strings {
        ensure!(
            !key.is_empty()
                && key.len() <= 31
                && key.is_ascii()
                && !key.contains('\0')
                && !text.contains('\0'),
            "CSF 标签/文本无效"
        );
        let units: Vec<_> = text.encode_utf16().collect();
        for n in [0x4c424c20, 1, key.len() as u32] {
            out.extend(n.to_le_bytes());
        }
        out.extend(key.as_bytes());
        for n in [0x53545220, units.len() as u32] {
            out.extend(n.to_le_bytes());
        }
        for u in units {
            out.extend((!u).to_le_bytes());
        }
    }
    for at in [8, 12] {
        let n = u32::from_le_bytes(base[at..at + 4].try_into()?)
            .checked_add(strings.len().try_into()?)
            .context("CSF 数量溢出")?;
        out[at..at + 4].copy_from_slice(&n.to_le_bytes());
    }
    let parsed = CsfFile::from_bytes(&out)?;
    for (key, text) in strings {
        ensure!(parsed.get(key) == Some(text.as_str()), "CSF 回读失败");
    }
    Ok(out)
}

pub struct Prepared {
    pub payload: BTreeMap<String, Vec<u8>>,
    pub report: Report,
}
fn read_bounded(path: &Path) -> Result<Vec<u8>> {
    ensure!(
        fs::metadata(path)?.len() <= 256 * 1024 * 1024,
        "资源超过 256 MiB：{}",
        path.display()
    );
    Ok(fs::read(path)?)
}
pub fn prepare(
    project: &Project,
    root: &Path,
    directory: &Path,
    current: Option<(&str, &[u8])>,
) -> Result<Prepared> {
    project.validate()?;
    // A new campaign starts at MAPSEL stage zero, regardless of UI list order.
    let mut ordered = project.clone();
    let entry = ordered
        .missions
        .iter()
        .position(|m| m.file.eq_ignore_ascii_case(&ordered.entry))
        .unwrap();
    let first = ordered.missions.remove(entry);
    ordered.missions.insert(0, first);
    let project = &ordered;
    let install = Install::open(directory)?;
    let mut payload = BTreeMap::new();
    for a in &project.assets {
        payload.insert(
            a.name.to_ascii_lowercase(),
            read_bounded(&root.join(&a.source))?,
        );
    }
    let stock = |name: &str| install.manager.get_ref(name).unwrap_or_default();
    if !project.strings.is_empty() {
        payload.insert(
            "ra2md.csf".into(),
            merge_csf(stock("ra2md.csf"), &project.strings)?,
        );
    }
    let mut mission_values = Vec::new();
    let mut maps = Vec::new();
    for m in &project.missions {
        let bytes = if let Some((name, bytes)) =
            current.filter(|(name, _)| name.eq_ignore_ascii_case(&m.file))
        {
            let _ = name;
            bytes.to_vec()
        } else {
            read_bounded(&root.join(&m.source))?
        };
        let map = crate::editor_io::validate(&bytes)?;
        ensure!(
            !map.ini
                .section_or_empty("Basic")
                .read_bool("MultiplayerOnly", false),
            "{} 仍标为多人地图",
            m.file
        );
        let roster = vera20k_core::map::houses::parse_house_roster(&map.ini, &[], None);
        let player = map
            .ini
            .section_or_empty("Basic")
            .read_string("Player", "", 64);
        ensure!(
            roster.houses.iter().any(|h| h.name == player),
            "{} 的 Player 未注册",
            m.file
        );
        for (key, value) in [
            ("UIName", &m.ui_name),
            ("Briefing", &m.briefing),
            ("LSLoadMessage", &m.load_message),
            ("LSLoadBriefing", &m.load_briefing),
            ("LS640BkgdName", &m.background_640),
            ("LS800BkgdName", &m.background_800),
            ("LS800BkgdPal", &m.background_palette),
        ] {
            // Empty project values must clear metadata inherited from stock maps.
            mission_values.push((m.file.to_ascii_uppercase(), key.into(), value.clone()));
        }
        payload.insert(m.file.to_ascii_lowercase(), bytes);
        maps.push(map);
    }
    let shell_campaign = if project
        .missions
        .iter()
        .find(|m| m.file.eq_ignore_ascii_case(&project.entry))
        .unwrap()
        .side
        == "GDI"
    {
        "all1"
    } else {
        "sov1"
    };
    let mut battle = format!(
        "[Battles]\n0={}\n[{}]\nScenario={}\nCD=-1\n",
        project.id,
        project.id,
        project.entry.to_ascii_uppercase()
    );
    if !project.id.eq_ignore_ascii_case(shell_campaign) {
        battle.push_str(&format!(
            "[{shell_campaign}]\nScenario={}\nCD=-1\n",
            project.entry.to_ascii_uppercase()
        ));
        battle = battle.replacen(
            "[Battles]\n",
            &format!("[Battles]\n1={shell_campaign}\n"),
            1,
        );
    }
    payload.insert("battlemd.ini".into(), battle.into_bytes());
    payload.insert(
        "missionmd.ini".into(),
        patch_ini(stock("missionmd.ini"), &mission_values)?,
    );
    let mut mapsel = String::new();
    for side in ["GDI", "Nod", "ThirdSide"] {
        mapsel.push_str(&format!("[{side}]\n"));
        for (i, _) in project.missions.iter().enumerate() {
            mapsel.push_str(&format!("{}=EDSTAGE{i}\n", i + 1));
        }
    }
    let mut warnings = Vec::new();
    for (i, (m, map)) in project.missions.iter().zip(maps.iter()).enumerate() {
        mapsel.push_str(&format!(
            "[EDSTAGE{i}]\nScenario={}\n",
            m.file.to_ascii_uppercase()
        ));
        let terminal = map.basic.end_of_game == Some(true) || map.basic.one_time_only == Some(true);
        if !terminal {
            ensure!(
                map.basic.skip_map_select == Some(true),
                "{} 续关须启用 SkipMapSelect；当前发布器生成直接分支目录",
                m.file
            );
            let mut edges = BTreeSet::new();
            for branch in [false, true] {
                let next = destination(&map.basic, branch)
                    .filter(|s| !s.is_empty())
                    .with_context(|| {
                        format!(
                            "{} 缺少 {}",
                            m.file,
                            if branch {
                                "AltNextScenario"
                            } else {
                                "NextScenario"
                            }
                        )
                    })?;
                let index = project
                    .missions
                    .iter()
                    .position(|m| m.file.eq_ignore_ascii_case(next))
                    .with_context(|| format!("关卡 {} 引用未打包的 {next}", m.file))?;
                if edges.insert(index) {
                    mapsel.push_str(&format!("{}=EDSTAGE{index}\n", edges.len() - 1));
                }
            }
        }
        if map.basic.one_time_only == Some(true) {
            warnings.push(format!("{} OneTimeOnly 会返回主界面，不进入下一关", m.file));
        }
    }
    payload.insert("mapselmd.ini".into(), mapsel.into_bytes());
    let mut report = audit_resources(project, &maps, &payload, &install)?;
    report.warnings.splice(0..0, warnings);
    report.shell_campaign = shell_campaign.into();
    Ok(Prepared { payload, report })
}

/// Shared dependency audit for CLI and package publishing. No export or rewrite.
pub fn audit_resources(
    project: &Project,
    maps: &[vera20k_core::map::map_file::MapFile],
    payload: &BTreeMap<String, Vec<u8>>,
    install: &Install,
) -> Result<Report> {
    let mut warnings = Vec::new();
    let lookup = |name: &str| {
        payload
            .get(&name.to_ascii_lowercase())
            .map(Vec::as_slice)
            .or_else(|| install.manager.get_ref(name))
    };
    let table = CsfFile::load_active(lookup)?.0;
    let sounds = SoundRegistry::from_ini(&IniFile::from_bytes(
        lookup("soundmd.ini").unwrap_or_default(),
    )?);
    let eva = EvaRegistry::from_ini(&IniFile::from_bytes(
        lookup("evamd.ini").unwrap_or_default(),
    )?);
    let mut themes = vera20k_core::rules::theme_ini::catalog_from_ini(&IniFile::from_bytes(
        lookup("thememd.ini").unwrap_or_default(),
    )?);
    for entry in vera20k_core::rules::theme_ini::catalog_from_ini(&IniFile::from_bytes(
        lookup("theme.ini").unwrap_or_default(),
    )?) {
        if entry.normal
            && lookup(&format!("{}.wav", entry.sound)).is_some()
            && !themes
                .iter()
                .any(|t| t.key.eq_ignore_ascii_case(&entry.key))
        {
            themes.push(entry);
        }
    }
    let art = IniFile::from_bytes(lookup("artmd.ini").context("缺少 artmd.ini")?)?;
    let movies: Vec<_> = art
        .section("Movies")
        .map(|s| s.raw_entries().map(|(_, v)| v.to_string()).collect())
        .unwrap_or_default();
    let custom_audio = payload.contains_key("audio.idx") || payload.contains_key("audio.bag");
    let audio = if custom_audio {
        Some(
            vera20k_core::assets::audio_bag::AudioIndex::from_idx_bag(
                lookup("audio.idx").context("缺少 audio.idx")?,
                lookup("audio.bag").context("缺少 audio.bag")?.to_vec(),
            )
            .context("自定义音频索引无效")?,
        )
    } else {
        install.manager.load_audio_index()?.map(|a| a.index)
    };
    let mut dependencies = Vec::new();
    let mut invalid_parameters = false;
    let ordinal_name = |kind: &str, value: Option<i32>| {
        let name = value.and_then(|index| match kind {
            "Sound" => sounds.entry_by_index(index).map(|e| e.id.clone()),
            "Eva" => eva.entry_by_index(index).map(|e| e.name.clone()),
            "Theme" => usize::try_from(index)
                .ok()
                .and_then(|i| themes.get(i))
                .map(|e| e.key.clone()),
            "Movie" => usize::try_from(index)
                .ok()
                .and_then(|i| movies.get(i))
                .cloned(),
            _ => None,
        });
        name.unwrap_or_else(|| format!("<invalid ordinal {value:?}>"))
    };
    let mut dep = |owner: &str, kind: &str, name: &str, resolved: bool, source: String| {
        dependencies.push(Dependency {
            owner: owner.into(),
            kind: kind.into(),
            name: name.into(),
            resolved,
            source,
        });
    };
    for (m, map) in project.missions.iter().zip(maps.iter()) {
        let mut refs: Vec<(String, String)> = Vec::new();
        for key in [
            "Intro",
            "Brief",
            "Action",
            "Win",
            "Lose",
            "PostScore",
            "PreMapSelect",
        ] {
            let s = map.ini.section_or_empty("Basic").read_string(key, "", 128);
            if !s.is_empty() && !s.eq_ignore_ascii_case("<none>") {
                refs.push(("Movie".into(), s));
            }
        }
        if let Some(s) = map
            .basic
            .theme
            .as_ref()
            .filter(|s| !s.eq_ignore_ascii_case("<none>"))
        {
            refs.push(("Theme".into(), s.clone()));
        }
        for key in [&m.ui_name, &m.briefing, &m.load_message, &m.load_briefing] {
            if !key.is_empty() {
                refs.push(("Csf".into(), key.clone()));
            }
        }
        for name in [&m.background_640, &m.background_800, &m.background_palette] {
            if !name.is_empty() {
                refs.push(("File".into(), name.clone()));
            }
        }
        let doc = MapDocument::new(Baseline::parse(
            payload[&m.file.to_ascii_lowercase()].clone().into(),
            map,
        )?);
        for id in mission::ids(&doc, Kind::Trigger) {
            match mission::decode_list(&doc.ini_text("Actions", &id), false) {
                Ok(rows) => {
                    for row in rows {
                        let kind = match row[1].as_str() {
                            "4" => Some("Csf"),
                            "6" => Some("Eva"),
                            "7" => Some("Sound"),
                            "8" => Some("Theme"),
                            _ => None,
                        };
                        if let Some(kind) = kind {
                            refs.push((kind.into(), row[2].clone()));
                        }
                        let consumed = match row[0].as_str() {
                            "19" | "98" | "99" => Some("Sound"),
                            "20" => Some("Theme"),
                            "21" => Some("Eva"),
                            "10" | "100" | "117" => Some("Movie"),
                            _ => None,
                        };
                        if let Some(consumed) = consumed.filter(|k| Some(*k) != kind) {
                            let entry = vera20k_core::map::actions::ActionEntry {
                                kind: row[0].parse().unwrap_or(-1),
                                params: row[1..].to_vec(),
                                waypoint_index: None,
                            };
                            let value = match row[1].as_str() {
                                "6" => eva.index_of(&row[2]),
                                "7" => sounds.index_of(&row[2]),
                                "8" => themes
                                    .iter()
                                    .position(|t| t.key.eq_ignore_ascii_case(&row[2]))
                                    .map(|i| i as i32),
                                _ => entry.literal_value(),
                            };
                            let literal = match row[1].as_str() {
                                "0" | "11" => Some(&row[2]),
                                "5" | "9" => Some(&row[7]),
                                _ => None,
                            };
                            if literal.is_some_and(|v| v.parse::<i32>().is_err()) {
                                invalid_parameters = true;
                                warnings.push(format!("{}/Actions/{id}: 动作 {} 的数值参数包含名称，请使用对应资源参数类型", m.file, row[0]));
                            }
                            refs.push((consumed.into(), ordinal_name(consumed, value)));
                        }
                        if mission::schema::opcode(false, row[0].parse().unwrap_or(-1)).is_none() {
                            warnings.push(format!(
                                "{}/Actions/{id}: 未具名动作 {} 可能含额外资源依赖",
                                m.file, row[0]
                            ));
                        }
                    }
                }
                Err(e) => warnings.push(format!("{}/Actions/{id}: {e}", m.file)),
            }
        }
        for id in mission::ids(&doc, Kind::Script) {
            for (key, value) in mission::fields(&doc, Kind::Script, &id) {
                if key.parse::<usize>().is_err() {
                    continue;
                }
                let row = mission::csv(&String::from_utf8_lossy(&value));
                if row.len() != 2 {
                    warnings.push(format!("{}/{id}/{key}: 无法解析脚本资源引用", m.file));
                    continue;
                }
                let kind = match row[0].as_str() {
                    "25" => Some("Sound"),
                    "26" => Some("Movie"),
                    "27" => Some("Theme"),
                    _ => None,
                };
                if let Some(kind) = kind {
                    refs.push((kind.into(), ordinal_name(kind, row[1].parse().ok())));
                }
            }
        }
        refs.sort();
        refs.dedup();
        for (kind, name) in refs {
            let mut files = Vec::new();
            let found = match kind.as_str() {
                "Csf" => table.get(&name).is_some(),
                "Sound" => {
                    let entry = sounds.get(&name);
                    if let Some(entry) = entry {
                        files.extend(entry.sounds.iter().map(|s| (s.clone(), true)));
                    }
                    entry.is_some_and(|entry| {
                        !entry.sounds.is_empty()
                            && audio.as_ref().is_none_or(|a| {
                                entry
                                    .sounds
                                    .iter()
                                    .all(|s| a.find_sample_index(s).is_some())
                            })
                    })
                }
                "Eva" => {
                    let entry = eva.entry(&name);
                    if let Some(entry) = entry {
                        files.extend(
                            [&entry.allied, &entry.russian, &entry.yuri]
                                .into_iter()
                                .filter_map(|s| s.as_ref().map(|s| (s.clone(), true))),
                        );
                    }
                    entry.is_some()
                }
                "Theme" => {
                    let entry = themes.iter().find(|t| t.key.eq_ignore_ascii_case(&name));
                    if let Some(entry) = entry {
                        files.push((format!("{}.wav", entry.sound), false));
                    }
                    entry.is_some()
                }
                "Movie" => {
                    let movie = movies.iter().find(|m| m.eq_ignore_ascii_case(&name));
                    if let Some(movie) = movie {
                        files.push((format!("{movie}.bik"), false));
                    }
                    movie.is_some()
                }
                _ => {
                    files.push((name.clone(), false));
                    true
                }
            };
            dep(
                &m.file,
                &kind,
                &name,
                found,
                if kind == "Csf" && payload.contains_key("ra2md.csf") {
                    "package:ra2md.csf".into()
                } else {
                    "effective catalog".into()
                },
            );
            for (file, bag) in files {
                let candidates = if bag {
                    vec![file.clone(), format!("{file}.wav"), format!("{file}.aud")]
                } else {
                    vec![file.clone()]
                };
                let path = candidates.iter().find(|s| lookup(s).is_some());
                let valid = path.is_none_or(|s| {
                    !s.to_ascii_lowercase().ends_with(".wav")
                        || vera20k_core::assets::wav_file::WavFile::parse(lookup(s).unwrap())
                            .is_some()
                });
                let indexed = bag && audio.as_ref().is_some_and(|a| a.get(&file).is_some());
                let source = path
                    .map(|s| {
                        if payload.contains_key(&s.to_ascii_lowercase()) {
                            format!("package:{s}")
                        } else {
                            install
                                .manager
                                .get_with_source_ref(s)
                                .map_or("base installation".into(), |(_, src)| src.into())
                        }
                    })
                    .unwrap_or_else(|| {
                        if indexed {
                            if custom_audio {
                                "package audio.idx/audio.bag".into()
                            } else {
                                "base audio.idx/audio.bag".into()
                            }
                        } else {
                            "missing".into()
                        }
                    });
                dep(
                    &m.file,
                    "File",
                    path.map_or(file.as_str(), |s| s.as_str()),
                    valid && (path.is_some() || indexed),
                    source,
                );
            }
        }
    }
    Ok(Report {
        passed: !invalid_parameters && dependencies.iter().all(|d| d.resolved),
        entry: project.entry.clone(),
        shell_campaign: String::new(),
        dependencies,
        warnings,
        files: payload.keys().cloned().collect(),
    })
}

pub fn audit_map(bytes: &[u8], name: &str, install: &Install) -> Result<Report> {
    let map = crate::editor_io::validate(bytes)?;
    let ini = IniFile::from_bytes(install.manager.get_ref("missionmd.ini").unwrap_or_default())?;
    let meta = ini.section_or_empty(name);
    let mission = ra2_map_document::campaign::Mission {
        file: name.into(),
        ui_name: meta.read_string("UIName", "", 128),
        briefing: meta.read_string("Briefing", "", 128),
        load_message: meta.read_string("LSLoadMessage", "", 128),
        load_briefing: meta.read_string("LSLoadBriefing", "", 128),
        background_640: meta.read_string("LS640BkgdName", "", 128),
        background_800: meta.read_string("LS800BkgdName", "", 128),
        background_palette: meta.read_string("LS800BkgdPal", "", 128),
        ..Default::default()
    };
    let project = Project {
        version: 1,
        entry: name.into(),
        missions: vec![mission],
        ..Default::default()
    };
    audit_resources(
        &project,
        &[map],
        &BTreeMap::from([(name.to_ascii_lowercase(), bytes.to_vec())]),
        install,
    )
}

/// Install into a new directory against an explicit base installation. Only
/// base archives, selected process roots and declared file dependencies enter it.
pub fn install_clean(package: &Path, base: &Path, output: &Path) -> Result<()> {
    ensure!(!output.exists(), "验收安装目录已存在");
    let manifest: serde_json::Value =
        serde_json::from_slice(&fs::read(package.join("manifest.json"))?)?;
    let files = manifest["files_sha256"]
        .as_object()
        .context("无效发布清单")?;
    let report: serde_json::Value =
        serde_json::from_slice(&fs::read(package.join("dependencies.json"))?)?;
    ensure!(report["passed"] == true, "发布包依赖未通过");
    let mut checked = Vec::new();
    for (name, hash) in files {
        filename(name)?;
        let bytes = read_bounded(&package.join(name))?;
        ensure!(
            Some(vera20k_core::util::sha256::sha256_hex(&bytes).as_str()) == hash.as_str(),
            "发布文件校验失败：{name}"
        );
        checked.push((name.clone(), bytes));
    }
    fs::create_dir(output)?;
    // Only this newly-created directory is removed on a failed install.
    let result = (|| {
        let mut names: BTreeSet<String> = [
            "rulesmd.ini",
            "artmd.ini",
            "aimd.ini",
            "soundmd.ini",
            "evamd.ini",
            "thememd.ini",
            "theme.ini",
            "ra2md.csf",
            "audio.idx",
            "audio.bag",
            "temperatmd.ini",
            "snowmd.ini",
            "urbanmd.ini",
            "urbannmd.ini",
            "desertmd.ini",
            "lunarmd.ini",
        ]
        .into_iter()
        .map(str::to_owned)
        .collect();
        for d in report["dependencies"].as_array().context("依赖表无效")? {
            if d["kind"] == "File"
                && let Some(name) = d["name"].as_str()
            {
                filename(name)?;
                names.insert(name.to_ascii_lowercase());
            }
        }
        for e in fs::read_dir(base)? {
            let e = e?;
            if !e.file_type()?.is_file() {
                continue;
            }
            let name = e.file_name().to_string_lossy().to_ascii_lowercase();
            if (name.ends_with(".mix") || names.contains(&name)) && !files.contains_key(&name) {
                fs::hard_link(e.path(), output.join(&name))
                    .or_else(|_| fs::copy(e.path(), output.join(&name)).map(|_| ()))?;
            }
        }
        for (name, bytes) in checked {
            fs::write(output.join(name), bytes)?;
        }
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_dir_all(output);
    }
    result
}
/// Destination must be new; no user's installation is modified by export.
pub fn export(prepared: Prepared, output: &Path) -> Result<Report> {
    ensure!(prepared.report.passed, "资源依赖缺失，请先查看依赖报告");
    ensure!(!output.exists(), "发布目录已存在，请选择新目录");
    let parent = output.parent().context("发布目录无父目录")?;
    fs::create_dir_all(parent)?;
    let stage = parent.join(format!(
        ".yr-package-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_nanos()
    ));
    fs::create_dir(&stage)?;
    let result = (|| {
        let mut hashes = BTreeMap::new();
        for (name, bytes) in &prepared.payload {
            filename(name)?;
            fs::write(stage.join(name), bytes)?;
            hashes.insert(name, vera20k_core::util::sha256::sha256_hex(bytes));
        }
        fs::write(
            stage.join("dependencies.json"),
            serde_json::to_vec_pretty(&prepared.report)?,
        )?;
        fs::write(
            stage.join("manifest.json"),
            serde_json::to_vec_pretty(
                &serde_json::json!({"version":1,"engine":"VERA20k fixed submodule","files_sha256":hashes,"scope":"campaign maps and declared mission resources; stock assets supplied by recipient"}),
            )?,
        )?;
        fs::write(
            stage.join("INSTALL.txt"),
            "将本目录数据文件复制到单独的 YR/VERA20k 游戏目录。保留依赖报告，使用配套引擎；BATTLEMD/MISSIONMD/MAPSELMD 与 RA2MD.CSF 是进程资源，安装后重启游戏。此包不含游戏本体或原版 MIX。\r\n",
        )?;
        fs::rename(&stage, output)?;
        Ok(prepared.report.clone())
    })();
    if result.is_err() {
        let _ = fs::remove_dir_all(&stage);
    }
    result
}

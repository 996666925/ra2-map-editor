#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use anyhow::{Context, Result, bail, ensure};
use ra2_map_app::app::{EditorApp, LaunchOptions, SmokeOutcome};

fn main() -> Result<()> {
    env_logger::Builder::from_env(
        env_logger::Env::default()
            .default_filter_or("ra2_map_app=info,ra2_map_assets=info,ra2_map_scene=info,ra2_map_render=info,ra2_map_io=info,vera20k_core=warn,wgpu=warn"),
    )
    .init();
    if let Some(code) = ra2_map_io::cli::run(std::env::args_os().skip(1))? {
        std::process::exit(code);
    }
    let Some(options) = arguments()? else {
        return Ok(());
    };
    if let Some(output) = &options.smoke_output {
        std::fs::create_dir_all(output)?;
        std::fs::write(
            output.join("report.json"),
            b"{\"passed\":false,\"status\":\"starting\"}\n",
        )?;
    }
    let outcome: SmokeOutcome = Arc::new(Mutex::new(None));
    let result_handle = outcome.clone();
    let smoke = options.smoke_output.is_some();
    let native = eframe::NativeOptions {
        renderer: eframe::Renderer::Wgpu,
        viewport: eframe::egui::ViewportBuilder::default()
            .with_inner_size([1280.0, 800.0])
            .with_min_inner_size([900.0, 600.0]),
        multisampling: 0,
        persist_window: !smoke,
        persistence_path: options
            .smoke_output
            .as_ref()
            .map(|p| p.join("window-state.ron")),
        ..Default::default()
    };
    eframe::run_native(
        "YR Map Editor",
        native,
        Box::new(move |cc| {
            EditorApp::new(cc, options, outcome)
                .map(|app| Box::new(app) as Box<dyn eframe::App>)
                .map_err(Into::into)
        }),
    )
    .map_err(|error| anyhow::anyhow!("编辑器启动失败：{error}"))?;
    if smoke {
        match result_handle.lock().expect("smoke outcome").take() {
            Some(Ok(())) => (),
            Some(Err(error)) => bail!(error),
            None => bail!("烟测在完成前退出"),
        }
    }
    Ok(())
}

fn arguments() -> Result<Option<LaunchOptions>> {
    let mut options = LaunchOptions::default();
    let mut smoke = false;
    let mut smoke_output = None;
    let mut new_players = None;
    let mut new_theater = None;
    let mut args = std::env::args_os().skip(1);
    while let Some(arg) = args.next() {
        let arg = arg.to_str().context("参数名不是有效 Unicode")?;
        if matches!(arg, "--help" | "-h") {
            println!(
                "YR Map Editor — P2 basic editing\n\nUsage: ra2-map-editor [OPTIONS]\n  --game-dir PATH      YR game resources (or RA2_DIR; otherwise use the folder picker)\n  --profile FILE       Mod profile JSON (resources, schema, executable)\n  --map PATH_OR_NAME   Open a map file or MIX resource\n  --new-map WxH       Create map (20..492 per dimension, W+H <= 512)\n  --theater NAME      TEMPERATE, SNOW, URBAN, NEWURBAN, DESERT or LUNAR\n  --players N         Player starts for --new-map (2..30; default: 2)\n  --playtest-exe PATH  VERA20k executable for isolated -SPAWN playtests\n  --tmp NAME           TMP resource (default: clear01.tem)\n  --sub-tile INDEX     TMP sub-tile (default: 0)\n  --shp NAME           Unit-palette SHP resource (default: gi.shp)\n  --frame INDEX        SHP frame (default: 0)\n  --font PATH          CJK font override\n  --smoke-test         Verify P1 map when --map is set; otherwise verify P0 assets\n  --p2-smoke-test      Edit, undo/redo, recover, save, reopen, capture; needs --map or --new-map\n                      With --playtest-exe: also run the snapshot in VERA20k for 8s\n  --smoke-output PATH  Artifact directory (default: target/p0-smoke, p1-smoke or p2-smoke)"
            );
            println!("{}", ra2_map_io::cli::HELP);
            return Ok(None);
        }
        if arg == "--p2-smoke-test" {
            smoke = true;
            options.p2_smoke = true;
            continue;
        }
        if arg == "--smoke-test" {
            smoke = true;
            continue;
        }
        ensure!(
            matches!(
                arg,
                "--game-dir"
                    | "--profile"
                    | "--map"
                    | "--new-map"
                    | "--players"
                    | "--theater"
                    | "--tmp"
                    | "--sub-tile"
                    | "--shp"
                    | "--frame"
                    | "--font"
                    | "--playtest-exe"
                    | "--smoke-output"
            ),
            "未知参数：{arg}"
        );
        let value = args.next().with_context(|| format!("{arg} 缺少参数值"))?;
        match arg {
            "--profile" => {
                let p = ra2_map_assets::profile::Profile::read(&PathBuf::from(value))?;
                options.game_dir = Some(p.game_dir);
                options.playtest_exe = p.playtest_exe;
            }
            "--new-map" => {
                let text = value.to_str().context("尺寸无效")?;
                let (width, height) = text
                    .split_once(['x', 'X'])
                    .context("--new-map 使用 WIDTHxHEIGHT，例如 80x80")?;
                let new = ra2_map_document::new_map::NewMapOptions {
                    width: width.parse().context("地图宽度无效")?,
                    height: height.parse().context("地图高度无效")?,
                    ..Default::default()
                };
                new.validate()?;
                options.new_map = Some(new);
            }
            "--game-dir" => options.game_dir = Some(value.into()),
            "--players" => {
                new_players = Some(
                    value
                        .to_str()
                        .context("玩家数量无效")?
                        .parse::<u8>()
                        .context("玩家数量须为 2～30 的整数")?,
                );
            }
            "--theater" => {
                new_theater = Some(value.to_str().context("环境名称无效")?.to_ascii_uppercase())
            }
            "--map" => {
                options.map = Some(
                    value
                        .into_string()
                        .map_err(|_| anyhow::anyhow!("地图路径无效"))?,
                )
            }
            "--font" => options.font = Some(value.into()),
            "--playtest-exe" => options.playtest_exe = Some(value.into()),
            "--smoke-output" => smoke_output = Some(PathBuf::from(value)),
            "--tmp" => {
                options.tmp = Some(
                    value
                        .into_string()
                        .map_err(|_| anyhow::anyhow!("TMP 名称无效"))?,
                )
            }
            "--shp" => {
                options.shp = Some(
                    value
                        .into_string()
                        .map_err(|_| anyhow::anyhow!("SHP 名称无效"))?,
                )
            }
            "--sub-tile" => {
                options.sub_tile = Some(
                    value
                        .to_str()
                        .context("子块索引无效")?
                        .parse()
                        .context("子块索引应为非负整数")?,
                )
            }
            "--frame" => {
                options.frame = Some(
                    value
                        .to_str()
                        .context("帧索引无效")?
                        .parse()
                        .context("帧索引应为非负整数")?,
                )
            }
            _ => unreachable!(),
        }
    }
    if let Some(theater) = new_theater {
        let new = options
            .new_map
            .as_mut()
            .context("--theater 需要与 --new-map 一起使用")?;
        new.theater = theater;
        new.validate()?;
    }
    if let Some(players) = new_players {
        let new = options
            .new_map
            .as_mut()
            .context("--players 需要与 --new-map 一起使用")?;
        new.players = players;
        new.validate()?;
    }
    ensure!(
        options.map.is_none() || options.new_map.is_none(),
        "--map 与 --new-map 不能同时使用"
    );
    if smoke {
        ensure!(
            !options.p2_smoke || options.map.is_some() || options.new_map.is_some(),
            "P2 烟测需要 --map 或 --new-map"
        );
        ensure!(
            options.new_map.is_none() || options.p2_smoke,
            "新建地图烟测请使用 --p2-smoke-test"
        );
        if options.game_dir.is_none() {
            options.game_dir = std::env::var_os("RA2_DIR").map(PathBuf::from);
        }
        ensure!(
            options.game_dir.is_some(),
            "烟测需要 --game-dir PATH 或 RA2_DIR"
        );
        options.smoke_output = Some(smoke_output.unwrap_or_else(|| {
            PathBuf::from(if options.p2_smoke {
                "target/p2-smoke"
            } else if options.map.is_some() {
                "target/p1-smoke"
            } else {
                "target/p0-smoke"
            })
        }));
    }
    Ok(Some(options))
}

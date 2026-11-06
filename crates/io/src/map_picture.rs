//! Run VERA's tactical map-picture renderer on an isolated authoring snapshot.
use anyhow::{Context, Result, ensure};
use std::{
    fs,
    path::Path,
    process::{Command, Stdio},
    time::{Duration, Instant},
};

pub fn export(
    bytes: &[u8],
    exe: &Path,
    directory: &Path,
    root: &Path,
    path: &Path,
) -> Result<[u32; 2]> {
    crate::editor_io::validate(bytes)?;
    let exe = fs::canonicalize(exe)
        .context("VERA20k 可执行文件不存在；请在试玩设置中选择支持 --map-picture 的版本")?;
    let directory = fs::canonicalize(directory).context("游戏资源目录不存在")?;
    let revision = ra2_map_assets::profile::Revision::scan(&directory)?;
    let resources = crate::mission_script::playtest_payload(bytes, &directory, &revision.id())?;
    let folder = root.join("map-pictures").join(crate::editor_io::unique());
    fs::create_dir_all(&folder)?;
    let folder = fs::canonicalize(folder)?;
    let result = (|| {
        let game_dir = crate::editor_io::prepare_playtest_files(
            &folder,
            &directory,
            resources,
            &Default::default(),
        )?;
        let map = folder.join("snapshot.map");
        let output = folder.join("picture.png");
        fs::write(&map, bytes)?;
        let mut command = Command::new(&exe);
        command
            .arg("--map-picture")
            .arg(&map)
            .arg("--sha256")
            .arg(vera20k_core::util::sha256::sha256_hex(bytes))
            .arg("--output")
            .arg(&output)
            .arg("--area")
            .arg("view")
            .arg("--max-size")
            .arg("4096x4096")
            .current_dir(&folder)
            .env("RA2_DIR", game_dir)
            .stdin(Stdio::null())
            .stdout(fs::File::create(folder.join("stdout.log"))?)
            .stderr(fs::File::create(folder.join("stderr.log"))?);
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(0x08000000);
        }
        let mut child = command.spawn().context("启动 VERA20k 高清预览失败")?;
        let started = Instant::now();
        let status = loop {
            match child.try_wait() {
                Ok(Some(status)) => break status,
                Ok(None) if started.elapsed() < Duration::from_secs(180) => {
                    std::thread::sleep(Duration::from_millis(100));
                }
                result => {
                    let _ = child.kill();
                    let _ = child.wait();
                    result.context("等待 VERA20k 高清预览失败")?;
                    anyhow::bail!("VERA20k 高清预览超时");
                }
            }
        };
        ensure!(
            status.success(),
            "VERA20k 高清预览失败（{status}），请确认引擎支持 --map-picture；日志：{}",
            folder.display()
        );
        let image = image::open(&output)
            .context("VERA20k 未生成有效的高清预览 PNG")?
            .to_rgba8();
        ensure!(
            image.width() > 0
                && image.height() > 0
                && image.width() <= 4096
                && image.height() <= 4096,
            "VERA20k 高清预览尺寸无效"
        );
        let size = [image.width(), image.height()];
        crate::publication::write_png(&image, path)?;
        Ok(size)
    })();
    // Failed runs retain their engine logs. Only our own successful session is removed.
    if result.is_ok() {
        let _ = fs::remove_dir_all(&folder);
    }
    result
}

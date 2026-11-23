use std::path::Path;
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::{Context, Result, ensure};
use eframe::egui::{self, RichText, Stroke};
use self_update::backends::custom;
#[cfg(any(not(windows), test))]
use self_update::backends::github;
#[cfg(any(windows, test))]
use self_update::backends::manifest::ManifestSource;
use self_update::reqwest::{Url, blocking::Client, redirect};
use self_update::update::{Release, ReleaseSource};
use semver::Version;

use crate::i18n::tr;
use crate::ui_theme as theme;

const VERSION: &str = self_update::cargo_crate_version!();
#[cfg(any(windows, test))]
const WINDOWS_UPDATE_BASE_URL: &str = "http://3bbs.top/download/";
#[cfg(not(windows))]
const GITHUB_API: &str = "https://api.github.com";

#[cfg(windows)]
const UPDATE_LINE_STORAGE_KEY: &str = "editor-update-line";

#[cfg(windows)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "snake_case")]
enum UpdateLine {
    #[default]
    New,
    Old,
}

#[cfg(windows)]
impl UpdateLine {
    fn label(self) -> &'static str {
        match self {
            Self::New => tr!("新线路"),
            Self::Old => tr!("旧线路"),
        }
    }

    fn check(self) -> Result<Option<Release>> {
        match self {
            Self::New => check_static(&format!("{WINDOWS_UPDATE_BASE_URL}manifest.json")),
            Self::Old => check_static("https://download.ra2.diy/manifest.json"),
        }
    }
}

#[cfg(not(windows))]
fn check() -> Result<Option<Release>> {
    let repo =
        option_env!("RA2_MAP_EDITOR_GITHUB_REPOSITORY").unwrap_or("996666925/ra2-map-editor");
    check_github(GITHUB_API, repo)
}

fn native_target() -> String {
    format!("{}-{}", std::env::consts::OS, std::env::consts::ARCH)
}

fn asset_name(target: &str, legacy_version: Option<&Version>) -> String {
    let suffix = if target.starts_with("windows-") {
        ".exe"
    } else {
        ""
    };
    match legacy_version {
        Some(version) => format!("ra2-map-editor-v{version}-{target}{suffix}"),
        None => format!("ra2-map-editor-{target}{suffix}"),
    }
}

fn parse_url(value: &str) -> Result<Url> {
    let url = Url::parse(value).context("Invalid update URL")?;
    let allow_http = url.host_str() == Some("127.0.0.1");
    #[cfg(any(windows, test))]
    let allow_http = allow_http || url.as_str().starts_with(WINDOWS_UPDATE_BASE_URL);
    ensure!(
        url.scheme() == "https" || (url.scheme() == "http" && allow_http),
        "HTTP is allowed only for the configured update server or 127.0.0.1"
    );
    ensure!(
        url.username().is_empty() && url.password().is_none(),
        "Update URLs cannot contain credentials"
    );
    Ok(url)
}

fn http_client(url: &Url) -> Result<Client> {
    let builder = Client::builder()
        .user_agent("ra2-map-editor")
        .connect_timeout(Duration::from_secs(10));
    if url.scheme() == "https" {
        Ok(builder.https_only(true).build()?)
    } else {
        let builder = builder.redirect(redirect::Policy::none());
        let builder = if url.host_str() == Some("127.0.0.1") {
            builder.no_proxy()
        } else {
            builder
        };
        Ok(builder.build()?)
    }
}

#[cfg(any(windows, test))]
fn check_static(url: &str) -> Result<Option<Release>> {
    let url = parse_url(url)?;
    let mut source = ManifestSource::new(url.as_str());
    source
        .reqwest_client(http_client(&url)?)
        .timeout(Duration::from_secs(15));
    select_for_source(source.get_releases()?, &url)
}

#[cfg(any(not(windows), test))]
fn check_github(api: &str, repo: &str) -> Result<Option<Release>> {
    let (owner, name) = repo
        .split_once('/')
        .context("Expected a GitHub repository in owner/name format")?;
    ensure!(
        !owner.is_empty() && !name.is_empty() && !name.contains('/'),
        "Invalid GitHub repository"
    );
    let api = parse_url(api)?;
    let releases = github::ReleaseList::configure()
        .repo_owner(owner)
        .repo_name(name)
        .api_base_url(api.as_str().trim_end_matches('/'))
        .reqwest_client(http_client(&api)?)
        .timeout(Duration::from_secs(15))
        .build()?
        .fetch()?;
    select_for_source(releases.into_vec(), &api)
}

fn select_for_source(releases: Vec<Release>, url: &Url) -> Result<Option<Release>> {
    let release = select_release(releases, &Version::parse(VERSION)?, &native_target())?;
    if let Some(release) = &release {
        let asset_url = parse_url(release.assets()[0].download_url())?;
        if url.scheme() == "http" {
            ensure!(
                url.origin() == asset_url.origin(),
                "An HTTP update source must serve its executable from the same origin"
            );
        } else {
            ensure!(
                asset_url.scheme() == "https",
                "An HTTPS update source must serve its executable over HTTPS"
            );
        }
    }
    Ok(release)
}

fn select_release(
    releases: Vec<Release>,
    current: &Version,
    target: &str,
) -> Result<Option<Release>> {
    let name = asset_name(target, None);
    let newest = releases
        .into_iter()
        .filter_map(|release| {
            let version = Version::parse(release.version()).ok()?;
            if !version.pre.is_empty() || !version.build.is_empty() || &version <= current {
                return None;
            }
            let find = |name: &str| release.assets().iter().find(|asset| asset.name() == name);
            let asset = find(&name).or_else(|| {
                let legacy_name = asset_name(target, Some(&version));
                find(&legacy_name)
            })?;
            Some((version, asset.clone()))
        })
        .max_by(|(left, _), (right, _)| left.cmp(right));
    let Some((version, asset)) = newest else {
        return Ok(None);
    };
    ensure!(
        asset
            .digest()
            .and_then(|digest| digest.strip_prefix("sha256:"))
            .is_some_and(|hash| {
                hash.len() == 64 && hash.bytes().all(|byte| byte.is_ascii_hexdigit())
            }),
        "Missing or invalid SHA-256 for {}",
        asset.name()
    );
    Ok(Some(
        Release::builder()
            .version(version.to_string())
            .asset(asset)
            .build()?,
    ))
}

struct ConfirmedRelease(Release);

impl ReleaseSource for ConfirmedRelease {
    fn get_releases(&self) -> self_update::Result<Vec<Release>> {
        Ok(vec![self.0.clone()])
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum InstallProgress {
    Downloading { bytes: u64, total: Option<u64> },
    Applying,
}

fn install(
    release: Release,
    destination: &Path,
    report: impl Fn(InstallProgress) + Send + Sync + 'static,
) -> Result<()> {
    let report = Arc::new(report);
    let download_progress = Arc::clone(&report);
    let asset = &release.assets()[0];
    let client = http_client(&parse_url(asset.download_url())?)?;
    // Keep the version, asset and digest shown at confirmation even if the manifest changes.
    let mut update = custom::Update::configure();
    update
        .source(ConfirmedRelease(release))
        .bin_name("ra2-map-editor")
        .bin_install_path(destination)
        .check_install_path_writable(true)
        .target(native_target())
        .current_version(VERSION)
        .reqwest_client(client)
        .timeout(Duration::from_secs(180))
        .unattended()
        .verify_release_digest(true)
        .progress_callback(move |bytes, total| {
            download_progress(InstallProgress::Downloading { bytes, total });
        })
        .verify_archive(move |_| {
            // This hook runs after the digest check and before the executable is replaced.
            report(InstallProgress::Applying);
            Ok(())
        });
    let status = install_binary(update, destination)?;
    ensure!(status.is_updated(), "No newer update to install");
    Ok(())
}

#[cfg(windows)]
fn install_binary(
    mut update: custom::UpdateBuilder,
    destination: &Path,
) -> Result<self_update::VersionStatus> {
    use std::fs;
    use std::os::windows::fs::OpenOptionsExt;

    // A process handle keeps the lock in place during installation and Windows removes it on close.
    const FILE_FLAG_DELETE_ON_CLOSE: u32 = 0x0400_0000;
    let _lock = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .share_mode(0)
        .custom_flags(FILE_FLAG_DELETE_ON_CLOSE)
        .open(destination.with_extension("exe.update-lock"))
        .context("Cannot lock the editor executable for updating")?;
    let backup_dir = tempfile::Builder::new()
        .prefix(".ra2-editor-update-")
        .tempdir_in(
            destination
                .parent()
                .context("Executable has no parent directory")?,
        )?;
    let backup = backup_dir.path().join("previous.exe");
    let snapshot = (destination.to_owned(), backup.clone());

    let result = update
        .verify_binary(move |_| {
            // Windows can move the running executable aside before the final swap.
            fs::copy(&snapshot.0, &snapshot.1)?;
            Ok(())
        })
        .build()?
        .update();
    if result.is_err()
        && matches!(destination.try_exists(), Ok(false))
        && backup.is_file()
        && let Err(error) = fs::rename(&backup, destination)
    {
        let saved = backup_dir.keep().join("previous.exe");
        return Err(error).with_context(|| format!("Update failed and automatic recovery could not restore the executable. The previous executable is at {}", saved.display()));
    }
    Ok(result?)
}

#[cfg(not(windows))]
fn install_binary(
    update: custom::UpdateBuilder,
    destination: &Path,
) -> Result<self_update::VersionStatus> {
    // self_update replaces the Unix executable with a same-directory rename and retains its mode.
    let running_file = std::fs::File::open(destination)?;
    running_file
        .try_lock()
        .context("Cannot lock the editor executable for updating")?;
    Ok(update.build()?.update()?)
}

enum Status {
    Idle,
    Checking(Receiver<Result<Self>>),
    UpToDate,
    Available(Release),
    Installing {
        result: Receiver<Result<Self>>,
        progress: Arc<Mutex<InstallProgress>>,
        version: String,
    },
    Installed(String),
    Failed(String),
}

pub(crate) struct Updater {
    status: Status,
    show_dialog: bool,
    #[cfg(windows)]
    line: UpdateLine,
}

impl Updater {
    pub(crate) fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let mut updater = Self {
            status: Status::Idle,
            show_dialog: false,
            #[cfg(windows)]
            line: cc
                .storage
                .and_then(|storage| eframe::get_value(storage, UPDATE_LINE_STORAGE_KEY))
                .unwrap_or_default(),
        };
        updater.start_check(&cc.egui_ctx);
        updater
    }

    #[cfg(windows)]
    pub(crate) fn save(&self, storage: &mut dyn eframe::Storage) {
        eframe::set_value(storage, UPDATE_LINE_STORAGE_KEY, &self.line);
    }

    fn worker(
        ctx: &egui::Context,
        run: impl FnOnce() -> Result<Status> + Send + 'static,
    ) -> Result<Receiver<Result<Status>>> {
        let (tx, rx) = mpsc::channel();
        let ctx = ctx.clone();
        std::thread::Builder::new()
            .name("editor-update".into())
            .spawn(move || {
                let _ = tx.send(run());
                ctx.request_repaint();
            })?;
        Ok(rx)
    }

    fn start_check(&mut self, ctx: &egui::Context) {
        #[cfg(windows)]
        let line = self.line;
        self.status = match Self::worker(ctx, move || {
            #[cfg(windows)]
            let release = line.check()?;
            #[cfg(not(windows))]
            let release = check()?;
            Ok(release.map_or(Status::UpToDate, Status::Available))
        }) {
            Ok(rx) => Status::Checking(rx),
            Err(error) => Self::failed(error),
        };
    }

    fn failed(error: anyhow::Error) -> Status {
        log::warn!("Editor update failed: {error:#}");
        Status::Failed(format!("{error:#}"))
    }

    pub(crate) fn poll(&mut self, ctx: &egui::Context) {
        let rx = match &self.status {
            Status::Checking(rx) => rx,
            Status::Installing { result, .. } => result,
            _ => return,
        };
        self.status = match rx.try_recv() {
            Ok(Ok(status)) => status,
            Ok(Err(error)) => Self::failed(error),
            Err(TryRecvError::Disconnected) => {
                Self::failed(anyhow::anyhow!("Update worker exited"))
            }
            Err(TryRecvError::Empty) => {
                if matches!(self.status, Status::Installing { .. })
                    && ctx.input(|i| i.viewport().close_requested())
                {
                    ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
                }
                return;
            }
        };
        self.show_dialog |= matches!(self.status, Status::Available(_));
    }

    pub(crate) fn button(&mut self, ui: &mut egui::Ui) {
        if ui.button(tr!("检查更新…")).clicked() {
            self.show_dialog = true;
            if matches!(
                self.status,
                Status::Idle | Status::UpToDate | Status::Failed(_)
            ) {
                self.start_check(ui.ctx());
            }
        }
    }

    pub(crate) fn dialog(&mut self, ctx: &egui::Context) {
        if !self.show_dialog {
            return;
        }
        let installing = matches!(self.status, Status::Installing { .. });
        #[cfg(windows)]
        let previous_line = self.line;
        let response = theme::modal(ctx, "editor-update", |ui| {
            ui.set_width((ctx.content_rect().width() - 36.).clamp(120., 436.));
            egui::Frame::new()
                .inner_margin(egui::Margin::same(12))
                .show(ui, |ui| self.dialog_contents(ui))
                .inner
        });
        #[cfg(windows)]
        if self.line != previous_line {
            self.start_check(ctx);
            return;
        }
        match response.inner {
            Some(DialogAction::Install(release)) => {
                let version = release.version().to_owned();
                let installed_version = version.clone();
                let progress = Arc::new(Mutex::new(InstallProgress::Downloading {
                    bytes: 0,
                    total: None,
                }));
                let worker_progress = Arc::clone(&progress);
                self.status = match Self::worker(ctx, move || {
                    install(release, &std::env::current_exe()?, move |latest| {
                        *worker_progress.lock().unwrap() = latest;
                    })?;
                    Ok(Status::Installed(installed_version))
                }) {
                    Ok(result) => Status::Installing {
                        result,
                        progress,
                        version,
                    },
                    Err(error) => Self::failed(error),
                };
            }
            Some(DialogAction::Retry) => self.start_check(ctx),
            Some(DialogAction::Close) => self.show_dialog = false,
            None if !installing && response.should_close() => self.show_dialog = false,
            None => {}
        }
    }

    fn dialog_contents(&mut self, ui: &mut egui::Ui) -> Option<DialogAction> {
        let progress = match &self.status {
            Status::Installing { progress, .. } => Some(*progress.lock().unwrap()),
            _ => None,
        };
        let (summary, color) = match &self.status {
            Status::Idle => ("", theme::ACCENT),
            Status::Checking(_) => (tr!("正在检查更新…"), theme::BLUE),
            Status::UpToDate => (tr!("已是最新版本。"), theme::GREEN),
            Status::Available(_) => (tr!("发现新版本"), theme::ACCENT),
            Status::Installing { .. } => {
                let summary = match progress.unwrap() {
                    InstallProgress::Applying => tr!("正在安装更新…"),
                    InstallProgress::Downloading {
                        bytes,
                        total: Some(total),
                    } if total > 0 && bytes >= total => tr!("正在校验更新…"),
                    InstallProgress::Downloading { .. } => tr!("正在下载更新…"),
                };
                (summary, theme::BLUE)
            }
            Status::Installed(_) => (tr!("更新已安装"), theme::GREEN),
            Status::Failed(_) => (tr!("更新失败。"), ui.visuals().error_fg_color),
        };
        ui.horizontal(|ui| {
            let (rect, _) = ui.allocate_exact_size(egui::Vec2::splat(44.), egui::Sense::hover());
            let painter = ui.painter_at(rect);
            painter.rect_filled(rect, 10, color.gamma_multiply(0.15));
            let center = rect.center();
            let at = |x, y| center + egui::vec2(x, y);
            let stroke = Stroke::new(2., color);
            match &self.status {
                Status::Checking(_) | Status::Installing { .. } => {
                    egui::Spinner::new().color(color).paint_at(
                        ui,
                        egui::Rect::from_center_size(center, egui::Vec2::splat(20.)),
                    );
                }
                Status::UpToDate | Status::Installed(_) => {
                    painter.line(vec![at(-9., 0.), at(-3., 6.), at(9., -6.)], stroke);
                }
                Status::Failed(_) => {
                    painter.line_segment([at(0., -8.), at(0., 2.)], stroke);
                    painter.circle_filled(at(0., 7.), 1.25, color);
                }
                Status::Idle | Status::Available(_) => {
                    painter.line_segment([at(0., -9.), at(0., 4.)], stroke);
                    painter.line(vec![at(-5., -1.), at(0., 4.), at(5., -1.)], stroke);
                    painter.line_segment([at(-8., 9.), at(8., 9.)], stroke);
                }
            }
            ui.add_space(4.);
            ui.vertical(|ui| {
                ui.heading(RichText::new(tr!("软件更新")).size(20.).strong());
                ui.label(RichText::new(summary).color(color));
            });
        });

        let next = match &self.status {
            Status::Available(release) => Some((tr!("新版本"), release.version())),
            Status::Installing { version, .. } => Some((tr!("新版本"), version.as_str())),
            Status::Installed(version) => Some((tr!("已安装版本"), version.as_str())),
            _ => None,
        };
        ui.add_space(12.);
        egui::Frame::new()
            .fill(theme::BACKGROUND)
            .stroke(Stroke::new(1., theme::BORDER))
            .corner_radius(8)
            .inner_margin(egui::Margin::symmetric(14, 12))
            .show(ui, |ui| {
                let width = ui.available_width();
                if let Some((label, version)) = next {
                    ui.columns(2, |columns| {
                        Self::version_label(&mut columns[0], tr!("当前版本"), VERSION, theme::TEXT);
                        Self::version_label(&mut columns[1], label, version, color);
                    });
                } else {
                    ui.set_min_width(width);
                    Self::version_label(ui, tr!("当前版本"), VERSION, theme::TEXT);
                }
            });

        #[cfg(windows)]
        {
            ui.add_space(8.);
            let can_switch = !matches!(
                self.status,
                Status::Installing { .. } | Status::Installed(_)
            );
            ui.add_enabled_ui(can_switch, |ui| {
                ui.horizontal(|ui| {
                    let label = ui.label(tr!("更新线路"));
                    egui::ComboBox::from_id_salt("editor-update-line")
                        .selected_text(self.line.label())
                        .show_ui(ui, |ui| {
                            for line in [UpdateLine::New, UpdateLine::Old] {
                                ui.selectable_value(&mut self.line, line, line.label());
                            }
                        })
                        .response
                        .labelled_by(label.id);
                });
            });
        }

        match &self.status {
            Status::Available(_) => {
                ui.add_space(6.);
                ui.label(
                    RichText::new(tr!("安装后重启编辑器即可使用新版本。")).color(theme::MUTED),
                );
            }
            Status::Installed(_) => {
                ui.add_space(6.);
                ui.label(RichText::new(tr!("请保存地图并重新打开编辑器。")).color(theme::MUTED));
            }
            Status::Failed(error) => {
                ui.add_space(6.);
                egui::CollapsingHeader::new(tr!("错误详情")).show(ui, |ui| {
                    egui::ScrollArea::vertical().max_height(94.).show(ui, |ui| {
                        ui.add(
                            egui::Label::new(RichText::new(error).small())
                                .wrap()
                                .selectable(true),
                        );
                    });
                });
            }
            _ => {}
        }

        if let Some(progress) = progress {
            ui.add_space(8.);
            Self::download_progress(ui, progress);
            return None;
        }
        ui.add_space(10.);
        theme::right_to_left(ui, |ui| {
            let action = match &self.status {
                Status::Available(release)
                    if ui.add(theme::primary(tr!("下载并安装"))).clicked() =>
                {
                    Some(DialogAction::Install(release.clone()))
                }
                Status::Failed(_) if ui.add(theme::primary(tr!("重试"))).clicked() => {
                    Some(DialogAction::Retry)
                }
                _ => None,
            };
            let secondary = if matches!(self.status, Status::Available(_)) {
                tr!("稍后")
            } else {
                tr!("关闭")
            };
            if ui.button(secondary).clicked() {
                Some(DialogAction::Close)
            } else {
                action
            }
        })
        .inner
    }

    fn download_progress(ui: &mut egui::Ui, progress: InstallProgress) {
        const MIB: f64 = 1024. * 1024.;
        let amount = match progress {
            InstallProgress::Applying => Some((1., 100)),
            InstallProgress::Downloading { bytes, total } => {
                let downloaded = format!("{:.1} MiB", bytes as f64 / MIB);
                if let Some(total) = total.filter(|&total| total > 0) {
                    let fraction = (bytes as f64 / total as f64).min(1.);
                    let total = format!("{:.1} MiB", total as f64 / MIB);
                    ui.label(RichText::new(format!("{downloaded} / {total}")).color(theme::MUTED));
                    Some((fraction as f32, (fraction * 100.) as u32))
                } else {
                    ui.label(
                        RichText::new(tr!("已下载 {downloaded}", downloaded = downloaded))
                            .color(theme::MUTED),
                    );
                    None
                }
            }
        };
        let width = ui.available_width();
        if let Some((fraction, percent)) = amount {
            ui.add(
                egui::ProgressBar::new(fraction)
                    .desired_width(width)
                    .desired_height(18.)
                    .fill(theme::BLUE.gamma_multiply(0.35))
                    .text(format!("{percent}%")),
            );
        } else {
            // The server did not give a total size. Animate a segment without implying a percentage.
            let (track, _) = ui.allocate_exact_size(egui::vec2(width, 8.), egui::Sense::hover());
            let painter = ui.painter();
            painter.rect_filled(track, 4, theme::BACKGROUND);
            let phase = ui.input(|input| ((input.time * 3.).cos() + 1.) as f32 * 0.5);
            let segment = egui::Rect::from_min_size(
                track.min + egui::vec2(track.width() * 0.75 * phase, 0.),
                egui::vec2(track.width() * 0.25, track.height()),
            );
            painter.rect_filled(segment, 4, theme::BLUE);
        }
    }

    fn version_label(ui: &mut egui::Ui, label: &str, version: &str, color: egui::Color32) {
        ui.label(RichText::new(label).size(12.).color(theme::MUTED));
        ui.add(
            egui::Label::new(RichText::new(version).monospace().size(21.).color(color)).truncate(),
        )
        .on_hover_text(version);
    }
}

enum DialogAction {
    Install(Release),
    Retry,
    Close,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::io::{BufRead, BufReader, Write};
    use std::net::TcpListener;

    const HELLO_HASH: &str =
        "sha256:2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824";

    fn release(version: &str, target: &str, digest: Option<&str>) -> Release {
        let name = asset_name(target, None);
        let mut asset =
            self_update::update::ReleaseAsset::new(&name, format!("https://example.com/{name}"));
        if let Some(digest) = digest {
            asset = asset.with_digest(digest);
        }
        Release::builder()
            .version(version)
            .asset(asset)
            .build()
            .unwrap()
    }

    #[test]
    fn update_dialog_keeps_version_comparison_and_actions_visible_after_resize() {
        use crate::i18n::{Language, enter};
        use egui_kittest::kittest::Queryable as _;

        for language in [Language::Chinese, Language::English] {
            let _language = enter(language);
            let mut version = Version::parse(VERSION).unwrap();
            version.patch += 1;
            let updater = Updater {
                status: Status::Available(release(
                    &version.to_string(),
                    &native_target(),
                    Some(HELLO_HASH),
                )),
                show_dialog: true,
                #[cfg(windows)]
                line: UpdateLine::default(),
            };
            let mut harness = egui_kittest::Harness::builder()
                .with_context_setup(|ctx| {
                    theme::install(ctx);
                    ctx.add_font_provider(std::sync::Arc::new(
                        egui_system_font_provider::SystemFontProvider::new(),
                    ));
                })
                .with_size([760., 460.])
                .build_ui_state(|ui, updater| updater.dialog(ui.ctx()), updater);
            for size in [[760., 460.], [320., 340.]] {
                harness.set_size(size.into());
                harness.run();
                let bounds = harness.ctx.content_rect();
                let current = harness.get_by_label(tr!("当前版本")).rect();
                let next = harness.get_by_label(tr!("新版本")).rect();
                let later = harness.get_by_label(tr!("稍后")).rect();
                let install = harness.get_by_label(tr!("下载并安装")).rect();
                assert!(
                    [current, next, later, install]
                        .into_iter()
                        .all(|rect| bounds.contains_rect(rect))
                );
                assert!(current.right() < next.left());
                assert!(later.right() < install.left());
            }
            harness.get_by_label(tr!("稍后")).click();
            harness.run();
            assert!(!harness.state().show_dialog);
            assert!(matches!(harness.state().status, Status::Available(_)));
        }
    }

    #[cfg(windows)]
    #[test]
    fn update_dialog_switches_between_named_lines() {
        use crate::i18n::{Language, enter};
        use egui_kittest::kittest::Queryable as _;

        let _language = enter(Language::English);
        let updater = Updater {
            status: Status::UpToDate,
            show_dialog: true,
            line: UpdateLine::default(),
        };
        let mut harness = egui_kittest::Harness::builder()
            .with_context_setup(theme::install)
            .with_size([320., 340.])
            .build_ui_state(
                |ui, updater| {
                    updater.dialog_contents(ui);
                },
                updater,
            );
        assert_eq!(harness.state().line, UpdateLine::New);
        harness.get_by_label("Update server").click();
        harness.run();
        harness.get_by_label("Old server").click();
        harness.run();
        assert_eq!(harness.state().line, UpdateLine::Old);
        harness.get_by_label("Update server").click();
        harness.run();
        harness.get_by_label("New server").click();
        harness.run();
        assert_eq!(harness.state().line, UpdateLine::New);
    }

    #[test]
    fn install_dialog_displays_download_progress_and_install_stages() {
        use crate::i18n::{Language, enter};
        use egui_kittest::kittest::Queryable as _;

        let _language = enter(Language::English);
        let (finish, result) = mpsc::channel();
        let progress = Arc::new(Mutex::new(InstallProgress::Downloading {
            bytes: 2 * 1024 * 1024,
            total: Some(8 * 1024 * 1024),
        }));
        let updater = Updater {
            status: Status::Installing {
                result,
                progress: Arc::clone(&progress),
                version: "0.9.0".into(),
            },
            show_dialog: true,
            #[cfg(windows)]
            line: UpdateLine::default(),
        };
        let mut harness = egui_kittest::Harness::<Updater>::builder()
            .with_context_setup(theme::install)
            .with_size([320., 340.])
            .build_ui_state(|ui, updater| updater.dialog(ui.ctx()), updater);
        harness.run_steps(3);
        let bounds = harness.ctx.content_rect();
        for label in ["25%", "2.0 MiB / 8.0 MiB", "New version"] {
            assert!(bounds.contains_rect(harness.get_by_label(label).rect()));
        }
        for (event, label) in [
            (
                InstallProgress::Downloading {
                    bytes: 8 * 1024 * 1024,
                    total: Some(8 * 1024 * 1024),
                },
                "Verifying update…",
            ),
            (
                InstallProgress::Downloading {
                    bytes: 2 * 1024 * 1024,
                    total: None,
                },
                "Downloaded 2.0 MiB",
            ),
            (InstallProgress::Applying, "Installing update…"),
        ] {
            *progress.lock().unwrap() = event;
            harness.run_steps(3);
            assert!(bounds.contains_rect(harness.get_by_label(label).rect()));
        }
        finish.send(Ok(Status::Installed("0.9.0".into()))).unwrap();
        let ctx = harness.ctx.clone();
        harness.state_mut().poll(&ctx);
        harness.run_steps(3);
        harness.get_by_label("Update installed");
    }

    #[test]
    fn selects_the_latest_stable_executable_for_each_platform() {
        let current = Version::new(0, 2, 5);
        let target = "windows-x86_64";
        let mut releases = vec![
            release("0.2.10", "macos-aarch64", Some(HELLO_HASH)),
            release("0.2.9", "macos-x86_64", Some(HELLO_HASH)),
            release("0.2.5", target, Some(HELLO_HASH)),
            release("0.2.4", target, Some(HELLO_HASH)),
            release("0.3.0-beta.1", target, Some(HELLO_HASH)),
        ];
        assert!(
            select_release(releases.clone(), &current, target)
                .unwrap()
                .is_none()
        );
        releases.extend([
            release("0.2.6", target, Some(HELLO_HASH)),
            release("0.2.8", target, Some(HELLO_HASH)),
            release("0.2.7", target, Some(HELLO_HASH)),
        ]);
        assert_eq!(
            select_release(releases.clone(), &current, target)
                .unwrap()
                .unwrap()
                .version(),
            "0.2.8"
        );
        for (target, expected) in [("macos-aarch64", "0.2.10"), ("macos-x86_64", "0.2.9")] {
            let release = select_release(releases.clone(), &current, target)
                .unwrap()
                .unwrap();
            assert_eq!(release.version(), expected);
            assert_eq!(
                release.assets()[0].name(),
                format!("ra2-map-editor-{target}")
            );
        }
    }

    #[test]
    fn prefers_the_fixed_asset_name_and_accepts_older_release_names() {
        let current = Version::new(0, 2, 5);
        let version = Version::new(0, 2, 6);
        let target = "windows-x86_64";
        let fixed = release(&version.to_string(), target, Some(HELLO_HASH));
        let legacy_name = "ra2-map-editor-v0.2.6-windows-x86_64.exe";
        let legacy =
            self_update::update::ReleaseAsset::new(legacy_name, "https://example.com/older.exe")
                .with_digest(HELLO_HASH);
        for (assets, expected) in [
            (vec![legacy.clone()], legacy_name),
            (
                vec![legacy, fixed.assets()[0].clone()],
                "ra2-map-editor-windows-x86_64.exe",
            ),
        ] {
            let release = Release::builder()
                .version(version.to_string())
                .assets(assets)
                .build()
                .unwrap();
            let selected = select_release(vec![release], &current, target)
                .unwrap()
                .unwrap();
            assert_eq!(selected.assets()[0].name(), expected);
        }
    }

    #[test]
    fn requires_sha256_and_restricts_http_update_urls() {
        let current = Version::new(0, 2, 5);
        let target = "macos-aarch64";
        assert!(select_release(vec![release("0.2.6", target, None)], &current, target).is_err());
        assert!(
            select_release(
                vec![release("0.2.6", target, Some("sha256:123"))],
                &current,
                target
            )
            .is_err()
        );
        assert!(parse_url(&format!("{WINDOWS_UPDATE_BASE_URL}manifest.json")).is_ok());
        assert!(
            parse_url(&format!(
                "{WINDOWS_UPDATE_BASE_URL}ra2-map-editor-windows-x86_64.exe"
            ))
            .is_ok()
        );
        assert!(parse_url("http://3bbs.top/manifest.json").is_err());
        assert!(parse_url("http://3bbs.top:8080/download/manifest.json").is_err());
        assert!(parse_url("http://3bbs.top.example.com/download/manifest.json").is_err());
        assert!(parse_url("http://127.0.0.1:8080/manifest.json").is_ok());
        assert!(parse_url("http://example.com/manifest.json").is_err());
        assert!(parse_url("https://example.com/manifest.json").is_ok());
    }

    #[test]
    fn reads_github_release_assets_for_the_native_target() {
        let mut version = Version::parse(VERSION).unwrap();
        version.patch += 1;
        let name = asset_name(&native_target(), None);
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let api = format!("http://{}", listener.local_addr().unwrap());
        let asset_url = format!("{api}/repos/owner/editor/releases/assets/1");
        let body = serde_json::json!([{
            "tag_name": format!("v{version}"),
            "created_at": "2026-11-21T00:00:00Z",
            "assets": [{ "name": name, "url": asset_url, "digest": HELLO_HASH }],
        }])
        .to_string();
        let server = std::thread::spawn(move || {
            respond(
                &listener,
                "/repos/owner/editor/releases?per_page=100",
                body.as_bytes(),
            );
        });
        let release = check_github(&api, "owner/editor").unwrap().unwrap();
        assert_eq!(release.version(), version.to_string());
        let asset = &release.assets()[0];
        assert_eq!(asset.download_url(), asset_url);
        assert_eq!(asset.digest(), Some(HELLO_HASH));
        server.join().unwrap();
    }

    fn respond(listener: &TcpListener, path: &str, payload: &[u8]) {
        let (mut stream, _) = listener.accept().unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        let mut request = BufReader::new(&mut stream);
        let mut line = String::new();
        request.read_line(&mut line).unwrap();
        assert_eq!(line.split_whitespace().nth(1), Some(path));
        loop {
            line.clear();
            if request.read_line(&mut line).unwrap() == 0 || line == "\r\n" {
                break;
            }
        }
        write!(
            stream,
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            payload.len()
        )
        .unwrap();
        stream.write_all(payload).unwrap();
    }

    #[test]
    fn downloads_from_static_manifest_and_checks_digest_before_replacing() {
        const CHILD_URL: &str = "RA2_UPDATE_TEST_CHILD_URL";
        if let Ok(url) = std::env::var(CHILD_URL) {
            let release = check_static(&url).unwrap().unwrap();
            let progress = Arc::new(Mutex::new(Vec::new()));
            let events = Arc::clone(&progress);
            install(release, &std::env::current_exe().unwrap(), move |event| {
                events.lock().unwrap().push(event);
            })
            .unwrap();
            let events = progress.lock().unwrap();
            assert!(events.ends_with(&[
                InstallProgress::Downloading {
                    bytes: 5,
                    total: Some(5),
                },
                InstallProgress::Applying,
            ]));
            return;
        }
        for (body, should_install) in [(b"hello".as_slice(), true), (b"corrupt".as_slice(), false)]
        {
            let mut version = Version::parse(VERSION).unwrap();
            version.patch += 1;
            let name = asset_name(&native_target(), None);
            let manifest = serde_json::json!({
                "schema": 1,
                "releases": [{ "version": version.to_string(), "assets": [{
                    "name": name, "url": name, "digest": HELLO_HASH,
                }] }],
            })
            .to_string();
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            let url = format!(
                "http://{}/updates/manifest.json",
                listener.local_addr().unwrap()
            );
            let server = std::thread::spawn(move || {
                respond(&listener, "/updates/manifest.json", manifest.as_bytes());
                respond(&listener, &format!("/updates/{name}"), body);
            });
            let temp = tempfile::tempdir().unwrap();
            let destination = temp.path().join(asset_name(&native_target(), None));
            if should_install {
                // Run a disposable copy to exercise the Windows in-use executable replacement.
                fs::copy(std::env::current_exe().unwrap(), &destination).unwrap();
                let child = std::process::Command::new(&destination)
                    .args(["--exact", "update::tests::downloads_from_static_manifest_and_checks_digest_before_replacing"])
                    .env(CHILD_URL, &url)
                    .output()
                    .unwrap();
                assert!(
                    child.status.success(),
                    "{}\n{}",
                    String::from_utf8_lossy(&child.stdout),
                    String::from_utf8_lossy(&child.stderr)
                );
            } else {
                fs::write(&destination, b"original").unwrap();
                let release = check_static(&url).unwrap().unwrap();
                assert!(
                    install(release, &destination, |event| {
                        assert_ne!(event, InstallProgress::Applying);
                    })
                    .is_err()
                );
            }
            #[cfg(windows)]
            assert!(!destination.with_extension("exe.update-lock").exists());
            assert_eq!(
                std::fs::read(destination).unwrap(),
                if should_install {
                    b"hello".as_slice()
                } else {
                    b"original"
                }
            );
            server.join().unwrap();
        }
    }
}

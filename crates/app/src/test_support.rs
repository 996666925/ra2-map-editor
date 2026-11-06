use eframe::egui;

/// Empty startup archives plus loose synthetic images exercise Install::open.
pub fn map_loader_fixture() -> ra2_map_assets::test_support::Fixture {
    let fixture = ra2_map_assets::test_support::Fixture::new();
    // New-format MIX: zero marker, flags, entry count and body size.
    for archive in [
        "ra2md.mix",
        "ra2.mix",
        "cachemd.mix",
        "cache.mix",
        "localmd.mix",
        "local.mix",
        "conqmd.mix",
        "conquer.mix",
        "cameomd.mix",
        "cameo.mix",
        "mapsmd03.mix",
        "multimd.mix",
        "movmd03.mix",
    ] {
        std::fs::write(fixture.0.join(archive), [0u8; 10]).unwrap();
    }
    fixture
}

pub fn harness() -> egui_kittest::HarnessBuilder {
    let builder = egui_kittest::Harness::builder().with_context_setup(crate::ui_theme::install);
    #[cfg(feature = "ui-test-video")]
    let builder = builder.with_render_cursor(true);
    builder
}

/// Run a CPU-only UI frame, explicitly discarding texture uploads without a GPU.
pub fn run_ui(
    ctx: &egui::Context,
    input: egui::RawInput,
    add_contents: impl FnMut(&mut egui::Ui),
) -> egui::FullOutput {
    let mut output = ctx.run_ui(input, add_contents);
    output.textures_delta.clear();
    output
}

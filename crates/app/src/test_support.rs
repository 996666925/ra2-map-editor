use eframe::egui;

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

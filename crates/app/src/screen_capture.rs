//! Keep asynchronous screenshot callbacks available to the smoke state machines.
use std::sync::{Arc, Mutex};

use eframe::egui;

type PendingCapture = Arc<Mutex<Option<Arc<egui::ColorImage>>>>;

fn id(ctx: &egui::Context) -> egui::Id {
    egui::Id::unique(("screen-capture", ctx.viewport_id()))
}

pub fn request(ctx: &egui::Context) {
    let id = id(ctx);
    let pending = PendingCapture::default();
    ctx.data_mut(|data| data.insert_temp(id, pending.clone()));
    let repaint = ctx.clone();
    ctx.request_screenshot(move |image| {
        *pending.lock().expect("screen capture") = Some(image);
        repaint.request_repaint();
    });
}

pub fn take(ctx: &egui::Context) -> Option<Arc<egui::ColorImage>> {
    let id = id(ctx);
    let pending = ctx.data(|data| data.get_temp::<PendingCapture>(id))?;
    let image = pending.lock().expect("screen capture").take()?;
    ctx.data_mut(|data| data.remove::<PendingCapture>(id));
    Some(image)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request_callback(ctx: &egui::Context) -> egui::ScreenshotCallback {
        let output = crate::test_support::run_ui(ctx, Default::default(), |ui| request(ui.ctx()));
        output
            .viewport_output
            .into_values()
            .flat_map(|viewport| viewport.commands)
            .find_map(|command| match command {
                egui::ViewportCommand::Screenshot(callback) => Some(callback),
                _ => None,
            })
            .unwrap()
    }

    #[test]
    fn delayed_capture_is_consumed_once_and_replaced_requests_are_ignored() {
        let ctx = egui::Context::default();
        let old = request_callback(&ctx);
        let current = request_callback(&ctx);
        assert!(take(&ctx).is_none());
        let image = Arc::new(egui::ColorImage::filled([2, 2], egui::Color32::RED));
        old.complete(image.clone());
        assert!(take(&ctx).is_none());
        current.complete(image.clone());
        assert!(Arc::ptr_eq(&take(&ctx).unwrap(), &image));
        assert!(take(&ctx).is_none());
    }
}

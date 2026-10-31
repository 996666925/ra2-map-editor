use eframe::egui::{self, Color32, FontId, Pos2, Rect, RichText, Stroke, Vec2};
use egui::class::HasClasses as _;
use egui::theme::{DefaultStyle, StyleProvider};
use egui::widget_style::{ButtonStyle, StyleArgs, WidgetState};

pub const BACKGROUND: Color32 = Color32::from_rgb(17, 21, 26);
pub const PANEL: Color32 = Color32::from_rgb(25, 30, 36);
pub const SURFACE: Color32 = Color32::from_rgb(33, 39, 47);
pub const BORDER: Color32 = Color32::from_rgb(49, 58, 68);
pub const TEXT: Color32 = Color32::from_rgb(220, 227, 234);
pub const MUTED: Color32 = Color32::from_rgb(139, 154, 168);
pub const ACCENT: Color32 = Color32::from_rgb(229, 180, 93);
pub const GREEN: Color32 = Color32::from_rgb(115, 196, 156);
pub const BLUE: Color32 = Color32::from_rgb(115, 180, 212);

pub fn install(ctx: &egui::Context) {
    ctx.set_theme(egui::Theme::Dark);
    let mut style = (*ctx.global_style()).clone();
    style
        .text_styles
        .insert(egui::TextStyle::Body, FontId::proportional(13.));
    style
        .text_styles
        .insert(egui::TextStyle::Button, FontId::proportional(13.));
    style
        .text_styles
        .insert(egui::TextStyle::Small, FontId::proportional(11.));
    style
        .text_styles
        .insert(egui::TextStyle::Heading, FontId::proportional(17.));
    style
        .text_styles
        .insert(egui::TextStyle::Monospace, FontId::monospace(12.));
    style.spacing.item_spacing = egui::vec2(8., 6.);
    style.spacing.button_padding = egui::vec2(10., 6.);
    style.spacing.interact_size = egui::vec2(28., 28.);
    style.spacing.text_edit_width = 180.;
    style.spacing.combo_width = 180.;
    style.spacing.slider_width = 115.;
    style.spacing.window_margin = egui::Margin::same(12);
    let mut v = egui::Visuals::dark();
    v.panel_fill = PANEL;
    v.window_fill = PANEL;
    v.extreme_bg_color = BACKGROUND;
    v.faint_bg_color = SURFACE;
    v.code_bg_color = SURFACE;
    v.weak_text_color = Some(MUTED);
    v.selection.bg_fill = Color32::from_rgb(77, 64, 43);
    v.selection.stroke = Stroke::new(1., ACCENT);
    v.hyperlink_color = BLUE;
    v.warn_fg_color = ACCENT;
    v.error_fg_color = Color32::from_rgb(235, 119, 119);
    v.window_stroke = Stroke::new(1., BORDER);
    v.window_corner_radius = 8.into();
    v.menu_corner_radius = 6.into();
    for widget in [
        &mut v.widgets.noninteractive,
        &mut v.widgets.inactive,
        &mut v.widgets.hovered,
        &mut v.widgets.active,
        &mut v.widgets.open,
    ] {
        widget.corner_radius = 5.into();
        widget.expansion = 0.;
        widget.bg_stroke = Stroke::new(1., BORDER);
        widget.fg_stroke = Stroke::new(1.3, TEXT);
    }
    v.widgets.noninteractive.bg_fill = PANEL;
    v.widgets.noninteractive.weak_bg_fill = PANEL;
    v.widgets.inactive.bg_fill = SURFACE;
    v.widgets.inactive.weak_bg_fill = SURFACE;
    v.widgets.hovered.bg_fill = Color32::from_rgb(45, 53, 63);
    v.widgets.hovered.weak_bg_fill = v.widgets.hovered.bg_fill;
    v.widgets.hovered.bg_stroke = Stroke::new(1., MUTED);
    v.widgets.active.bg_fill = Color32::from_rgb(74, 60, 39);
    v.widgets.active.weak_bg_fill = v.widgets.active.bg_fill;
    v.widgets.active.fg_stroke = Stroke::new(1.3, ACCENT);
    v.widgets.open = v.widgets.hovered;
    style.visuals = v;
    ctx.set_global_style(style);
    ctx.add_widget_theme::<ButtonStyle>(EditorButtons);
}

struct EditorButtons;
impl StyleProvider<ButtonStyle> for EditorButtons {
    fn style(&mut self, args: &StyleArgs<'_>) -> ButtonStyle {
        let mut style: ButtonStyle = DefaultStyle.style(args);
        if args.has_class("editor-primary") {
            style.atom_layout.frame.fill = match args.state {
                WidgetState::Hovered => ACCENT.gamma_multiply(1.1),
                WidgetState::Active => ACCENT.gamma_multiply(0.85),
                _ => ACCENT,
            };
            style.atom_layout.frame.stroke = Stroke::NONE;
            style.atom_layout.text_style.color = BACKGROUND;
        }
        style
    }
}
pub fn panel_frame() -> egui::Frame {
    egui::Frame::new()
        .fill(PANEL)
        .inner_margin(12)
        .stroke(Stroke::new(1., BORDER))
}

/// Keep dialogs usable after a resize, including with remembered window sizes.
pub fn window<'a>(ctx: &egui::Context, title: &str, id: &str, size: Vec2) -> egui::Window<'a> {
    let bounds = ctx.content_rect().shrink(12.);
    let max = (bounds.size() - egui::vec2(26., 54.)).max(egui::vec2(240., 160.));
    let size = size.min(max);
    egui::Window::new(title)
        .id(egui::Id::unique(id))
        .collapsible(false)
        .default_size(size)
        .min_size(egui::vec2(320., 180.).min(size))
        .max_size(max)
        .default_pos(bounds.center() - (size + egui::vec2(24., 48.)) * 0.5)
        .constrain_to(bounds)
}

/// An anchored Area positions itself using the previous frame's size. Repaint
/// when a form changes size so it settles at the center after a viewport resize.
pub fn modal<R>(
    ctx: &egui::Context,
    id: &str,
    content: impl FnOnce(&mut egui::Ui) -> R,
) -> egui::ModalResponse<R> {
    let id = egui::Id::unique(id);
    let previous_size = ctx.memory(|m| m.area_rect(id)).map(|r| r.size());
    let response = egui::Modal::new(id).show(ctx, content);
    if previous_size != Some(response.response.rect.size()) {
        ctx.request_repaint();
    }
    response
}

pub fn primary(text: &str) -> egui::Button<'_> {
    egui::Button::new(RichText::new(text).color(BACKGROUND).strong())
        .with_class("editor-primary")
        .min_size(egui::vec2(90., 30.))
}

/// Right-align a single row without consuming the parent's remaining height.
pub fn right_to_left<R>(
    ui: &mut egui::Ui,
    add_contents: impl FnOnce(&mut egui::Ui) -> R,
) -> egui::InnerResponse<R> {
    ui.allocate_ui_with_layout(
        egui::vec2(ui.available_width(), ui.spacing().interact_size.y),
        egui::Layout::right_to_left(egui::Align::Center),
        add_contents,
    )
}

pub fn path_entry(ui: &mut egui::Ui, value: &mut String, hint: &str, browse: &str) -> bool {
    ui.horizontal(|ui| {
        let width = (ui.available_width() - 76.).max(100.);
        ui.add_sized(
            [width, 28.],
            egui::TextEdit::singleline(value).hint_text(hint),
        )
        .accessible_name(hint)
        .on_hover_text(value.as_str());
        ui.add_sized([68., 28.], egui::Button::new(browse))
            .clicked()
    })
    .inner
}

/// A fixed thumbnail slot keeps rows aligned even for missing or tiny images.
pub fn thumbnail(ui: &mut egui::Ui, texture: Option<&egui::TextureHandle>, detail: &str) {
    let (rect, response) = ui.allocate_exact_size(egui::vec2(44., 40.), egui::Sense::hover());
    let p = ui.painter().with_clip_rect(rect);
    p.rect_filled(rect, 4, BACKGROUND);
    if let Some(texture) = texture {
        let size = texture.size_vec2();
        let scale = (40. / size.x.max(1.)).min(36. / size.y.max(1.)).min(1.);
        p.image(
            texture.id(),
            Rect::from_center_size(rect.center(), size * scale),
            Rect::from_min_max(Pos2::ZERO, egui::pos2(1., 1.)),
            Color32::WHITE,
        );
    } else {
        p.text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            "—",
            FontId::proportional(14.),
            MUTED,
        );
    }
    response.on_hover_text(detail);
}
pub fn section(ui: &mut egui::Ui, title: &str, detail: &str) {
    ui.horizontal(|ui| {
        ui.label(RichText::new(title).strong().size(13.));
        if !detail.is_empty() {
            right_to_left(ui, |ui| {
                ui.label(RichText::new(detail).size(10.).color(MUTED));
            });
        }
    });
    ui.add_space(2.);
}
pub fn field(ui: &mut egui::Ui, name: &str, value: impl ToString) {
    let value = value.to_string();
    ui.horizontal(|ui| {
        ui.label(RichText::new(name).color(MUTED).size(12.));
        right_to_left(ui, |ui| {
            ui.add(egui::Label::new(RichText::new(&value).size(12.)).truncate())
                .on_hover_text(&value);
        });
    });
}
pub fn badge(ui: &mut egui::Ui, text: &str, color: Color32) {
    egui::Frame::new()
        .fill(color.gamma_multiply(0.15))
        .corner_radius(4)
        .inner_margin(egui::Margin::symmetric(7, 3))
        .show(ui, |ui| {
            ui.label(RichText::new(text).color(color).size(10.));
        });
}
#[derive(Clone, Copy)]
pub enum Icon {
    Select,
    Terrain,
    Ore,
    Gem,
    Erase,
    Building,
    Move,
    Flag,
    Pick,
    Folder,
    Save,
    Undo,
    Redo,
    Play,
}
pub fn icon(p: &egui::Painter, rect: Rect, kind: Icon, color: Color32) {
    let r = Rect::from_center_size(rect.center(), Vec2::splat(16.));
    let at = |x: f32, y: f32| r.min + egui::vec2(x, y);
    let stroke = Stroke::new(1.4, color);
    let line = |points: &[[f32; 2]]| {
        p.add(egui::Shape::line(
            points.iter().map(|a| at(a[0], a[1])).collect(),
            stroke,
        ));
    };
    match kind {
        Icon::Select => {
            line(&[
                [3., 1.],
                [3., 14.],
                [7., 10.],
                [10., 15.],
                [12., 14.],
                [9., 9.],
                [14., 8.],
                [3., 1.],
            ]);
        }
        Icon::Terrain => {
            line(&[[1., 6.], [8., 2.], [15., 6.], [8., 10.], [1., 6.]]);
            line(&[[1., 10.], [8., 14.], [15., 10.]]);
        }
        Icon::Ore | Icon::Gem => {
            line(&[
                [2., 5.],
                [5., 2.],
                [11., 2.],
                [14., 5.],
                [8., 14.],
                [2., 5.],
                [14., 5.],
            ]);
            line(&[[5., 2.], [6., 5.], [8., 14.], [10., 5.], [11., 2.]]);
        }
        Icon::Erase => {
            line(&[
                [2., 9.],
                [9., 2.],
                [15., 8.],
                [8., 15.],
                [6., 15.],
                [2., 11.],
                [2., 9.],
            ]);
            line(&[[6., 5.], [12., 11.]]);
        }
        Icon::Building => {
            line(&[
                [2., 15.],
                [2., 6.],
                [8., 2.],
                [14., 6.],
                [14., 15.],
                [2., 15.],
            ]);
            line(&[[6., 15.], [6., 10.], [10., 10.], [10., 15.]]);
        }
        Icon::Move => {
            line(&[[8., 1.], [8., 15.]]);
            line(&[[1., 8.], [15., 8.]]);
            line(&[[5., 4.], [8., 1.], [11., 4.]]);
            line(&[[5., 12.], [8., 15.], [11., 12.]]);
            line(&[[4., 5.], [1., 8.], [4., 11.]]);
            line(&[[12., 5.], [15., 8.], [12., 11.]]);
        }
        Icon::Flag => {
            line(&[
                [3., 15.],
                [3., 2.],
                [13., 2.],
                [10., 6.],
                [13., 9.],
                [3., 9.],
            ]);
        }
        Icon::Pick => {
            line(&[
                [2., 13.],
                [3., 9.],
                [11., 1.],
                [15., 5.],
                [7., 13.],
                [2., 14.],
            ]);
            line(&[[9., 3.], [13., 7.]]);
        }
        Icon::Folder => {
            line(&[
                [1., 13.],
                [1., 3.],
                [6., 3.],
                [8., 5.],
                [15., 5.],
                [14., 13.],
                [1., 13.],
            ]);
        }
        Icon::Save => {
            line(&[
                [2., 2.],
                [12., 2.],
                [15., 5.],
                [15., 14.],
                [2., 14.],
                [2., 2.],
            ]);
            line(&[[5., 2.], [5., 6.], [11., 6.], [11., 2.]]);
            line(&[[5., 14.], [5., 9.], [12., 9.], [12., 14.]]);
        }
        Icon::Undo => {
            line(&[[6., 2.], [2., 6.], [6., 10.]]);
            line(&[[2., 6.], [10., 6.], [14., 9.], [14., 14.]]);
        }
        Icon::Redo => {
            line(&[[10., 2.], [14., 6.], [10., 10.]]);
            line(&[[14., 6.], [6., 6.], [2., 9.], [2., 14.]]);
        }
        Icon::Play => {
            p.add(egui::Shape::convex_polygon(
                vec![at(4., 2.), at(14., 8.), at(4., 14.)],
                color,
                Stroke::NONE,
            ));
        }
    }
}
pub fn button(
    ui: &mut egui::Ui,
    kind: Icon,
    text: &str,
    width: f32,
    selected: bool,
) -> egui::Response {
    let name = if text.is_empty() {
        match kind {
            Icon::Undo => "撤销",
            Icon::Redo => "重做",
            Icon::Select => "选择",
            Icon::Terrain => "地形",
            Icon::Ore => "矿石",
            Icon::Gem => "宝石",
            Icon::Erase => "擦除",
            Icon::Building => "建筑",
            Icon::Flag => "航点",
            Icon::Move => "移动",
            Icon::Pick => "吸管",
            Icon::Folder => "打开",
            Icon::Save => "保存",
            Icon::Play => "试玩",
        }
    } else {
        text
    };
    let response = ui
        .push_id(("icon-button", kind as u8, text), |ui| {
            ui.add_sized([width, 32.], egui::Button::new("").selected(selected))
        })
        .inner;
    let color = if !ui.is_enabled() {
        MUTED.gamma_multiply(0.4)
    } else if selected {
        ACCENT
    } else {
        TEXT
    };
    let p = ui.painter().with_clip_rect(response.rect);
    icon(
        &p,
        Rect::from_center_size(
            if text.is_empty() {
                response.rect.center()
            } else {
                response.rect.left_center() + egui::vec2(19., 0.)
            },
            Vec2::splat(18.),
        ),
        kind,
        color,
    );
    if !text.is_empty() {
        p.text(
            Pos2::new(response.rect.left() + 35., response.rect.center().y),
            egui::Align2::LEFT_CENTER,
            text,
            FontId::proportional(12.),
            color,
        );
    }
    response.widget_info(|| {
        egui::WidgetInfo::selected(egui::Role::Button, ui.is_enabled(), selected, name)
    });
    response.accessible_name(name)
}

#[cfg(all(test, feature = "ui-test-video"))]
mod tests {
    #[test]
    fn primary_button_text_is_visible_with_the_editor_theme() {
        let rect = std::cell::Cell::new(eframe::egui::Rect::NOTHING);
        let mut harness = crate::test_support::harness().build_ui(|ui| {
            rect.set(ui.add(super::primary("Create map")).rect);
        });
        harness.run();
        let image = harness.render().unwrap();
        let inside = rect.get().shrink(8.);
        let dark_pixels = image
            .enumerate_pixels()
            .filter(|(x, y, pixel)| {
                inside.contains(eframe::egui::pos2(*x as f32, *y as f32))
                    && pixel[0] < 80
                    && pixel[1] < 80
                    && pixel[2] < 80
            })
            .count();
        assert!(
            dark_pixels > 20,
            "primary button text disappeared: {dark_pixels} dark pixels"
        );
    }
}

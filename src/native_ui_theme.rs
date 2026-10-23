use eframe::egui;

pub const BG: egui::Color32 = egui::Color32::from_rgb(8, 10, 8);
pub const BG_DEEP: egui::Color32 = egui::Color32::from_rgb(5, 7, 5);
pub const PANEL: egui::Color32 = egui::Color32::from_rgb(14, 17, 14);
pub const PANEL_ALT: egui::Color32 = egui::Color32::from_rgb(19, 23, 19);
pub const HOVER: egui::Color32 = egui::Color32::from_rgb(27, 33, 27);
pub const ACTIVE: egui::Color32 = egui::Color32::from_rgb(38, 46, 38);
pub const LINE: egui::Color32 = egui::Color32::from_rgb(57, 64, 57);
pub const LINE_STRONG: egui::Color32 = egui::Color32::from_rgb(104, 114, 104);
pub const TEXT: egui::Color32 = egui::Color32::from_rgb(231, 236, 231);
pub const MUTED: egui::Color32 = egui::Color32::from_rgb(155, 164, 155);
pub const OK: egui::Color32 = egui::Color32::from_rgb(184, 216, 184);
pub const WARN: egui::Color32 = egui::Color32::from_rgb(216, 197, 143);
pub const BAD: egui::Color32 = egui::Color32::from_rgb(215, 162, 162);

pub const SPACE_XS: f32 = 4.0;
pub const SPACE_SM: f32 = 8.0;
pub const SPACE_MD: f32 = 12.0;
pub const SPACE_LG: f32 = 16.0;
pub const ICON_SIZE: f32 = 13.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PixelIcon {
    Home,
    Tools,
    Files,
    Editor,
    Workspace,
    Mobile,
    System,
    Hash,
    Compare,
    Repository,
    Backup,
    Folder,
    File,
    Link,
    Other,
}

impl PixelIcon {
    fn rows(self) -> [u8; 5] {
        match self {
            Self::Home => [0b00100, 0b01110, 0b11111, 0b10101, 0b11111],
            Self::Tools => [0b10101, 0b01110, 0b11111, 0b01110, 0b10101],
            Self::Files => [0b11000, 0b11111, 0b10001, 0b10001, 0b11111],
            Self::Editor => [0b11110, 0b10010, 0b10110, 0b11000, 0b11111],
            Self::Workspace => [0b10001, 0b11111, 0b10101, 0b11111, 0b10001],
            Self::Mobile => [0b01110, 0b10001, 0b10101, 0b10001, 0b01110],
            Self::System => [0b10101, 0b01110, 0b11011, 0b01110, 0b10101],
            Self::Hash => [0b01010, 0b11111, 0b01010, 0b11111, 0b01010],
            Self::Compare => [0b11000, 0b00110, 0b11111, 0b01100, 0b00011],
            Self::Repository => [0b00100, 0b01110, 0b10101, 0b00100, 0b11111],
            Self::Backup => [0b01110, 0b10001, 0b10111, 0b10001, 0b01110],
            Self::Folder => [0b11000, 0b11111, 0b10001, 0b10001, 0b11111],
            Self::File => [0b11100, 0b10110, 0b10010, 0b10010, 0b11110],
            Self::Link => [0b01100, 0b10010, 0b00100, 0b01001, 0b00110],
            Self::Other => [0b00100, 0b00000, 0b00100, 0b00000, 0b00100],
        }
    }
}

pub fn install(context: &egui::Context) {
    let mut visuals = egui::Visuals::dark();
    visuals.panel_fill = BG;
    visuals.window_fill = PANEL;
    visuals.extreme_bg_color = BG_DEEP;
    visuals.text_edit_bg_color = Some(BG_DEEP);
    visuals.override_text_color = Some(TEXT);
    visuals.warn_fg_color = WARN;
    visuals.error_fg_color = BAD;
    visuals.window_corner_radius = egui::CornerRadius::same(0);
    visuals.menu_corner_radius = egui::CornerRadius::same(0);
    visuals.window_stroke = egui::Stroke::new(1.0, LINE_STRONG);

    let square = egui::CornerRadius::same(0);

    visuals.widgets.noninteractive.weak_bg_fill = PANEL;
    visuals.widgets.noninteractive.bg_fill = PANEL;
    visuals.widgets.noninteractive.bg_stroke = egui::Stroke::new(1.0, LINE);
    visuals.widgets.noninteractive.fg_stroke = egui::Stroke::new(1.0, MUTED);
    visuals.widgets.noninteractive.corner_radius = square;

    visuals.widgets.inactive.weak_bg_fill = PANEL_ALT;
    visuals.widgets.inactive.bg_fill = PANEL_ALT;
    visuals.widgets.inactive.bg_stroke = egui::Stroke::new(1.0, LINE);
    visuals.widgets.inactive.fg_stroke = egui::Stroke::new(1.0, TEXT);
    visuals.widgets.inactive.corner_radius = square;

    visuals.widgets.hovered.weak_bg_fill = HOVER;
    visuals.widgets.hovered.bg_fill = HOVER;
    visuals.widgets.hovered.bg_stroke = egui::Stroke::new(1.0, LINE_STRONG);
    visuals.widgets.hovered.fg_stroke = egui::Stroke::new(1.0, TEXT);
    visuals.widgets.hovered.corner_radius = square;
    visuals.widgets.hovered.expansion = 0.0;

    visuals.widgets.active.weak_bg_fill = ACTIVE;
    visuals.widgets.active.bg_fill = ACTIVE;
    visuals.widgets.active.bg_stroke = egui::Stroke::new(1.0, TEXT);
    visuals.widgets.active.fg_stroke = egui::Stroke::new(1.0, TEXT);
    visuals.widgets.active.corner_radius = square;
    visuals.widgets.active.expansion = 0.0;

    visuals.widgets.open.weak_bg_fill = ACTIVE;
    visuals.widgets.open.bg_fill = PANEL;
    visuals.widgets.open.bg_stroke = egui::Stroke::new(1.0, LINE_STRONG);
    visuals.widgets.open.fg_stroke = egui::Stroke::new(1.0, TEXT);
    visuals.widgets.open.corner_radius = square;

    context.set_visuals(visuals);
    context.all_styles_mut(|style| {
        style.spacing.item_spacing = egui::vec2(SPACE_SM, SPACE_SM);
        style.spacing.button_padding = egui::vec2(10.0, 6.0);
        style.spacing.window_margin = egui::Margin::same(SPACE_MD as i8);
        style.spacing.menu_margin = egui::Margin::same(SPACE_SM as i8);
    });
}

pub fn panel(ui: &mut egui::Ui, contents: impl FnOnce(&mut egui::Ui)) {
    egui::Frame::new()
        .fill(PANEL)
        .stroke(egui::Stroke::new(1.0, LINE))
        .corner_radius(0)
        .inner_margin(egui::Margin::same(SPACE_MD as i8))
        .show(ui, contents);
}

pub fn panel_alt(ui: &mut egui::Ui, contents: impl FnOnce(&mut egui::Ui)) {
    egui::Frame::new()
        .fill(PANEL_ALT)
        .stroke(egui::Stroke::new(1.0, LINE))
        .corner_radius(0)
        .inner_margin(egui::Margin::same(10))
        .show(ui, contents);
}

pub fn section_title(ui: &mut egui::Ui, icon: PixelIcon, text: &str) {
    ui.horizontal(|ui| {
        pixel_icon(ui, icon, TEXT);
        ui.label(
            egui::RichText::new(text)
                .monospace()
                .strong()
                .size(15.0)
                .color(TEXT),
        );
    });
}

pub fn small_label(ui: &mut egui::Ui, text: &str) {
    ui.label(
        egui::RichText::new(text)
            .monospace()
            .size(11.0)
            .color(MUTED),
    );
}

pub fn key_value(ui: &mut egui::Ui, key: &str, value: &str) {
    ui.horizontal(|ui| {
        ui.add_sized(
            [150.0, 20.0],
            egui::Label::new(egui::RichText::new(key).color(MUTED).monospace()),
        );
        ui.label(egui::RichText::new(value).monospace());
    });
}

pub fn metric(ui: &mut egui::Ui, icon: PixelIcon, label: &str, value: &str, detail: &str) {
    panel_alt(ui, |ui| {
        ui.horizontal(|ui| {
            pixel_icon(ui, icon, MUTED);
            small_label(ui, label);
        });
        ui.label(
            egui::RichText::new(value)
                .monospace()
                .strong()
                .size(19.0)
                .color(TEXT),
        );
        ui.label(egui::RichText::new(detail).color(MUTED).monospace());
    });
}

pub fn pixel_button(
    ui: &mut egui::Ui,
    icon: PixelIcon,
    label: &str,
    selected: bool,
    enabled: bool,
    min_width: f32,
) -> egui::Response {
    let button = egui::Button::selectable(
        selected,
        egui::RichText::new(format!("    {label}")).monospace(),
    )
    .corner_radius(0)
    .min_size(egui::vec2(min_width, 28.0));

    let response = ui.add_enabled(enabled, button);
    let color = if !enabled {
        MUTED
    } else if selected || response.hovered() {
        TEXT
    } else {
        OK
    };

    paint_icon_in_button(ui, response.rect, icon, color);
    response
}

pub fn status_badge(ui: &mut egui::Ui, text: &str, color: egui::Color32) {
    egui::Frame::new()
        .fill(BG_DEEP)
        .stroke(egui::Stroke::new(1.0, color))
        .corner_radius(0)
        .inner_margin(egui::Margin::symmetric(6, 2))
        .show(ui, |ui| {
            ui.label(
                egui::RichText::new(text)
                    .monospace()
                    .strong()
                    .size(10.0)
                    .color(color),
            );
        });
}

pub fn pixel_icon(ui: &mut egui::Ui, icon: PixelIcon, color: egui::Color32) {
    let (rect, _) = ui.allocate_exact_size(
        egui::vec2(ICON_SIZE, ICON_SIZE),
        egui::Sense::hover(),
    );
    paint_icon(ui, rect, icon, color);
}

fn paint_icon_in_button(
    ui: &egui::Ui,
    rect: egui::Rect,
    icon: PixelIcon,
    color: egui::Color32,
) {
    let size = 11.0;
    let icon_rect = egui::Rect::from_center_size(
        egui::pos2(rect.left() + 14.0, rect.center().y),
        egui::vec2(size, size),
    );
    paint_icon(ui, icon_rect, icon, color);
}

fn paint_icon(ui: &egui::Ui, rect: egui::Rect, icon: PixelIcon, color: egui::Color32) {
    if !ui.is_rect_visible(rect) {
        return;
    }

    let rows = icon.rows();
    let cell = (rect.width().min(rect.height()) / 5.0).floor().max(1.0);
    let extent = cell * 5.0;
    let origin = egui::pos2(
        rect.center().x - extent / 2.0,
        rect.center().y - extent / 2.0,
    );

    for (y, row) in rows.iter().enumerate() {
        for x in 0..5 {
            let bit = 1 << (4 - x);
            if row & bit == 0 {
                continue;
            }

            let min = egui::pos2(origin.x + x as f32 * cell, origin.y + y as f32 * cell);
            let pixel = egui::Rect::from_min_size(min, egui::vec2(cell, cell));
            ui.painter().rect_filled(pixel, 0.0, color);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_icon_has_at_least_one_pixel() {
        for icon in [
            PixelIcon::Home,
            PixelIcon::Tools,
            PixelIcon::Files,
            PixelIcon::Editor,
            PixelIcon::Workspace,
            PixelIcon::Mobile,
            PixelIcon::System,
            PixelIcon::Hash,
            PixelIcon::Compare,
            PixelIcon::Repository,
            PixelIcon::Backup,
            PixelIcon::Folder,
            PixelIcon::File,
            PixelIcon::Link,
            PixelIcon::Other,
        ] {
            assert!(icon.rows().iter().any(|row| *row != 0));
        }
    }

    #[test]
    fn icon_rows_fit_five_columns() {
        for icon in [
            PixelIcon::Home,
            PixelIcon::Tools,
            PixelIcon::Files,
            PixelIcon::Editor,
            PixelIcon::Workspace,
            PixelIcon::Mobile,
            PixelIcon::System,
            PixelIcon::Hash,
            PixelIcon::Compare,
            PixelIcon::Repository,
            PixelIcon::Backup,
            PixelIcon::Folder,
            PixelIcon::File,
            PixelIcon::Link,
            PixelIcon::Other,
        ] {
            assert!(icon.rows().iter().all(|row| *row <= 0b1_1111));
        }
    }
}

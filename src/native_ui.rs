use crate::workspace;
use eframe::egui;
use std::path::PathBuf;

const BG: egui::Color32 = egui::Color32::from_rgb(8, 10, 8);
const PANEL: egui::Color32 = egui::Color32::from_rgb(14, 17, 14);
const PANEL_ALT: egui::Color32 = egui::Color32::from_rgb(19, 23, 19);
const LINE: egui::Color32 = egui::Color32::from_rgb(57, 64, 57);
const TEXT: egui::Color32 = egui::Color32::from_rgb(231, 236, 231);
const MUTED: egui::Color32 = egui::Color32::from_rgb(155, 164, 155);
const OK: egui::Color32 = egui::Color32::from_rgb(184, 216, 184);
const WARN: egui::Color32 = egui::Color32::from_rgb(216, 197, 143);

pub fn run() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("DANGOMUSHI")
            .with_inner_size([960.0, 640.0])
            .with_min_inner_size([720.0, 480.0]),
        ..Default::default()
    };

    eframe::run_native(
        "dangomushi",
        options,
        Box::new(|creation_context| Ok(Box::new(NativeApp::new(creation_context)))),
    )
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum View {
    Home,
    Workspace,
    Mobile,
    System,
}

struct NativeApp {
    view: View,
    workspace_root: PathBuf,
    notice: String,
}

impl NativeApp {
    fn new(creation_context: &eframe::CreationContext<'_>) -> Self {
        let mut visuals = egui::Visuals::dark();
        visuals.panel_fill = BG;
        visuals.window_fill = PANEL;
        visuals.override_text_color = Some(TEXT);
        creation_context.egui_ctx.set_visuals(visuals);

        creation_context.egui_ctx.all_styles_mut(|style| {
            style.spacing.item_spacing = egui::vec2(8.0, 8.0);
            style.spacing.button_padding = egui::vec2(10.0, 6.0);
        });

        Self {
            view: View::Home,
            workspace_root: workspace::default_workspace_root(),
            notice: "READY".to_string(),
        }
    }

    fn top_bar(&mut self, ui: &mut egui::Ui) {
        egui::Frame::new()
            .fill(PANEL)
            .stroke(egui::Stroke::new(1.0, LINE))
            .inner_margin(egui::Margin::same(10))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new("DANGOMUSHI")
                            .monospace()
                            .strong()
                            .size(17.0),
                    );
                    ui.label(egui::RichText::new("/ PORTABLE LOCAL WORKSPACE").color(MUTED));

                    ui.with_layout(
                        egui::Layout::right_to_left(egui::Align::Center),
                        |ui| {
                            ui.label(egui::RichText::new(&self.notice).color(OK).monospace());
                        },
                    );
                });
            });

        ui.add_space(6.0);

        egui::Frame::new()
            .fill(PANEL)
            .stroke(egui::Stroke::new(1.0, LINE))
            .inner_margin(egui::Margin::same(4))
            .show(ui, |ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.selectable_value(&mut self.view, View::Home, "[01] HOME");
                    ui.selectable_value(&mut self.view, View::Workspace, "[02] WORKSPACE");
                    ui.selectable_value(&mut self.view, View::Mobile, "[03] MOBILE");
                    ui.selectable_value(&mut self.view, View::System, "[04] SYSTEM");
                });
            });
    }

    fn home(&mut self, ui: &mut egui::Ui) {
        let summary = workspace::summary(&self.workspace_root);

        section_title(ui, "LOCAL CONTROL PANEL");
        ui.label(
            egui::RichText::new(
                "Native Rust shell for files, tools, recovery and trusted local access.",
            )
            .color(MUTED),
        );
        ui.add_space(12.0);

        ui.columns(3, |columns| {
            metric(
                &mut columns[0],
                "RUNTIME",
                env!("CARGO_PKG_VERSION"),
                "eframe / egui",
            );
            metric(&mut columns[1], "NETWORK", "LOCAL", "no LAN listener");
            metric(
                &mut columns[2],
                "WORKSPACE",
                if summary.initialized { "READY" } else { "NOT INIT" },
                &format!("{}/{} dirs", summary.present_dirs, summary.total_dirs),
            );
        });

        ui.add_space(10.0);
        panel(ui, |ui| {
            small_label(ui, "TOOLS");
            ui.add_space(4.0);

            for (name, state) in [
                ("FILES", "next"),
                ("EDITOR", "planned"),
                ("HASH / VERIFY", "planned"),
                ("COMPARE", "planned"),
                ("REPOSITORIES", "planned"),
                ("BACKUP", "planned"),
            ] {
                ui.horizontal(|ui| {
                    ui.add_enabled(
                        false,
                        egui::Button::new(name).min_size(egui::vec2(180.0, 28.0)),
                    );
                    ui.label(egui::RichText::new(state).color(MUTED).monospace());
                });
            }
        });
    }

    fn workspace(&mut self, ui: &mut egui::Ui) {
        let summary = workspace::summary(&self.workspace_root);

        section_title(ui, "WORKSPACE");
        panel(ui, |ui| {
            key_value(ui, "PATH", &self.workspace_root.display().to_string());
            key_value(
                ui,
                "STATE",
                if summary.initialized {
                    "READY"
                } else {
                    "NOT INITIALIZED"
                },
            );
            key_value(
                ui,
                "DIRECTORIES",
                &format!("{}/{}", summary.present_dirs, summary.total_dirs),
            );

            ui.add_space(8.0);

            if summary.initialized {
                ui.label(
                    egui::RichText::new("Workspace marker and required directories are present.")
                        .color(OK),
                );
            } else if ui.button("INITIALIZE WORKSPACE").clicked() {
                match workspace::initialize(&self.workspace_root) {
                    Ok(()) => self.notice = "WORKSPACE INITIALIZED".to_string(),
                    Err(error) => self.notice = format!("INIT FAILED: {error}"),
                }
            }
        });
    }

    fn mobile(&mut self, ui: &mut egui::Ui) {
        section_title(ui, "MOBILE ACCESS");
        panel(ui, |ui| {
            ui.label(
                egui::RichText::new("NOT ENABLED IN v0.2")
                    .color(WARN)
                    .strong()
                    .monospace(),
            );
            ui.add_space(6.0);
            ui.label(
                "LAN listening remains deliberately absent until device pairing, persistent trusted-device identity, revocation and permission boundaries are implemented.",
            );
        });
    }

    fn system(&mut self, ui: &mut egui::Ui) {
        section_title(ui, "SYSTEM");
        panel(ui, |ui| {
            key_value(ui, "DESKTOP UI", "eframe / egui 0.37");
            key_value(ui, "WEB GATEWAY", "127.0.0.1 only");
            key_value(ui, "PUBLIC INTERNET", "not exposed");
            key_value(ui, "EXTERNAL CDN", "none");
            key_value(ui, "WORKSPACE SOURCE", "local filesystem");
        });
    }
}

impl eframe::App for NativeApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        egui::CentralPanel::default()
            .frame(
                egui::Frame::new()
                    .fill(BG)
                    .inner_margin(egui::Margin::same(14)),
            )
            .show(ui, |ui| {
                self.top_bar(ui);
                ui.add_space(12.0);

                match self.view {
                    View::Home => self.home(ui),
                    View::Workspace => self.workspace(ui),
                    View::Mobile => self.mobile(ui),
                    View::System => self.system(ui),
                }
            });
    }
}

fn section_title(ui: &mut egui::Ui, text: &str) {
    ui.label(
        egui::RichText::new(text)
            .monospace()
            .strong()
            .size(15.0)
            .color(TEXT),
    );
}

fn panel(ui: &mut egui::Ui, contents: impl FnOnce(&mut egui::Ui)) {
    egui::Frame::new()
        .fill(PANEL)
        .stroke(egui::Stroke::new(1.0, LINE))
        .inner_margin(egui::Margin::same(12))
        .show(ui, contents);
}

fn metric(ui: &mut egui::Ui, label: &str, value: &str, detail: &str) {
    egui::Frame::new()
        .fill(PANEL_ALT)
        .stroke(egui::Stroke::new(1.0, LINE))
        .inner_margin(egui::Margin::same(10))
        .show(ui, |ui| {
            small_label(ui, label);
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

fn small_label(ui: &mut egui::Ui, text: &str) {
    ui.label(
        egui::RichText::new(text)
            .monospace()
            .size(11.0)
            .color(MUTED),
    );
}

fn key_value(ui: &mut egui::Ui, key: &str, value: &str) {
    ui.horizontal(|ui| {
        ui.add_sized(
            [150.0, 20.0],
            egui::Label::new(egui::RichText::new(key).color(MUTED).monospace()),
        );
        ui.label(egui::RichText::new(value).monospace());
    });
}

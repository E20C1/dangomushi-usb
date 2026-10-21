use crate::file_browser::{self, EntryKind, FileEntry};
use crate::workspace;
use eframe::egui;
use std::path::{Path, PathBuf};

const BG: egui::Color32 = egui::Color32::from_rgb(8, 10, 8);
const PANEL: egui::Color32 = egui::Color32::from_rgb(14, 17, 14);
const PANEL_ALT: egui::Color32 = egui::Color32::from_rgb(19, 23, 19);
const LINE: egui::Color32 = egui::Color32::from_rgb(57, 64, 57);
const TEXT: egui::Color32 = egui::Color32::from_rgb(231, 236, 231);
const MUTED: egui::Color32 = egui::Color32::from_rgb(155, 164, 155);
const OK: egui::Color32 = egui::Color32::from_rgb(184, 216, 184);
const WARN: egui::Color32 = egui::Color32::from_rgb(216, 197, 143);
const BAD: egui::Color32 = egui::Color32::from_rgb(215, 162, 162);

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
    Files,
    Workspace,
    Mobile,
    System,
}

struct FileBrowserState {
    current_dir: PathBuf,
    entries: Vec<FileEntry>,
    selected: Option<PathBuf>,
    error: Option<String>,
}

impl FileBrowserState {
    fn new(root: &Path) -> Self {
        let mut state = Self {
            current_dir: PathBuf::new(),
            entries: Vec::new(),
            selected: None,
            error: None,
        };
        state.refresh(root);
        state
    }

    fn refresh(&mut self, root: &Path) {
        match file_browser::list_directory(root, &self.current_dir) {
            Ok(entries) => {
                self.entries = entries;
                self.error = None;
            }
            Err(error) => {
                self.entries.clear();
                self.error = Some(error.to_string());
            }
        }
    }

    fn navigate(&mut self, root: &Path, destination: PathBuf) {
        match file_browser::list_directory(root, &destination) {
            Ok(entries) => {
                self.current_dir = destination;
                self.entries = entries;
                self.selected = None;
                self.error = None;
            }
            Err(error) => {
                self.error = Some(error.to_string());
            }
        }
    }

    fn up(&mut self, root: &Path) {
        if self.current_dir.as_os_str().is_empty() {
            return;
        }

        self.navigate(root, file_browser::parent(&self.current_dir));
    }
}

struct NativeApp {
    view: View,
    workspace_root: PathBuf,
    browser: FileBrowserState,
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

        let workspace_root = workspace::default_workspace_root();
        let browser = FileBrowserState::new(&workspace_root);

        Self {
            view: View::Home,
            workspace_root,
            browser,
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
                    ui.selectable_value(&mut self.view, View::Files, "[02] FILES");
                    ui.selectable_value(&mut self.view, View::Workspace, "[03] WORKSPACE");
                    ui.selectable_value(&mut self.view, View::Mobile, "[04] MOBILE");
                    ui.selectable_value(&mut self.view, View::System, "[05] SYSTEM");
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

            ui.horizontal(|ui| {
                if ui
                    .add(egui::Button::new("FILES").min_size(egui::vec2(180.0, 28.0)))
                    .clicked()
                {
                    self.browser.refresh(&self.workspace_root);
                    self.view = View::Files;
                }
                ui.label(egui::RichText::new("ready").color(OK).monospace());
            });

            for (name, state) in [
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

    fn files(&mut self, ui: &mut egui::Ui) {
        section_title(ui, "FILES");
        ui.label(
            egui::RichText::new("Read-only browser constrained to the DANGOMUSHI workspace root.")
                .color(MUTED),
        );
        ui.add_space(10.0);

        panel(ui, |ui| {
            ui.horizontal(|ui| {
                let at_root = self.browser.current_dir.as_os_str().is_empty();
                if ui.add_enabled(!at_root, egui::Button::new("[UP]")).clicked() {
                    self.browser.up(&self.workspace_root);
                }

                if ui.button("[REFRESH]").clicked() {
                    self.browser.refresh(&self.workspace_root);
                }

                ui.separator();
                ui.label(
                    egui::RichText::new(file_browser::display_path(&self.browser.current_dir))
                        .monospace()
                        .color(TEXT),
                );
            });
        });

        ui.add_space(8.0);

        if let Some(error) = self.browser.error.clone() {
            egui::Frame::new()
                .fill(PANEL)
                .stroke(egui::Stroke::new(1.0, BAD))
                .inner_margin(egui::Margin::same(10))
                .show(ui, |ui| {
                    ui.label(egui::RichText::new("BROWSER ERROR").color(BAD).strong());
                    ui.label(egui::RichText::new(error).color(MUTED).monospace());

                    if !self.workspace_root.is_dir()
                        && ui.button("INITIALIZE WORKSPACE").clicked()
                    {
                        match workspace::initialize(&self.workspace_root) {
                            Ok(()) => {
                                self.notice = "WORKSPACE INITIALIZED".to_string();
                                self.browser.current_dir.clear();
                                self.browser.refresh(&self.workspace_root);
                            }
                            Err(error) => {
                                self.notice = format!("INIT FAILED: {error}");
                            }
                        }
                    }
                });
            return;
        }

        let entries = self.browser.entries.clone();
        let mut navigate_to: Option<PathBuf> = None;

        egui::Frame::new()
            .fill(PANEL)
            .stroke(egui::Stroke::new(1.0, LINE))
            .inner_margin(egui::Margin::same(8))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.add_sized(
                        [80.0, 18.0],
                        egui::Label::new(egui::RichText::new("TYPE").color(MUTED).monospace()),
                    );
                    ui.add_sized(
                        [420.0, 18.0],
                        egui::Label::new(egui::RichText::new("NAME").color(MUTED).monospace()),
                    );
                    ui.label(egui::RichText::new("SIZE").color(MUTED).monospace());
                });
                ui.separator();

                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        if entries.is_empty() {
                            ui.label(egui::RichText::new("(empty directory)").color(MUTED));
                        }

                        for entry in entries {
                            let selected = self
                                .browser
                                .selected
                                .as_ref()
                                .is_some_and(|path| path == &entry.relative_path);

                            ui.horizontal(|ui| {
                                ui.add_sized(
                                    [80.0, 22.0],
                                    egui::Label::new(
                                        egui::RichText::new(entry.kind.label())
                                            .monospace()
                                            .color(kind_color(entry.kind)),
                                    ),
                                );

                                let response = ui.add_sized(
                                    [420.0, 22.0],
                                    egui::Button::selectable(
                                        selected,
                                        egui::RichText::new(&entry.name).monospace(),
                                    ),
                                );

                                if response.clicked() {
                                    self.browser.selected = Some(entry.relative_path.clone());
                                }

                                if response.double_clicked() && entry.kind == EntryKind::Directory {
                                    navigate_to = Some(entry.relative_path.clone());
                                }

                                let size = if entry.kind == EntryKind::File {
                                    format_bytes(entry.size_bytes)
                                } else {
                                    "-".to_string()
                                };
                                ui.label(egui::RichText::new(size).color(MUTED).monospace());
                            });
                        }
                    });
            });

        if let Some(destination) = navigate_to {
            self.browser.navigate(&self.workspace_root, destination);
        }

        ui.add_space(8.0);

        let selected_path = self.browser.selected.clone();
        let selected_entry = selected_path.as_ref().and_then(|selected| {
            self.browser
                .entries
                .iter()
                .find(|entry| &entry.relative_path == selected)
                .cloned()
        });
        let mut open_selected: Option<PathBuf> = None;

        panel(ui, |ui| {
            if let Some(selected) = &selected_path {
                key_value(ui, "SELECTED", &file_browser::display_path(selected));

                if let Some(entry) = &selected_entry {
                    key_value(ui, "TYPE", entry.kind.label());

                    if entry.kind == EntryKind::Directory {
                        if ui.button("OPEN DIRECTORY").clicked() {
                            open_selected = Some(entry.relative_path.clone());
                        }
                    } else if entry.kind == EntryKind::Symlink {
                        ui.label(
                            egui::RichText::new(
                                "Symlinks are visible but are never traversed by the browser.",
                            )
                            .color(WARN),
                        );
                    } else {
                        ui.label(
                            egui::RichText::new(
                                "File opening/editing arrives with the editor milestone.",
                            )
                            .color(MUTED),
                        );
                    }
                }
            } else {
                ui.label(egui::RichText::new("No item selected.").color(MUTED));
            }
        });

        if let Some(destination) = open_selected {
            self.browser.navigate(&self.workspace_root, destination);
        }
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
                    Ok(()) => {
                        self.notice = "WORKSPACE INITIALIZED".to_string();
                        self.browser.refresh(&self.workspace_root);
                    }
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
            key_value(ui, "FILE BROWSER", "workspace-rooted / read-only");
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
                    View::Files => self.files(ui),
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

fn kind_color(kind: EntryKind) -> egui::Color32 {
    match kind {
        EntryKind::Directory => OK,
        EntryKind::File => TEXT,
        EntryKind::Symlink => WARN,
        EntryKind::Other => MUTED,
    }
}

fn format_bytes(bytes: u64) -> String {
    const KIB: u64 = 1024;
    const MIB: u64 = KIB * 1024;
    const GIB: u64 = MIB * 1024;

    if bytes >= GIB {
        format!("{:.1} GiB", bytes as f64 / GIB as f64)
    } else if bytes >= MIB {
        format!("{:.1} MiB", bytes as f64 / MIB as f64)
    } else if bytes >= KIB {
        format!("{:.1} KiB", bytes as f64 / KIB as f64)
    } else {
        format!("{bytes} B")
    }
}

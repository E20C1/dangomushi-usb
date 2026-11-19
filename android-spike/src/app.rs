use crate::android_bridge::{AndroidBridge, StorageVolumeInfo};
use crate::contract::ContractReport;
use eframe::egui;
use std::fs;
use std::path::PathBuf;
use std::time::Duration;
use winit::platform::android::activity::AndroidApp;

pub fn run(android_app: AndroidApp) {
    let bridge_app = android_app.clone();
    let options = eframe::NativeOptions {
        android_app: Some(android_app),
        renderer: eframe::Renderer::Glow,
        ..Default::default()
    };

    if let Err(error) = eframe::run_native(
        "DANGOMUSHI Storage Contract Spike",
        options,
        Box::new(move |_cc| Ok(Box::new(StorageSpikeApp::new(bridge_app)))),
    ) {
        eprintln!("DANGOMUSHI Android storage spike failed: {error}");
    }
}

struct RemovalWatch {
    path: PathBuf,
}

struct StorageSpikeApp {
    bridge: AndroidBridge,
    all_files_access: Option<bool>,
    permission_error: Option<String>,
    volumes: Vec<StorageVolumeInfo>,
    discovery_error: Option<String>,
    selected: Option<usize>,
    report: Option<ContractReport>,
    removal_watch: Option<RemovalWatch>,
    removal_result: Option<String>,
}

impl StorageSpikeApp {
    fn new(android_app: AndroidApp) -> Self {
        let mut app = Self {
            bridge: AndroidBridge::new(android_app),
            all_files_access: None,
            permission_error: None,
            volumes: Vec::new(),
            discovery_error: None,
            selected: None,
            report: None,
            removal_watch: None,
            removal_result: None,
        };
        app.refresh();
        app
    }

    fn refresh(&mut self) {
        match self.bridge.is_all_files_access_granted() {
            Ok(granted) => {
                self.all_files_access = Some(granted);
                self.permission_error = None;
            }
            Err(error) => {
                self.all_files_access = None;
                self.permission_error = Some(error);
            }
        }

        let previous = self
            .selected
            .and_then(|index| self.volumes.get(index))
            .map(|volume| volume.directory.clone());

        match self.bridge.storage_volumes() {
            Ok(volumes) => {
                self.volumes = volumes;
                self.discovery_error = None;
                self.selected = previous.and_then(|path| {
                    self.volumes
                        .iter()
                        .position(|volume| volume.directory == path)
                });
                if self.selected.is_none() {
                    self.selected = self
                        .volumes
                        .iter()
                        .position(|volume| volume.removable && volume.state == "mounted");
                }
            }
            Err(error) => {
                self.volumes.clear();
                self.selected = None;
                self.discovery_error = Some(error);
            }
        }
    }

    fn selected_volume(&self) -> Option<&StorageVolumeInfo> {
        self.selected.and_then(|index| self.volumes.get(index))
    }

    fn poll_removal_watch(&mut self, context: &egui::Context) {
        let Some(watch) = self.removal_watch.as_ref() else {
            return;
        };

        match fs::metadata(&watch.path) {
            Ok(_) => {
                context.request_repaint_after(Duration::from_millis(250));
            }
            Err(error) => {
                self.removal_result = Some(format!(
                    "PASS / selected volume became unavailable without process crash: {error}"
                ));
                self.removal_watch = None;
            }
        }
    }
}

impl eframe::App for StorageSpikeApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        egui::Panel::top("android-status-bar-space").show(ui, |ui| {
            ui.set_height(28.0);
        });

        egui::CentralPanel::default().show(ui, |ui| {
            ui.heading("DANGOMUSHI / ANDROID USB STORAGE CONTRACT");
            ui.label(
                "A0 spike only. It creates an isolated temporary Workspace on the selected removable volume, exercises the current Rust filesystem contracts, then removes it.",
            );
            ui.separator();

            ui.horizontal(|ui| {
                if ui.button("REFRESH ANDROID STORAGE").clicked() {
                    self.refresh();
                }
                match self.all_files_access {
                    Some(true) => {
                        ui.colored_label(egui::Color32::LIGHT_GREEN, "ALL FILES ACCESS: GRANTED");
                    }
                    Some(false) => {
                        ui.colored_label(egui::Color32::YELLOW, "ALL FILES ACCESS: NOT GRANTED");
                    }
                    None => {
                        ui.colored_label(egui::Color32::LIGHT_RED, "ALL FILES ACCESS: UNKNOWN");
                    }
                }
            });

            if let Some(error) = &self.permission_error {
                ui.colored_label(
                    egui::Color32::LIGHT_RED,
                    format!("Permission probe failed: {error}"),
                );
            }

            if self.all_files_access == Some(false) {
                ui.label(
                    "Grant All files access to DANGOMUSHI Storage Spike in Android system settings, then return here and press REFRESH.",
                );
            }

            ui.add_space(8.0);
            ui.heading("StorageManager volumes");

            if let Some(error) = &self.discovery_error {
                ui.colored_label(
                    egui::Color32::LIGHT_RED,
                    format!("StorageManager discovery failed: {error}"),
                );
            }

            let mut next_selection = self.selected;
            egui::ScrollArea::vertical()
                .max_height(220.0)
                .show(ui, |ui| {
                    for (index, volume) in self.volumes.iter().enumerate() {
                        let selectable = volume.removable && volume.state == "mounted";
                        ui.horizontal_wrapped(|ui| {
                            if ui
                                .add_enabled(
                                    selectable,
                                    egui::Button::selectable(
                                        self.selected == Some(index),
                                        if selectable { "SELECT" } else { "UNAVAILABLE" },
                                    ),
                                )
                                .clicked()
                            {
                                next_selection = Some(index);
                            }
                            ui.label(format!(
                                "{} / {} / removable={} / uuid={} / {}",
                                volume.description,
                                volume.state,
                                volume.removable,
                                volume.uuid.as_deref().unwrap_or("(none)"),
                                volume.directory.display()
                            ));
                        });
                    }
                });
            self.selected = next_selection;

            ui.add_space(8.0);
            let can_run = self.all_files_access == Some(true)
                && self
                    .selected_volume()
                    .is_some_and(|volume| volume.removable && volume.state == "mounted");

            ui.horizontal(|ui| {
                if ui
                    .add_enabled(can_run, egui::Button::new("RUN STORAGE CONTRACT"))
                    .clicked()
                {
                    if let Some(volume) = self.selected_volume() {
                        self.report =
                            Some(crate::contract::run_storage_contract(&volume.directory));
                    }
                }

                if ui
                    .add_enabled(can_run, egui::Button::new("ARM USB REMOVAL TEST"))
                    .clicked()
                {
                    if let Some(volume) = self.selected_volume() {
                        self.removal_watch = Some(RemovalWatch {
                            path: volume.directory.clone(),
                        });
                        self.removal_result = None;
                    }
                }
            });

            if self.removal_watch.is_some() {
                ui.colored_label(
                    egui::Color32::YELLOW,
                    "REMOVAL TEST ARMED: physically remove the selected USB drive. The probe performs read-only metadata polling until Android reports the volume unavailable.",
                );
            }
            if let Some(result) = &self.removal_result {
                ui.colored_label(egui::Color32::LIGHT_GREEN, result);
            }

            if let Some(report) = &self.report {
                ui.separator();
                ui.heading(if report.passed() {
                    "CONTRACT RESULT: PASS"
                } else {
                    "CONTRACT RESULT: FAIL"
                });
                ui.label(format!("Volume: {}", report.volume_root.display()));
                for step in &report.steps {
                    let color = if step.passed {
                        egui::Color32::LIGHT_GREEN
                    } else {
                        egui::Color32::LIGHT_RED
                    };
                    ui.colored_label(
                        color,
                        format!(
                            "{}  {}  {}",
                            if step.passed { "[PASS]" } else { "[FAIL]" },
                            step.label,
                            step.detail
                        ),
                    );
                }
            }
        });

        self.poll_removal_watch(ui.ctx());
    }
}

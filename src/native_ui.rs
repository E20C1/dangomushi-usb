use crate::file_browser::{self, EntryKind, FileEntry};
use crate::native_ui_theme::{self as theme, PixelIcon, BAD, BG, LINE, MUTED, OK, PANEL, TEXT, WARN};
use crate::session_state::{self, SessionState, SessionView};
use crate::text_editor;
use crate::tool_registry::{self, ToolId, ToolStatus};
use crate::workspace;
use eframe::egui;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

const AUTOSAVE_DELAY: Duration = Duration::from_secs(2);

pub fn run() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("DANGOMUSHI")
            .with_inner_size([1040.0, 700.0])
            .with_min_inner_size([760.0, 520.0]),
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
    Tools,
    Files,
    Editor,
    Workspace,
    Mobile,
    System,
}

impl View {
    fn session_view(self) -> SessionView {
        match self {
            Self::Home => SessionView::Home,
            Self::Tools => SessionView::Tools,
            Self::Files => SessionView::Files,
            Self::Editor => SessionView::Editor,
            Self::Workspace => SessionView::Workspace,
            Self::Mobile => SessionView::Mobile,
            Self::System => SessionView::System,
        }
    }

    fn from_session(view: SessionView) -> Self {
        match view {
            SessionView::Home => Self::Home,
            SessionView::Tools => Self::Tools,
            SessionView::Files => Self::Files,
            SessionView::Editor => Self::Editor,
            SessionView::Workspace => Self::Workspace,
            SessionView::Mobile => Self::Mobile,
            SessionView::System => Self::System,
        }
    }
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

    fn restore_session_location(
        &mut self,
        root: &Path,
        current_dir: &Path,
        selected: Option<&Path>,
    ) -> bool {
        match file_browser::list_directory(root, current_dir) {
            Ok(entries) => {
                self.current_dir = current_dir.to_path_buf();
                self.selected = selected
                    .filter(|selected| {
                        entries
                            .iter()
                            .any(|entry| entry.relative_path.as_path() == *selected)
                    })
                    .map(Path::to_path_buf);
                self.entries = entries;
                self.error = None;
                true
            }
            Err(_) => {
                self.current_dir.clear();
                self.selected = None;
                self.refresh(root);
                false
            }
        }
    }
}

struct EditorState {
    path: Option<PathBuf>,
    buffer: String,
    saved_text: String,
    dirty: bool,
    pending_recovery: Option<String>,
    last_change: Option<Instant>,
    recovery_status: String,
    confirm_discard: bool,
}

impl EditorState {
    fn new() -> Self {
        Self {
            path: None,
            buffer: String::new(),
            saved_text: String::new(),
            dirty: false,
            pending_recovery: None,
            last_change: None,
            recovery_status: "NO FILE".to_string(),
            confirm_discard: false,
        }
    }

    fn open(&mut self, root: &Path, path: PathBuf) -> io::Result<()> {
        if self.path.as_ref() == Some(&path) {
            return Ok(());
        }

        if self.dirty {
            if let Some(current) = self.path.clone() {
                text_editor::write_recovery(root, &current, &self.buffer)?;
            }
        }

        let disk_text = text_editor::read_text(root, &path)?;
        let recovery = text_editor::latest_recovery(root, &path)?;
        let pending_recovery = match recovery {
            Some(draft) if draft.text != disk_text => Some(draft.text),
            Some(_) => {
                let _ = text_editor::clear_recovery(root, &path);
                None
            }
            None => None,
        };

        self.path = Some(path);
        self.buffer = disk_text.clone();
        self.saved_text = disk_text;
        self.dirty = false;
        self.pending_recovery = pending_recovery;
        self.last_change = None;
        self.confirm_discard = false;
        self.recovery_status = if self.pending_recovery.is_some() {
            "RECOVERY AVAILABLE".to_string()
        } else {
            "CLEAN".to_string()
        };
        Ok(())
    }

    fn mark_changed(&mut self) {
        self.dirty = self.buffer != self.saved_text;
        self.confirm_discard = false;
        self.last_change = self.dirty.then(Instant::now);
        if self.dirty {
            self.recovery_status = "RECOVERY PENDING".to_string();
        } else {
            self.recovery_status = "CLEAN".to_string();
        }
    }

    fn maybe_autosave(&mut self, root: &Path) -> io::Result<bool> {
        if !self.dirty || self.pending_recovery.is_some() {
            return Ok(false);
        }

        let Some(changed_at) = self.last_change else {
            return Ok(false);
        };

        if changed_at.elapsed() < AUTOSAVE_DELAY {
            return Ok(false);
        }

        let path = self.path.as_ref().ok_or_else(|| {
            io::Error::new(io::ErrorKind::NotFound, "no editor file is open")
        })?;

        text_editor::write_recovery(root, path, &self.buffer)?;
        self.last_change = None;
        self.recovery_status = "RECOVERY STORED".to_string();
        Ok(true)
    }

    fn save(&mut self, root: &Path) -> io::Result<()> {
        let path = self.path.as_ref().ok_or_else(|| {
            io::Error::new(io::ErrorKind::NotFound, "no editor file is open")
        })?;

        let disk_now = text_editor::read_text(root, path)?;
        if disk_now != self.saved_text {
            text_editor::write_recovery(root, path, &self.buffer)?;
            return Err(io::Error::new(
                io::ErrorKind::Other,
                "file changed on disk after it was opened; recovery was stored instead of overwriting external changes",
            ));
        }

        text_editor::save_text(root, path, &self.buffer)?;
        self.saved_text = self.buffer.clone();
        self.dirty = false;
        self.pending_recovery = None;
        self.last_change = None;
        self.confirm_discard = false;
        self.recovery_status = "SAVED".to_string();
        Ok(())
    }

    fn restore_pending_recovery(&mut self) {
        if let Some(recovery) = self.pending_recovery.take() {
            self.buffer = recovery;
            self.dirty = self.buffer != self.saved_text;
            self.last_change = None;
            self.confirm_discard = false;
            self.recovery_status = "RECOVERY RESTORED".to_string();
        }
    }

    fn discard_pending_recovery(&mut self, root: &Path) -> io::Result<()> {
        if let Some(path) = &self.path {
            text_editor::clear_recovery(root, path)?;
        }
        self.pending_recovery = None;
        self.recovery_status = "RECOVERY DISCARDED".to_string();
        Ok(())
    }

    fn discard_changes(&mut self, root: &Path) -> io::Result<()> {
        let path = self.path.as_ref().ok_or_else(|| {
            io::Error::new(io::ErrorKind::NotFound, "no editor file is open")
        })?;

        let disk_text = text_editor::read_text(root, path)?;
        let _ = text_editor::clear_recovery(root, path);
        self.buffer = disk_text.clone();
        self.saved_text = disk_text;
        self.dirty = false;
        self.pending_recovery = None;
        self.last_change = None;
        self.confirm_discard = false;
        self.recovery_status = "RELOADED FROM DISK".to_string();
        Ok(())
    }
}

struct NativeApp {
    view: View,
    workspace_root: PathBuf,
    browser: FileBrowserState,
    editor: EditorState,
    notice: String,
    session_saved: SessionState,
    session_failed: Option<SessionState>,
}

impl NativeApp {
    fn new(creation_context: &eframe::CreationContext<'_>) -> Self {
        theme::install(&creation_context.egui_ctx);

        let workspace_root = workspace::default_workspace_root();
        let browser = FileBrowserState::new(&workspace_root);

        let mut app = Self {
            view: View::Home,
            workspace_root,
            browser,
            editor: EditorState::new(),
            notice: "READY".to_string(),
            session_saved: SessionState::default(),
            session_failed: None,
        };

        app.restore_saved_session();
        app.session_saved = app.session_snapshot();
        app
    }

    fn session_snapshot(&self) -> SessionState {
        SessionState {
            view: self.view.session_view(),
            browser_dir: self.browser.current_dir.clone(),
            browser_selected: self.browser.selected.clone(),
            editor_path: self.editor.path.clone(),
        }
    }

    fn restore_saved_session(&mut self) {
        let saved = match session_state::load(&self.workspace_root) {
            Ok(Some(saved)) => saved,
            Ok(None) => return,
            Err(_) => {
                self.notice = "SESSION RESET".to_string();
                return;
            }
        };

        let mut partial = false;

        if !self.browser.restore_session_location(
            &self.workspace_root,
            &saved.browser_dir,
            saved.browser_selected.as_deref(),
        ) {
            partial = true;
        }

        if let Some(path) = saved.editor_path.clone() {
            if self.editor.open(&self.workspace_root, path).is_err() {
                partial = true;
            }
        }

        self.view = View::from_session(saved.view);
        self.notice = if partial {
            "SESSION PARTIALLY RESTORED".to_string()
        } else {
            "SESSION RESTORED".to_string()
        };
    }

    fn persist_session_if_changed(&mut self) {
        if !workspace::summary(&self.workspace_root).initialized {
            return;
        }

        let current = self.session_snapshot();
        if current == self.session_saved
            || self.session_failed.as_ref().is_some_and(|failed| failed == &current)
        {
            return;
        }

        match session_state::save(&self.workspace_root, &current) {
            Ok(()) => {
                self.session_saved = current;
                self.session_failed = None;
            }
            Err(error) => {
                self.session_failed = Some(current);
                self.notice = format!("SESSION SAVE FAILED: {error}");
            }
        }
    }

    fn open_in_editor(&mut self, path: PathBuf) {
        match self.editor.open(&self.workspace_root, path) {
            Ok(()) => {
                self.notice = "EDITOR READY".to_string();
                self.view = View::Editor;
            }
            Err(error) => {
                self.notice = format!("OPEN FAILED: {error}");
            }
        }
    }

    fn top_bar(&mut self, ui: &mut egui::Ui) {
        egui::Frame::new()
            .fill(PANEL)
            .stroke(egui::Stroke::new(1.0, LINE))
            .corner_radius(0)
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
                            let color = if self.notice.contains("FAILED") {
                                BAD
                            } else {
                                OK
                            };
                            ui.label(
                                egui::RichText::new(&self.notice)
                                    .color(color)
                                    .monospace(),
                            );
                        },
                    );
                });
            });

        ui.add_space(6.0);

        egui::Frame::new()
            .fill(PANEL)
            .stroke(egui::Stroke::new(1.0, LINE))
            .corner_radius(0)
            .inner_margin(egui::Margin::same(4))
            .show(ui, |ui| {
                ui.horizontal_wrapped(|ui| {
                    for (view, icon, label) in [
                        (View::Home, PixelIcon::Home, "[01] HOME"),
                        (View::Tools, PixelIcon::Tools, "[02] TOOLS"),
                        (View::Files, PixelIcon::Files, "[03] FILES"),
                        (View::Editor, PixelIcon::Editor, "[04] EDITOR"),
                        (View::Workspace, PixelIcon::Workspace, "[05] WORKSPACE"),
                        (View::Mobile, PixelIcon::Mobile, "[06] MOBILE"),
                        (View::System, PixelIcon::System, "[07] SYSTEM"),
                    ] {
                        if theme::pixel_button(
                            ui,
                            icon,
                            label,
                            self.view == view,
                            true,
                            118.0,
                        )
                        .clicked()
                        {
                            self.view = view;
                        }
                    }
                });
            });
    }

    fn home(&mut self, ui: &mut egui::Ui) {
        let summary = workspace::summary(&self.workspace_root);

        theme::section_title(ui, PixelIcon::Home, "LOCAL CONTROL PANEL");
        ui.label(
            egui::RichText::new(
                "Native Rust shell for files, editing, recovery and trusted local access.",
            )
            .color(MUTED),
        );
        ui.add_space(12.0);

        ui.columns(3, |columns| {
            theme::metric(
                &mut columns[0],
                PixelIcon::System,
                "RUNTIME",
                env!("CARGO_PKG_VERSION"),
                "eframe / egui",
            );
            theme::metric(
                &mut columns[1],
                PixelIcon::Mobile,
                "NETWORK",
                "LOCAL",
                "no LAN listener",
            );
            theme::metric(
                &mut columns[2],
                PixelIcon::Workspace,
                "WORKSPACE",
                if summary.initialized { "READY" } else { "NOT INIT" },
                &format!("{}/{} dirs", summary.present_dirs, summary.total_dirs),
            );
        });

        ui.add_space(10.0);
        theme::panel(ui, |ui| {
            theme::small_label(ui, "TOOLS");
            ui.add_space(4.0);

            for tool in tool_registry::all() {
                ui.horizontal(|ui| {
                    let enabled = tool.status.is_ready();
                    if theme::pixel_button(
                        ui,
                        tool_icon(tool.id),
                        tool.label,
                        false,
                        enabled,
                        190.0,
                    )
                    .clicked()
                    {
                        self.launch_tool(tool.id);
                    }

                    theme::status_badge(
                        ui,
                        tool.status.label(),
                        if enabled { OK } else { MUTED },
                    );
                });
            }

            ui.add_space(4.0);
            if ui.button("OPEN TOOL LAUNCHER").clicked() {
                self.view = View::Tools;
            }
        });
    }

    fn tools(&mut self, ui: &mut egui::Ui) {
        theme::section_title(ui, PixelIcon::Tools, "TOOLS");
        ui.label(
            egui::RichText::new(
                "Central registry for built-in DANGOMUSHI tools. Registry entries describe tools; explicit Rust dispatch decides what can run.",
            )
            .color(MUTED),
        );
        ui.add_space(10.0);

        for category in [
            crate::tool_registry::ToolCategory::Workspace,
            crate::tool_registry::ToolCategory::Utility,
            crate::tool_registry::ToolCategory::VersionControl,
            crate::tool_registry::ToolCategory::Recovery,
        ] {
            theme::panel(ui, |ui| {
                theme::small_label(ui, category.label());
                ui.add_space(4.0);

                for tool in tool_registry::all()
                    .iter()
                    .filter(|tool| tool.category == category)
                {
                    ui.horizontal(|ui| {
                        let ready = tool.status == ToolStatus::Ready;
                        if theme::pixel_button(
                            ui,
                            tool_icon(tool.id),
                            tool.label,
                            false,
                            ready,
                            200.0,
                        )
                        .clicked()
                        {
                            self.launch_tool(tool.id);
                        }

                        ui.vertical(|ui| {
                            theme::status_badge(
                                ui,
                                tool.status.label(),
                                if ready { OK } else { MUTED },
                            );
                            ui.label(
                                egui::RichText::new(tool.description)
                                    .color(MUTED)
                                    .size(12.0),
                            );
                        });
                    });
                    ui.add_space(4.0);
                }
            });
            ui.add_space(8.0);
        }
    }

    fn launch_tool(&mut self, tool: ToolId) {
        match tool {
            ToolId::Files => {
                self.browser.refresh(&self.workspace_root);
                self.view = View::Files;
                self.notice = "FILES READY".to_string();
            }
            ToolId::Editor => {
                self.view = View::Editor;
                self.notice = if self.editor.path.is_some() {
                    "EDITOR READY".to_string()
                } else {
                    "EDITOR: CHOOSE A FILE".to_string()
                };
            }
            ToolId::HashVerify
            | ToolId::Compare
            | ToolId::Repositories
            | ToolId::Backup => {
                self.notice = "TOOL NOT IMPLEMENTED".to_string();
            }
        }
    }

    fn files(&mut self, ui: &mut egui::Ui) {
        theme::section_title(ui, PixelIcon::Files, "FILES");
        ui.label(
            egui::RichText::new(
                "Read-only browser constrained to the DANGOMUSHI workspace root.",
            )
            .color(MUTED),
        );
        ui.add_space(10.0);

        theme::panel(ui, |ui| {
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
                .corner_radius(0)
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
        let mut edit_path: Option<PathBuf> = None;

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
                    .max_height(330.0)
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
                                ui.horizontal(|ui| {
                                    theme::pixel_icon(ui, entry_icon(entry.kind), kind_color(entry.kind));
                                    ui.add_sized(
                                        [58.0, 22.0],
                                        egui::Label::new(
                                            egui::RichText::new(entry.kind.label())
                                                .monospace()
                                                .color(kind_color(entry.kind)),
                                        ),
                                    );
                                });

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

                                if response.double_clicked() {
                                    match entry.kind {
                                        EntryKind::Directory => {
                                            navigate_to = Some(entry.relative_path.clone())
                                        }
                                        EntryKind::File => edit_path = Some(entry.relative_path.clone()),
                                        _ => {}
                                    }
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

        theme::panel(ui, |ui| {
            if let Some(selected) = &selected_path {
                theme::key_value(ui, "SELECTED", &file_browser::display_path(selected));

                if let Some(entry) = &selected_entry {
                    theme::key_value(ui, "TYPE", entry.kind.label());

                    match entry.kind {
                        EntryKind::Directory => {
                            if ui.button("OPEN DIRECTORY").clicked() {
                                open_selected = Some(entry.relative_path.clone());
                            }
                        }
                        EntryKind::File => {
                            if ui.button("OPEN IN EDITOR").clicked() {
                                edit_path = Some(entry.relative_path.clone());
                            }
                        }
                        EntryKind::Symlink => {
                            ui.label(
                                egui::RichText::new(
                                    "Symlinks are visible but are never traversed or edited.",
                                )
                                .color(WARN),
                            );
                        }
                        EntryKind::Other => {
                            ui.label(
                                egui::RichText::new("Unsupported file type.").color(MUTED),
                            );
                        }
                    }
                }
            } else {
                ui.label(egui::RichText::new("No item selected.").color(MUTED));
            }
        });

        if let Some(destination) = open_selected {
            self.browser.navigate(&self.workspace_root, destination);
        }
        if let Some(path) = edit_path {
            self.open_in_editor(path);
        }
    }

    fn editor(&mut self, ui: &mut egui::Ui) {
        theme::section_title(ui, PixelIcon::Editor, "EDITOR");

        let Some(path) = self.editor.path.clone() else {
            theme::panel(ui, |ui| {
                ui.label(
                    egui::RichText::new("NO FILE OPEN")
                        .color(WARN)
                        .strong()
                        .monospace(),
                );
                ui.add_space(6.0);
                ui.label("Choose a regular UTF-8 text file from FILES.");
                if ui.button("GO TO FILES").clicked() {
                    self.view = View::Files;
                }
            });
            return;
        };

        theme::panel(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.label(
                    egui::RichText::new(file_browser::display_path(&path))
                        .monospace()
                        .color(TEXT),
                );
                ui.separator();

                let state = if self.editor.dirty { "MODIFIED" } else { "SAVED" };
                ui.label(
                    egui::RichText::new(state)
                        .monospace()
                        .strong()
                        .color(if self.editor.dirty { WARN } else { OK }),
                );
                ui.separator();
                ui.label(
                    egui::RichText::new(&self.editor.recovery_status)
                        .monospace()
                        .color(MUTED),
                );
            });
        });

        ui.add_space(8.0);

        if self.editor.pending_recovery.is_some() {
            egui::Frame::new()
                .fill(PANEL)
                .stroke(egui::Stroke::new(1.0, WARN))
                .corner_radius(0)
            .inner_margin(egui::Margin::same(10))
                .show(ui, |ui| {
                    ui.label(
                        egui::RichText::new("UNSAVED RECOVERY FOUND")
                            .color(WARN)
                            .strong()
                            .monospace(),
                    );
                    ui.label(
                        "A recovery draft differs from the file on disk. Choose which state to continue from before editing.",
                    );
                    ui.horizontal(|ui| {
                        if ui.button("RESTORE RECOVERY").clicked() {
                            self.editor.restore_pending_recovery();
                            self.notice = "RECOVERY RESTORED".to_string();
                        }
                        if ui.button("DISCARD RECOVERY").clicked() {
                            match self.editor.discard_pending_recovery(&self.workspace_root) {
                                Ok(()) => self.notice = "RECOVERY DISCARDED".to_string(),
                                Err(error) => {
                                    self.notice = format!("RECOVERY ERROR: {error}")
                                }
                            }
                        }
                    });
                });
            ui.add_space(8.0);
        }

        let editing_enabled = self.editor.pending_recovery.is_none();

        ui.horizontal(|ui| {
            let save_enabled = editing_enabled && self.editor.dirty;
            if ui
                .add_enabled(save_enabled, egui::Button::new("[SAVE]"))
                .clicked()
            {
                match self.editor.save(&self.workspace_root) {
                    Ok(()) => {
                        self.notice = "FILE SAVED".to_string();
                        self.browser.refresh(&self.workspace_root);
                    }
                    Err(error) => self.notice = format!("SAVE FAILED: {error}"),
                }
            }

            if self.editor.dirty {
                if self.editor.confirm_discard {
                    if ui.button("[CONFIRM DISCARD]").clicked() {
                        match self.editor.discard_changes(&self.workspace_root) {
                            Ok(()) => self.notice = "CHANGES DISCARDED".to_string(),
                            Err(error) => self.notice = format!("RELOAD FAILED: {error}"),
                        }
                    }
                } else if ui.button("[DISCARD CHANGES]").clicked() {
                    self.editor.confirm_discard = true;
                }
            }

            ui.label(
                egui::RichText::new(format!(
                    "{} / {} bytes",
                    self.editor.buffer.len(),
                    text_editor::MAX_TEXT_FILE_BYTES
                ))
                .color(MUTED)
                .monospace(),
            );
        });

        ui.add_space(6.0);

        let response = ui.add_enabled(
            editing_enabled,
            egui::TextEdit::multiline(&mut self.editor.buffer)
                .font(egui::TextStyle::Monospace)
                .desired_width(f32::INFINITY)
                .desired_rows(25),
        );

        if response.changed() {
            self.editor.mark_changed();

            if !self.editor.dirty {
                let _ = text_editor::clear_recovery(&self.workspace_root, &path);
            }
        }

        if self.editor.dirty && self.editor.last_change.is_some() {
            ui.ctx().request_repaint_after(Duration::from_millis(250));
        }

        match self.editor.maybe_autosave(&self.workspace_root) {
            Ok(true) => self.notice = "RECOVERY AUTOSAVED".to_string(),
            Ok(false) => {}
            Err(error) => self.notice = format!("AUTOSAVE FAILED: {error}"),
        }
    }

    fn workspace(&mut self, ui: &mut egui::Ui) {
        let summary = workspace::summary(&self.workspace_root);

        theme::section_title(ui, PixelIcon::Workspace, "WORKSPACE");
        theme::panel(ui, |ui| {
            theme::key_value(ui, "PATH", &self.workspace_root.display().to_string());
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
        theme::section_title(ui, PixelIcon::Mobile, "MOBILE ACCESS");
        theme::panel(ui, |ui| {
            ui.label(
                egui::RichText::new("NOT ENABLED IN M1")
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
        theme::section_title(ui, PixelIcon::System, "SYSTEM");
        theme::panel(ui, |ui| {
            theme::key_value(ui, "DESKTOP UI", "eframe / egui 0.37");
            theme::key_value(ui, "FILE BROWSER", "workspace-rooted / read-only");
            theme::key_value(ui, "TEXT EDITOR", "UTF-8 / explicit save / recovery");
            theme::key_value(ui, "EDITOR LIMIT", "4 MiB per file");
            theme::key_value(ui, "WEB GATEWAY", "127.0.0.1 only");
            theme::key_value(ui, "PUBLIC INTERNET", "not exposed");
            theme::key_value(ui, "EXTERNAL CDN", "none");
            theme::key_value(ui, "WORKSPACE SOURCE", "local filesystem");
        });
    }
}

impl eframe::App for NativeApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        egui::CentralPanel::default()
            .frame(
                egui::Frame::new()
                    .fill(BG)
                    .corner_radius(0)
                    .inner_margin(egui::Margin::same(14)),
            )
            .show(ui, |ui| {
                self.top_bar(ui);
                ui.add_space(12.0);

                match self.view {
                    View::Home => self.home(ui),
                    View::Tools => self.tools(ui),
                    View::Files => self.files(ui),
                    View::Editor => self.editor(ui),
                    View::Workspace => self.workspace(ui),
                    View::Mobile => self.mobile(ui),
                    View::System => self.system(ui),
                }
            });

        self.persist_session_if_changed();
    }
}

fn entry_icon(kind: EntryKind) -> PixelIcon {
    match kind {
        EntryKind::Directory => PixelIcon::Folder,
        EntryKind::File => PixelIcon::File,
        EntryKind::Symlink => PixelIcon::Link,
        EntryKind::Other => PixelIcon::Other,
    }
}

fn tool_icon(tool: ToolId) -> PixelIcon {
    match tool {
        ToolId::Files => PixelIcon::Files,
        ToolId::Editor => PixelIcon::Editor,
        ToolId::HashVerify => PixelIcon::Hash,
        ToolId::Compare => PixelIcon::Compare,
        ToolId::Repositories => PixelIcon::Repository,
        ToolId::Backup => PixelIcon::Backup,
    }
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

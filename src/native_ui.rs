use crate::device_pairing::{DevicePairing, PairedDeviceCandidate, PairingInvitation, PairingStatus};
use crate::document_search;
use crate::file_browser::{self, EntryKind, FileEntry};
use crate::file_compare;
use crate::hash_verify;
use crate::lan_access_control::LanEnableControl;
use crate::native_ui_theme::{self as theme, PixelIcon, BAD, BG, LINE, MUTED, OK, PANEL, TEXT, WARN};
use crate::repository_git;
use crate::session_state::{self, SessionState, SessionView};
use crate::text_editor;
use crate::text_json;
use crate::tool_registry::{self, ToolId, ToolStatus};
use crate::workspace;
use crate::workspace_backup;
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
    HashVerify,
    TextJson,
    Compare,
    Search,
    Repositories,
    Backup,
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
            Self::HashVerify => SessionView::HashVerify,
            Self::TextJson => SessionView::TextJson,
            Self::Compare => SessionView::Compare,
            Self::Search => SessionView::Search,
            Self::Repositories => SessionView::Repositories,
            Self::Backup => SessionView::Backup,
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
            SessionView::HashVerify => Self::HashVerify,
            SessionView::TextJson => Self::TextJson,
            SessionView::Compare => Self::Compare,
            SessionView::Search => Self::Search,
            SessionView::Repositories => Self::Repositories,
            SessionView::Backup => Self::Backup,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HashResultState {
    Calculated,
    Match,
    Mismatch,
}

struct HashVerifyState {
    target: String,
    expected: String,
    actual: Option<hash_verify::FileHash>,
    status: Option<HashResultState>,
    error: Option<String>,
}

impl HashVerifyState {
    fn new() -> Self {
        Self {
            target: String::new(),
            expected: String::new(),
            actual: None,
            status: None,
            error: None,
        }
    }

    fn set_target_path(&mut self, path: &Path) {
        self.target = path.to_string_lossy().into_owned();
        self.clear_result();
    }

    fn clear_result(&mut self) {
        self.actual = None;
        self.status = None;
        self.error = None;
    }

    fn calculate(&mut self, root: &Path) -> Result<(), hash_verify::HashVerifyError> {
        self.error = None;
        match hash_verify::hash_workspace_file(root, Path::new(self.target.trim())) {
            Ok(actual) => {
                self.actual = Some(actual);
                self.status = Some(HashResultState::Calculated);
                Ok(())
            }
            Err(error) => {
                self.actual = None;
                self.status = None;
                self.error = Some(error.to_string());
                Err(error)
            }
        }
    }

    fn verify(&mut self, root: &Path) -> Result<bool, hash_verify::HashVerifyError> {
        self.error = None;
        match hash_verify::verify_workspace_file(
            root,
            Path::new(self.target.trim()),
            &self.expected,
        ) {
            Ok(result) => {
                self.actual = Some(result.actual);
                self.status = Some(if result.matches {
                    HashResultState::Match
                } else {
                    HashResultState::Mismatch
                });
                Ok(result.matches)
            }
            Err(error) => {
                self.actual = None;
                self.status = None;
                self.error = Some(error.to_string());
                Err(error)
            }
        }
    }
}

struct TextJsonState {
    input: String,
    output: String,
    status: Option<String>,
    error: Option<String>,
}

impl TextJsonState {
    fn new() -> Self {
        Self {
            input: String::new(),
            output: String::new(),
            status: None,
            error: None,
        }
    }

    fn clear_result(&mut self) {
        self.output.clear();
        self.status = None;
        self.error = None;
    }

    fn set_input(&mut self, input: String) {
        self.input = input;
        self.clear_result();
    }

    fn load_file(&mut self, root: &Path, path: &Path) -> io::Result<()> {
        let input = text_editor::read_text(root, path)?;
        self.set_input(input);
        Ok(())
    }

    fn apply_text_result(
        &mut self,
        result: Result<String, text_json::TextJsonError>,
        status: &str,
    ) -> Result<(), text_json::TextJsonError> {
        match result {
            Ok(output) => {
                self.output = output;
                self.status = Some(status.to_string());
                self.error = None;
                Ok(())
            }
            Err(error) => {
                self.output.clear();
                self.status = None;
                self.error = Some(error.to_string());
                Err(error)
            }
        }
    }

    fn validate_json(&mut self) -> Result<text_json::JsonKind, text_json::TextJsonError> {
        match text_json::validate_json(&self.input) {
            Ok(kind) => {
                self.output.clear();
                self.status = Some(format!("JSON VALID / {}", kind.label()));
                self.error = None;
                Ok(kind)
            }
            Err(error) => {
                self.output.clear();
                self.status = None;
                self.error = Some(error.to_string());
                Err(error)
            }
        }
    }

    fn use_output_as_input(&mut self) {
        self.input = std::mem::take(&mut self.output);
        self.status = None;
        self.error = None;
    }
}

struct FileCompareState {
    left: String,
    right: String,
    result: Option<file_compare::FileComparison>,
    error: Option<String>,
}

impl FileCompareState {
    fn new() -> Self {
        Self {
            left: String::new(),
            right: String::new(),
            result: None,
            error: None,
        }
    }

    fn set_left_path(&mut self, path: &Path) {
        self.left = path.to_string_lossy().into_owned();
        self.clear_result();
    }

    fn set_right_path(&mut self, path: &Path) {
        self.right = path.to_string_lossy().into_owned();
        self.clear_result();
    }

    fn clear_result(&mut self) {
        self.result = None;
        self.error = None;
    }

    fn compare(&mut self, root: &Path) -> Result<bool, file_compare::FileCompareError> {
        self.error = None;
        match file_compare::compare_workspace_files(
            root,
            Path::new(self.left.trim()),
            Path::new(self.right.trim()),
        ) {
            Ok(result) => {
                let equal = result.equal;
                self.result = Some(result);
                Ok(equal)
            }
            Err(error) => {
                self.result = None;
                self.error = Some(error.to_string());
                Err(error)
            }
        }
    }
}

struct DocumentSearchState {
    query: String,
    report: Option<document_search::DocumentSearchReport>,
    error: Option<String>,
}

impl DocumentSearchState {
    fn new() -> Self {
        Self {
            query: String::new(),
            report: None,
            error: None,
        }
    }

    fn clear_result(&mut self) {
        self.report = None;
        self.error = None;
    }

    fn search(
        &mut self,
        root: &Path,
    ) -> Result<usize, document_search::DocumentSearchError> {
        self.error = None;
        match document_search::search_documents(root, &self.query) {
            Ok(report) => {
                let count = report.matches.len();
                self.report = Some(report);
                Ok(count)
            }
            Err(error) => {
                self.report = None;
                self.error = Some(error.to_string());
                Err(error)
            }
        }
    }
}

struct RepositoryUiState {
    repositories: Vec<repository_git::RepositoryEntry>,
    selected: Option<PathBuf>,
    status: Option<repository_git::RepositoryStatus>,
    output_label: Option<&'static str>,
    output: Option<repository_git::GitTextOutput>,
    error: Option<String>,
}

impl RepositoryUiState {
    fn new(root: &Path) -> Self {
        let mut state = Self {
            repositories: Vec::new(),
            selected: None,
            status: None,
            output_label: None,
            output: None,
            error: None,
        };
        let _ = state.refresh_list(root);
        state
    }

    fn refresh_list(
        &mut self,
        root: &Path,
    ) -> Result<usize, repository_git::RepositoryGitError> {
        match repository_git::list_repositories(root) {
            Ok(repositories) => {
                if self
                    .selected
                    .as_ref()
                    .is_some_and(|selected| {
                        !repositories
                            .iter()
                            .any(|repo| &repo.relative_path == selected)
                    })
                {
                    self.selected = None;
                    self.clear_result();
                }

                let count = repositories.len();
                self.repositories = repositories;
                self.error = None;
                Ok(count)
            }
            Err(error) => {
                self.repositories.clear();
                self.selected = None;
                self.clear_result();
                self.error = Some(error.to_string());
                Err(error)
            }
        }
    }

    fn select(&mut self, path: PathBuf) {
        self.selected = Some(path);
        self.clear_result();
    }

    fn clear_result(&mut self) {
        self.status = None;
        self.output_label = None;
        self.output = None;
        self.error = None;
    }

    fn refresh_status(
        &mut self,
        root: &Path,
    ) -> Result<(), repository_git::RepositoryGitError> {
        let selected = self
            .selected
            .as_deref()
            .ok_or(repository_git::RepositoryGitError::InvalidRepositoryPath)?;

        match repository_git::repository_status(root, selected) {
            Ok(status) => {
                self.status = Some(status);
                self.output_label = None;
                self.output = None;
                self.error = None;
                Ok(())
            }
            Err(error) => {
                self.status = None;
                self.output_label = None;
                self.output = None;
                self.error = Some(error.to_string());
                Err(error)
            }
        }
    }

    fn run_output(
        &mut self,
        root: &Path,
        label: &'static str,
        operation: fn(
            &Path,
            &Path,
        ) -> Result<repository_git::GitTextOutput, repository_git::RepositoryGitError>,
    ) -> Result<(), repository_git::RepositoryGitError> {
        let selected = self
            .selected
            .as_deref()
            .ok_or(repository_git::RepositoryGitError::InvalidRepositoryPath)?;

        match operation(root, selected) {
            Ok(output) => {
                self.output_label = Some(label);
                self.output = Some(output);
                self.error = None;
                Ok(())
            }
            Err(error) => {
                self.output_label = Some(label);
                self.output = None;
                self.error = Some(error.to_string());
                Err(error)
            }
        }
    }
}

struct BackupUiState {
    backups: Vec<workspace_backup::WorkspaceBackupEntry>,
    selected: Option<PathBuf>,
    summary: Option<workspace_backup::WorkspaceBackupSummary>,
    error: Option<String>,
    restore_confirmation: String,
}

impl BackupUiState {
    fn new(root: &Path) -> Self {
        let mut state = Self {
            backups: Vec::new(),
            selected: None,
            summary: None,
            error: None,
            restore_confirmation: String::new(),
        };
        let _ = state.refresh_list(root);
        state
    }

    fn refresh_list(
        &mut self,
        root: &Path,
    ) -> Result<usize, workspace_backup::WorkspaceBackupError> {
        match workspace_backup::list_backups(root) {
            Ok(backups) => {
                if self
                    .selected
                    .as_ref()
                    .is_some_and(|selected| {
                        !backups
                            .iter()
                            .any(|backup| &backup.relative_path == selected)
                    })
                {
                    self.selected = None;
                    self.summary = None;
                    self.restore_confirmation.clear();
                }

                let count = backups.len();
                self.backups = backups;
                self.error = None;
                Ok(count)
            }
            Err(error) => {
                self.backups.clear();
                self.selected = None;
                self.summary = None;
                self.restore_confirmation.clear();
                self.error = Some(error.to_string());
                Err(error)
            }
        }
    }

    fn select(&mut self, path: PathBuf) {
        self.selected = Some(path);
        self.summary = None;
        self.error = None;
        self.restore_confirmation.clear();
    }

    fn create(
        &mut self,
        root: &Path,
    ) -> Result<workspace_backup::WorkspaceBackupEntry, workspace_backup::WorkspaceBackupError> {
        self.error = None;
        match workspace_backup::create_backup(root) {
            Ok(created) => {
                let _ = self.refresh_list(root);
                self.selected = Some(created.relative_path.clone());
                self.summary = None;
                self.restore_confirmation.clear();
                Ok(created)
            }
            Err(error) => {
                self.error = Some(error.to_string());
                Err(error)
            }
        }
    }

    fn verify(
        &mut self,
        root: &Path,
    ) -> Result<workspace_backup::WorkspaceBackupSummary, workspace_backup::WorkspaceBackupError> {
        let selected = self
            .selected
            .as_deref()
            .ok_or(workspace_backup::WorkspaceBackupError::InvalidBackupPath)?;

        match workspace_backup::verify_backup(root, selected) {
            Ok(summary) => {
                self.summary = Some(summary);
                self.error = None;
                Ok(summary)
            }
            Err(error) => {
                self.summary = None;
                self.error = Some(error.to_string());
                Err(error)
            }
        }
    }
}

struct NativeApp {
    view: View,
    workspace_root: PathBuf,
    browser: FileBrowserState,
    editor: EditorState,
    hash_verify: HashVerifyState,
    lan_access: LanEnableControl,
    device_pairing: DevicePairing,
    pairing_invitation: Option<PairingInvitation>,
    paired_device: Option<PairedDeviceCandidate>,
    text_json: TextJsonState,
    file_compare: FileCompareState,
    document_search: DocumentSearchState,
    repository_git: RepositoryUiState,
    workspace_backup: BackupUiState,
    notice: String,
    session_saved: SessionState,
    session_failed: Option<SessionState>,
}

impl NativeApp {
    fn new(creation_context: &eframe::CreationContext<'_>) -> Self {
        theme::install(&creation_context.egui_ctx);

        let workspace_root = workspace::default_workspace_root();
        let browser = FileBrowserState::new(&workspace_root);
        let repository_git = RepositoryUiState::new(&workspace_root);
        let workspace_backup = BackupUiState::new(&workspace_root);

        let mut app = Self {
            view: View::Home,
            workspace_root,
            browser,
            editor: EditorState::new(),
            hash_verify: HashVerifyState::new(),
            lan_access: LanEnableControl::default(),
            device_pairing: DevicePairing::default(),
            pairing_invitation: None,
            paired_device: None,
            text_json: TextJsonState::new(),
            file_compare: FileCompareState::new(),
            document_search: DocumentSearchState::new(),
            repository_git,
            workspace_backup,
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
                            let color = if self.notice.contains("FAILED")
                                || self.notice.contains("MISMATCH")
                                || self.notice.contains("DIFFERENT")
                            {
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
                if self.lan_access.is_enabled() {
                    "LAN control ON / listener locked"
                } else {
                    "LAN control OFF / no LAN listener"
                },
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
            ToolId::HashVerify => {
                self.view = View::HashVerify;
                self.notice = "HASH / VERIFY READY".to_string();
            }
            ToolId::TextJson => {
                self.view = View::TextJson;
                self.notice = "TEXT / JSON READY".to_string();
            }
            ToolId::Compare => {
                self.view = View::Compare;
                self.notice = "COMPARE READY".to_string();
            }
            ToolId::Search => {
                self.view = View::Search;
                self.notice = "SEARCH READY".to_string();
            }
            ToolId::Repositories => {
                match self.repository_git.refresh_list(&self.workspace_root) {
                    Ok(count) => {
                        self.notice = format!("REPOSITORIES READY / {count} FOUND");
                    }
                    Err(error) => {
                        self.notice = format!("REPOSITORIES FAILED: {error}");
                    }
                }
                self.view = View::Repositories;
            }
            ToolId::Backup => {
                match self.workspace_backup.refresh_list(&self.workspace_root) {
                    Ok(count) => {
                        self.notice = format!("BACKUP READY / {count} SNAPSHOT(S)");
                    }
                    Err(error) => {
                        self.notice = format!("BACKUP FAILED: {error}");
                    }
                }
                self.view = View::Backup;
            }
        }
    }

    fn persist_dirty_editor_recovery_for_backup(&mut self) -> io::Result<()> {
        if !self.editor.dirty {
            return Ok(());
        }

        let path = self.editor.path.as_ref().ok_or_else(|| {
            io::Error::new(io::ErrorKind::NotFound, "no editor file is open")
        })?;
        text_editor::write_recovery(&self.workspace_root, path, &self.editor.buffer)?;
        self.editor.last_change = None;
        self.editor.recovery_status = "RECOVERY STORED FOR BACKUP".to_string();
        Ok(())
    }

    fn reset_after_workspace_restore(
        &mut self,
        restored_backup: PathBuf,
        summary: workspace_backup::WorkspaceBackupSummary,
    ) {
        self.browser = FileBrowserState::new(&self.workspace_root);
        self.editor = EditorState::new();
        self.hash_verify = HashVerifyState::new();
        self.text_json = TextJsonState::new();
        self.file_compare = FileCompareState::new();
        self.document_search = DocumentSearchState::new();
        self.repository_git = RepositoryUiState::new(&self.workspace_root);
        self.workspace_backup = BackupUiState::new(&self.workspace_root);
        self.workspace_backup.select(restored_backup);
        self.workspace_backup.summary = Some(summary);
        self.workspace_backup.restore_confirmation.clear();
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
        let mut hash_selected: Option<PathBuf> = None;
        let mut text_json_selected: Option<PathBuf> = None;
        let mut compare_left_selected: Option<PathBuf> = None;
        let mut compare_right_selected: Option<PathBuf> = None;

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
                            ui.horizontal_wrapped(|ui| {
                                if ui.button("OPEN IN EDITOR").clicked() {
                                    edit_path = Some(entry.relative_path.clone());
                                }
                                if ui.button("HASH / VERIFY").clicked() {
                                    hash_selected = Some(entry.relative_path.clone());
                                }
                                if ui.button("TEXT / JSON").clicked() {
                                    text_json_selected = Some(entry.relative_path.clone());
                                }
                                if ui.button("COMPARE AS A").clicked() {
                                    compare_left_selected = Some(entry.relative_path.clone());
                                }
                                if ui.button("COMPARE AS B").clicked() {
                                    compare_right_selected = Some(entry.relative_path.clone());
                                }
                            });
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
        if let Some(path) = hash_selected {
            self.hash_verify.set_target_path(&path);
            self.view = View::HashVerify;
            self.notice = "HASH / VERIFY READY".to_string();
        }
        if let Some(path) = text_json_selected {
            match self.text_json.load_file(&self.workspace_root, &path) {
                Ok(()) => {
                    self.view = View::TextJson;
                    self.notice = "TEXT / JSON FILE LOADED".to_string();
                }
                Err(error) => {
                    self.notice = format!("TEXT / JSON LOAD FAILED: {error}");
                }
            }
        }
        if let Some(path) = compare_left_selected {
            self.file_compare.set_left_path(&path);
            self.view = View::Compare;
            self.notice = "COMPARE A SET".to_string();
        }
        if let Some(path) = compare_right_selected {
            self.file_compare.set_right_path(&path);
            self.view = View::Compare;
            self.notice = "COMPARE B SET".to_string();
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

    fn hash_verify_view(&mut self, ui: &mut egui::Ui) {
        theme::section_title(ui, PixelIcon::Hash, "HASH / VERIFY");
        ui.label(
            egui::RichText::new(
                "Calculate SHA-256 locally or compare a Workspace file with a trusted expected digest.",
            )
            .color(MUTED),
        );
        ui.add_space(10.0);

        let selected_file = self.browser.selected.as_ref().and_then(|selected| {
            self.browser
                .entries
                .iter()
                .find(|entry| {
                    entry.kind == EntryKind::File && &entry.relative_path == selected
                })
                .map(|entry| entry.relative_path.clone())
        });

        theme::panel(ui, |ui| {
            theme::small_label(ui, "TARGET / WORKSPACE-RELATIVE PATH");
            let target_response = ui.add(
                egui::TextEdit::singleline(&mut self.hash_verify.target)
                    .font(egui::TextStyle::Monospace)
                    .desired_width(f32::INFINITY)
                    .hint_text("data/example.bin"),
            );
            if target_response.changed() {
                self.hash_verify.clear_result();
            }

            ui.horizontal(|ui| {
                if ui
                    .add_enabled(
                        selected_file.is_some(),
                        egui::Button::new("[USE FILES SELECTION]"),
                    )
                    .clicked()
                {
                    if let Some(path) = &selected_file {
                        self.hash_verify.set_target_path(path);
                    }
                }

                if ui.button("[GO TO FILES]").clicked() {
                    self.view = View::Files;
                }
            });
        });

        ui.add_space(8.0);

        theme::panel(ui, |ui| {
            theme::small_label(ui, "EXPECTED SHA-256 / OPTIONAL FOR CALCULATE");
            let expected_response = ui.add(
                egui::TextEdit::singleline(&mut self.hash_verify.expected)
                    .font(egui::TextStyle::Monospace)
                    .desired_width(f32::INFINITY)
                    .hint_text("64 hexadecimal characters"),
            );
            if expected_response.changed() {
                self.hash_verify.clear_result();
            }

            ui.label(
                egui::RichText::new(format!(
                    "{} / {} hex characters after trim",
                    self.hash_verify.expected.trim().len(),
                    hash_verify::SHA256_HEX_LENGTH
                ))
                .color(MUTED)
                .monospace()
                .size(11.0),
            );

            ui.add_space(4.0);
            ui.horizontal(|ui| {
                let can_hash = !self.hash_verify.target.trim().is_empty();

                if ui
                    .add_enabled(can_hash, egui::Button::new("[CALCULATE SHA-256]"))
                    .clicked()
                {
                    match self.hash_verify.calculate(&self.workspace_root) {
                        Ok(()) => self.notice = "SHA-256 CALCULATED".to_string(),
                        Err(error) => self.notice = format!("HASH FAILED: {error}"),
                    }
                }

                let can_verify =
                    can_hash && !self.hash_verify.expected.trim().is_empty();
                if ui
                    .add_enabled(can_verify, egui::Button::new("[VERIFY]"))
                    .clicked()
                {
                    match self.hash_verify.verify(&self.workspace_root) {
                        Ok(true) => self.notice = "SHA-256 MATCH".to_string(),
                        Ok(false) => self.notice = "SHA-256 MISMATCH".to_string(),
                        Err(error) => self.notice = format!("VERIFY FAILED: {error}"),
                    }
                }

                if ui.button("[CLEAR]").clicked() {
                    self.hash_verify = HashVerifyState::new();
                    self.notice = "HASH / VERIFY CLEARED".to_string();
                }
            });
        });

        ui.add_space(8.0);

        theme::panel(ui, |ui| {
            theme::small_label(ui, "RESULT");

            if let Some(error) = &self.hash_verify.error {
                ui.label(
                    egui::RichText::new("HASH / VERIFY ERROR")
                        .color(BAD)
                        .strong()
                        .monospace(),
                );
                ui.label(egui::RichText::new(error).color(MUTED).monospace());
                return;
            }

            let Some(actual) = self.hash_verify.actual else {
                ui.label(
                    egui::RichText::new("NO RESULT")
                        .color(MUTED)
                        .monospace(),
                );
                return;
            };

            theme::key_value(ui, "SHA-256", &actual.sha256.to_string());
            theme::key_value(ui, "BYTES HASHED", &actual.bytes_hashed.to_string());

            match self.hash_verify.status {
                Some(HashResultState::Calculated) => {
                    theme::status_badge(ui, "CALCULATED", OK);
                }
                Some(HashResultState::Match) => {
                    theme::status_badge(ui, "MATCH", OK);
                }
                Some(HashResultState::Mismatch) => {
                    theme::status_badge(ui, "MISMATCH", BAD);
                    ui.label(
                        egui::RichText::new(
                            "The file does not match the expected SHA-256 digest.",
                        )
                        .color(BAD),
                    );
                }
                None => {}
            }
        });

        ui.add_space(8.0);
        ui.label(
            egui::RichText::new(
                "A matching SHA-256 verifies bytes against the supplied digest; it does not authenticate who supplied that digest.",
            )
            .color(MUTED)
            .size(11.0),
        );
    }

    fn text_json_view(&mut self, ui: &mut egui::Ui) {
        theme::section_title(ui, PixelIcon::Editor, "TEXT / JSON");
        ui.label(
            egui::RichText::new(
                "In-memory non-secret text cleanup and JSON validation/formatting. No output is written back automatically.",
            )
            .color(MUTED),
        );
        ui.label(
            egui::RichText::new(
                "Do not use this generic utility for passwords, Recovery Codes, Master Passphrases or decrypted Vault data.",
            )
            .color(WARN)
            .size(11.0),
        );
        ui.add_space(10.0);

        let selected_file = self.browser.selected.as_ref().and_then(|selected| {
            self.browser
                .entries
                .iter()
                .find(|entry| {
                    entry.kind == EntryKind::File && &entry.relative_path == selected
                })
                .map(|entry| entry.relative_path.clone())
        });

        theme::panel(ui, |ui| {
            theme::small_label(ui, "INPUT / EPHEMERAL");
            let response = ui.add(
                egui::TextEdit::multiline(&mut self.text_json.input)
                    .font(egui::TextStyle::Monospace)
                    .desired_width(f32::INFINITY)
                    .desired_rows(10),
            );
            if response.changed() {
                self.text_json.clear_result();
            }

            ui.horizontal_wrapped(|ui| {
                ui.label(
                    egui::RichText::new(format!(
                        "{} / {} input bytes",
                        self.text_json.input.len(),
                        text_json::MAX_UTILITY_INPUT_BYTES
                    ))
                    .color(MUTED)
                    .monospace()
                    .size(11.0),
                );

                if ui
                    .add_enabled(
                        selected_file.is_some(),
                        egui::Button::new("[LOAD FILES SELECTION]"),
                    )
                    .clicked()
                {
                    if let Some(path) = &selected_file {
                        match self.text_json.load_file(&self.workspace_root, path) {
                            Ok(()) => self.notice = "TEXT / JSON FILE LOADED".to_string(),
                            Err(error) => {
                                self.notice = format!("TEXT / JSON LOAD FAILED: {error}")
                            }
                        }
                    }
                }

                if ui.button("[GO TO FILES]").clicked() {
                    self.view = View::Files;
                }

                if ui.button("[CLEAR ALL]").clicked() {
                    self.text_json = TextJsonState::new();
                    self.notice = "TEXT / JSON CLEARED".to_string();
                }
            });
        });

        ui.add_space(8.0);

        theme::panel(ui, |ui| {
            theme::small_label(ui, "TEXT");
            ui.horizontal_wrapped(|ui| {
                if ui.button("[NORMALIZE LF]").clicked() {
                    let result = text_json::normalize_line_endings(&self.text_json.input);
                    match self
                        .text_json
                        .apply_text_result(result, "TEXT / LF NORMALIZED")
                    {
                        Ok(()) => self.notice = "TEXT LF NORMALIZED".to_string(),
                        Err(error) => self.notice = format!("TEXT FAILED: {error}"),
                    }
                }

                if ui.button("[CLEAN LINES]").clicked() {
                    let result = text_json::clean_text_lines(&self.text_json.input);
                    match self
                        .text_json
                        .apply_text_result(result, "TEXT / LF + TRAILING SPACE CLEAN")
                    {
                        Ok(()) => self.notice = "TEXT LINES CLEANED".to_string(),
                        Err(error) => self.notice = format!("TEXT FAILED: {error}"),
                    }
                }
            });

            ui.add_space(6.0);
            theme::small_label(ui, "JSON");
            ui.horizontal_wrapped(|ui| {
                if ui.button("[VALIDATE JSON]").clicked() {
                    match self.text_json.validate_json() {
                        Ok(kind) => {
                            self.notice = format!("JSON VALID / {}", kind.label());
                        }
                        Err(error) => self.notice = format!("JSON INVALID: {error}"),
                    }
                }

                if ui.button("[PRETTY JSON]").clicked() {
                    let result = text_json::pretty_json(&self.text_json.input);
                    match self.text_json.apply_text_result(result, "JSON / PRETTY") {
                        Ok(()) => self.notice = "JSON PRETTY READY".to_string(),
                        Err(error) => self.notice = format!("JSON FAILED: {error}"),
                    }
                }

                if ui.button("[COMPACT JSON]").clicked() {
                    let result = text_json::compact_json(&self.text_json.input);
                    match self.text_json.apply_text_result(result, "JSON / COMPACT") {
                        Ok(()) => self.notice = "JSON COMPACT READY".to_string(),
                        Err(error) => self.notice = format!("JSON FAILED: {error}"),
                    }
                }
            });
        });

        ui.add_space(8.0);

        theme::panel(ui, |ui| {
            theme::small_label(ui, "RESULT / EPHEMERAL");

            if let Some(error) = &self.text_json.error {
                ui.label(
                    egui::RichText::new("UTILITY ERROR")
                        .color(BAD)
                        .strong()
                        .monospace(),
                );
                ui.label(egui::RichText::new(error).color(MUTED).monospace());
                return;
            }

            if let Some(status) = &self.text_json.status {
                theme::status_badge(ui, status, OK);
                ui.add_space(4.0);
            }

            if self.text_json.output.is_empty() {
                ui.label(
                    egui::RichText::new("NO TRANSFORM OUTPUT")
                        .color(MUTED)
                        .monospace(),
                );
            } else {
                ui.add(
                    egui::TextEdit::multiline(&mut self.text_json.output)
                        .font(egui::TextStyle::Monospace)
                        .desired_width(f32::INFINITY)
                        .desired_rows(10)
                        .interactive(false),
                );
                ui.label(
                    egui::RichText::new(format!(
                        "{} output bytes",
                        self.text_json.output.len()
                    ))
                    .color(MUTED)
                    .monospace()
                    .size(11.0),
                );

                if ui.button("[USE OUTPUT AS INPUT]").clicked() {
                    self.text_json.use_output_as_input();
                    self.notice = "TEXT / JSON OUTPUT MOVED TO INPUT".to_string();
                }
            }
        });
    }

    fn compare_view(&mut self, ui: &mut egui::Ui) {
        theme::section_title(ui, PixelIcon::Compare, "COMPARE");
        ui.label(
            egui::RichText::new(
                "Read-only binary-safe comparison for two regular files inside the Workspace.",
            )
            .color(MUTED),
        );
        ui.add_space(10.0);

        let selected_file = self.browser.selected.as_ref().and_then(|selected| {
            self.browser
                .entries
                .iter()
                .find(|entry| {
                    entry.kind == EntryKind::File && &entry.relative_path == selected
                })
                .map(|entry| entry.relative_path.clone())
        });

        theme::panel(ui, |ui| {
            theme::small_label(ui, "FILE A / WORKSPACE-RELATIVE PATH");
            let left_response = ui.add(
                egui::TextEdit::singleline(&mut self.file_compare.left)
                    .font(egui::TextStyle::Monospace)
                    .desired_width(f32::INFINITY)
                    .hint_text("data/a.bin"),
            );
            if left_response.changed() {
                self.file_compare.clear_result();
            }

            ui.horizontal_wrapped(|ui| {
                if ui
                    .add_enabled(
                        selected_file.is_some(),
                        egui::Button::new("[USE FILES SELECTION AS A]"),
                    )
                    .clicked()
                {
                    if let Some(path) = &selected_file {
                        self.file_compare.set_left_path(path);
                    }
                }

                if ui.button("[GO TO FILES]").clicked() {
                    self.view = View::Files;
                }
            });

            ui.add_space(8.0);
            theme::small_label(ui, "FILE B / WORKSPACE-RELATIVE PATH");
            let right_response = ui.add(
                egui::TextEdit::singleline(&mut self.file_compare.right)
                    .font(egui::TextStyle::Monospace)
                    .desired_width(f32::INFINITY)
                    .hint_text("data/b.bin"),
            );
            if right_response.changed() {
                self.file_compare.clear_result();
            }

            ui.horizontal_wrapped(|ui| {
                if ui
                    .add_enabled(
                        selected_file.is_some(),
                        egui::Button::new("[USE FILES SELECTION AS B]"),
                    )
                    .clicked()
                {
                    if let Some(path) = &selected_file {
                        self.file_compare.set_right_path(path);
                    }
                }

                if ui.button("[SWAP A / B]").clicked() {
                    std::mem::swap(
                        &mut self.file_compare.left,
                        &mut self.file_compare.right,
                    );
                    self.file_compare.clear_result();
                }

                if ui.button("[CLEAR]").clicked() {
                    self.file_compare = FileCompareState::new();
                    self.notice = "COMPARE CLEARED".to_string();
                }
            });

            ui.add_space(8.0);
            let can_compare = !self.file_compare.left.trim().is_empty()
                && !self.file_compare.right.trim().is_empty();

            if ui
                .add_enabled(can_compare, egui::Button::new("[COMPARE FILES]"))
                .clicked()
            {
                match self.file_compare.compare(&self.workspace_root) {
                    Ok(true) => self.notice = "FILES EQUAL".to_string(),
                    Ok(false) => self.notice = "FILES DIFFERENT".to_string(),
                    Err(error) => self.notice = format!("COMPARE FAILED: {error}"),
                }
            }
        });

        ui.add_space(8.0);

        theme::panel(ui, |ui| {
            theme::small_label(ui, "RESULT");

            if let Some(error) = &self.file_compare.error {
                ui.label(
                    egui::RichText::new("COMPARE ERROR")
                        .color(BAD)
                        .strong()
                        .monospace(),
                );
                ui.label(egui::RichText::new(error).color(MUTED).monospace());
                return;
            }

            let Some(result) = self.file_compare.result else {
                ui.label(
                    egui::RichText::new("NO RESULT")
                        .color(MUTED)
                        .monospace(),
                );
                return;
            };

            theme::status_badge(
                ui,
                if result.equal { "EQUAL" } else { "DIFFERENT" },
                if result.equal { OK } else { BAD },
            );
            ui.add_space(6.0);
            theme::key_value(ui, "FILE A BYTES", &result.left_bytes.to_string());
            theme::key_value(ui, "FILE B BYTES", &result.right_bytes.to_string());

            let first_difference = result
                .first_difference_byte
                .map(|offset| offset.to_string())
                .unwrap_or_else(|| "NONE".to_string());
            theme::key_value(ui, "FIRST DIFFERENCE BYTE / 0-BASED", &first_difference);

            if let Some(text) = result.text {
                ui.add_space(6.0);
                theme::small_label(ui, "UTF-8 TEXT DETAIL");
                theme::key_value(ui, "FILE A LINES", &text.left_lines.to_string());
                theme::key_value(ui, "FILE B LINES", &text.right_lines.to_string());
                let line = text
                    .first_differing_line
                    .map(|line| line.to_string())
                    .unwrap_or_else(|| "NONE".to_string());
                theme::key_value(ui, "FIRST DIFFERING LINE / 1-BASED", &line);
            } else {
                ui.add_space(6.0);
                ui.label(
                    egui::RichText::new(
                        "Text detail unavailable: one file is binary/non-UTF-8 or exceeds the 4 MiB text-detail limit.",
                    )
                    .color(MUTED)
                    .size(11.0),
                );
            }
        });

        ui.add_space(8.0);
        ui.label(
            egui::RichText::new(
                "Comparison is read-only. Paths, results and file contents are not stored in generic session state.",
            )
            .color(MUTED)
            .size(11.0),
        );
    }

    fn search_view(&mut self, ui: &mut egui::Ui) {
        theme::section_title(ui, PixelIcon::Files, "SEARCH / DOCUMENTS");
        ui.label(
            egui::RichText::new(
                "Read-only literal search under workspace:/data/documents. Internal state, backups, repositories, identity and config are outside this search scope.",
            )
            .color(MUTED),
        );
        ui.add_space(10.0);

        theme::panel(ui, |ui| {
            theme::small_label(ui, "QUERY / EPHEMERAL");
            let response = ui.add(
                egui::TextEdit::singleline(&mut self.document_search.query)
                    .font(egui::TextStyle::Monospace)
                    .desired_width(f32::INFINITY)
                    .hint_text("literal text to find"),
            );
            if response.changed() {
                self.document_search.clear_result();
            }

            ui.horizontal_wrapped(|ui| {
                ui.label(
                    egui::RichText::new(format!(
                        "{} / {} query bytes",
                        self.document_search.query.len(),
                        document_search::MAX_SEARCH_QUERY_BYTES
                    ))
                    .color(MUTED)
                    .monospace()
                    .size(11.0),
                );

                let can_search = !self.document_search.query.trim().is_empty()
                    && self.document_search.query.len()
                        <= document_search::MAX_SEARCH_QUERY_BYTES
                    && !self.document_search.query.contains('\n')
                    && !self.document_search.query.contains('\r');

                if ui
                    .add_enabled(can_search, egui::Button::new("[SEARCH DOCUMENTS]"))
                    .clicked()
                {
                    match self.document_search.search(&self.workspace_root) {
                        Ok(count) => {
                            self.notice = format!("SEARCH COMPLETE / {count} RESULT(S)");
                        }
                        Err(error) => {
                            self.notice = format!("SEARCH FAILED: {error}");
                        }
                    }
                }

                if ui.button("[CLEAR]").clicked() {
                    self.document_search = DocumentSearchState::new();
                    self.notice = "SEARCH CLEARED".to_string();
                }
            });

            ui.add_space(4.0);
            ui.label(
                egui::RichText::new(format!(
                    "Limits: {} MiB/file · {} files · {} results · {} preview chars",
                    document_search::MAX_DOCUMENT_BYTES / (1024 * 1024),
                    document_search::MAX_FILES_CONSIDERED,
                    document_search::MAX_SEARCH_RESULTS,
                    document_search::MAX_PREVIEW_CHARS
                ))
                .color(MUTED)
                .monospace()
                .size(11.0),
            );
        });

        ui.add_space(8.0);

        let mut open_path: Option<PathBuf> = None;
        theme::panel(ui, |ui| {
            theme::small_label(ui, "RESULTS");

            if let Some(error) = &self.document_search.error {
                ui.label(
                    egui::RichText::new("SEARCH ERROR")
                        .color(BAD)
                        .strong()
                        .monospace(),
                );
                ui.label(egui::RichText::new(error).color(MUTED).monospace());
                return;
            }

            let Some(report) = &self.document_search.report else {
                ui.label(
                    egui::RichText::new("NO SEARCH YET")
                        .color(MUTED)
                        .monospace(),
                );
                return;
            };

            theme::key_value(ui, "MATCHING LINES", &report.matches.len().to_string());
            theme::key_value(
                ui,
                "FILES SEARCHED",
                &format!("{}/{}", report.files_searched, report.files_considered),
            );
            theme::key_value(ui, "SKIPPED LARGE", &report.skipped_large.to_string());
            theme::key_value(
                ui,
                "SKIPPED NON-UTF8",
                &report.skipped_non_utf8.to_string(),
            );
            theme::key_value(
                ui,
                "SKIPPED SYMLINKS",
                &report.skipped_symlinks.to_string(),
            );

            if report.result_limit_reached {
                ui.label(
                    egui::RichText::new("RESULT LIMIT REACHED / RESULTS ARE TRUNCATED")
                        .color(WARN)
                        .strong()
                        .monospace(),
                );
            }
            if report.file_limit_reached {
                ui.label(
                    egui::RichText::new("FILE SCAN LIMIT REACHED / SEARCH IS PARTIAL")
                        .color(WARN)
                        .strong()
                        .monospace(),
                );
            }

            ui.add_space(6.0);
            egui::ScrollArea::vertical()
                .max_height(360.0)
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    if report.matches.is_empty() {
                        ui.label(
                            egui::RichText::new("(no matches)")
                                .color(MUTED)
                                .monospace(),
                        );
                    }

                    for result in &report.matches {
                        ui.group(|ui| {
                            ui.horizontal_wrapped(|ui| {
                                if ui.button("[OPEN]").clicked() {
                                    open_path = Some(result.relative_path.clone());
                                }
                                ui.label(
                                    egui::RichText::new(format!(
                                        "{} : {}:{}",
                                        file_browser::display_path(&result.relative_path),
                                        result.line,
                                        result.column
                                    ))
                                    .color(TEXT)
                                    .monospace(),
                                );
                            });
                            ui.label(
                                egui::RichText::new(&result.preview)
                                    .color(MUTED)
                                    .monospace(),
                            );
                        });
                        ui.add_space(4.0);
                    }
                });
        });

        if let Some(path) = open_path {
            self.open_in_editor(path);
        }

        ui.add_space(8.0);
        ui.label(
            egui::RichText::new(
                "Search query, previews and results are ephemeral and are not stored in generic session state.",
            )
            .color(MUTED)
            .size(11.0),
        );
    }

    fn repositories_view(&mut self, ui: &mut egui::Ui) {
        theme::section_title(ui, PixelIcon::Repository, "REPOSITORIES");
        ui.label(
            egui::RichText::new(
                "Bounded read-only Git inspection under workspace:/repositories. No network, shell, arbitrary argv, staging, commits, checkout, reset, clean, stash, fetch, pull or push.",
            )
            .color(MUTED),
        );
        ui.add_space(10.0);

        let mut select_repository: Option<PathBuf> = None;
        theme::panel(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                if ui.button("[REFRESH LIST]").clicked() {
                    match self.repository_git.refresh_list(&self.workspace_root) {
                        Ok(count) => {
                            self.notice = format!("REPOSITORY LIST REFRESHED / {count} FOUND");
                        }
                        Err(error) => {
                            self.notice = format!("REPOSITORY LIST FAILED: {error}");
                        }
                    }
                }

                theme::key_value(
                    ui,
                    "FOUND",
                    &self.repository_git.repositories.len().to_string(),
                );
            });

            ui.add_space(6.0);
            egui::ScrollArea::vertical()
                .max_height(180.0)
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    if self.repository_git.repositories.is_empty() {
                        ui.label(
                            egui::RichText::new("(no supported repositories found)")
                                .color(MUTED)
                                .monospace(),
                        );
                    }

                    for repo in self.repository_git.repositories.clone() {
                        let selected = self.repository_git.selected.as_ref()
                            == Some(&repo.relative_path);
                        ui.horizontal_wrapped(|ui| {
                            if ui
                                .selectable_label(selected, format!("[{}]", repo.name))
                                .clicked()
                            {
                                select_repository = Some(repo.relative_path.clone());
                            }
                            ui.label(
                                egui::RichText::new(file_browser::display_path(
                                    &repo.relative_path,
                                ))
                                .color(MUTED)
                                .monospace()
                                .size(11.0),
                            );
                        });
                    }
                });
        });

        if let Some(path) = select_repository {
            self.repository_git.select(path);
            self.notice = "REPOSITORY SELECTED".to_string();
        }

        ui.add_space(8.0);
        let selected = self.repository_git.selected.clone();

        theme::panel(ui, |ui| {
            theme::small_label(ui, "SELECTED REPOSITORY");
            match &selected {
                Some(path) => {
                    ui.label(
                        egui::RichText::new(file_browser::display_path(path))
                            .color(TEXT)
                            .monospace(),
                    );
                }
                None => {
                    ui.label(egui::RichText::new("(none)").color(MUTED).monospace());
                }
            }

            ui.add_space(6.0);
            ui.horizontal_wrapped(|ui| {
                let enabled = selected.is_some();

                if ui
                    .add_enabled(enabled, egui::Button::new("[STATUS]"))
                    .clicked()
                {
                    match self.repository_git.refresh_status(&self.workspace_root) {
                        Ok(()) => self.notice = "GIT STATUS COMPLETE".to_string(),
                        Err(error) => self.notice = format!("GIT STATUS FAILED: {error}"),
                    }
                }

                if ui
                    .add_enabled(enabled, egui::Button::new("[RECENT COMMITS]"))
                    .clicked()
                {
                    match self.repository_git.run_output(
                        &self.workspace_root,
                        "RECENT COMMITS",
                        repository_git::recent_commits,
                    ) {
                        Ok(()) => self.notice = "GIT LOG COMPLETE".to_string(),
                        Err(error) => self.notice = format!("GIT LOG FAILED: {error}"),
                    }
                }

                if ui
                    .add_enabled(enabled, egui::Button::new("[WORKTREE DIFF STAT]"))
                    .clicked()
                {
                    match self.repository_git.run_output(
                        &self.workspace_root,
                        "WORKTREE DIFF STAT",
                        repository_git::worktree_diff_stat,
                    ) {
                        Ok(()) => self.notice = "WORKTREE DIFF COMPLETE".to_string(),
                        Err(error) => {
                            self.notice = format!("WORKTREE DIFF FAILED: {error}")
                        }
                    }
                }

                if ui
                    .add_enabled(enabled, egui::Button::new("[STAGED DIFF STAT]"))
                    .clicked()
                {
                    match self.repository_git.run_output(
                        &self.workspace_root,
                        "STAGED DIFF STAT",
                        repository_git::staged_diff_stat,
                    ) {
                        Ok(()) => self.notice = "STAGED DIFF COMPLETE".to_string(),
                        Err(error) => self.notice = format!("STAGED DIFF FAILED: {error}"),
                    }
                }
            });

            ui.add_space(4.0);
            ui.label(
                egui::RichText::new(format!(
                    "Limits: {} repositories · {} status rows · {} KiB retained Git output · {}s runtime",
                    repository_git::MAX_REPOSITORIES,
                    repository_git::MAX_STATUS_ENTRIES,
                    repository_git::MAX_GIT_OUTPUT_BYTES / 1024,
                    repository_git::MAX_GIT_RUNTIME.as_secs()
                ))
                .color(MUTED)
                .monospace()
                .size(11.0),
            );
        });

        ui.add_space(8.0);

        if let Some(error) = &self.repository_git.error {
            theme::panel(ui, |ui| {
                ui.label(
                    egui::RichText::new("GIT ERROR")
                        .color(BAD)
                        .strong()
                        .monospace(),
                );
                ui.label(egui::RichText::new(error).color(MUTED).monospace());
            });
            ui.add_space(8.0);
        }

        if let Some(status) = &self.repository_git.status {
            theme::panel(ui, |ui| {
                theme::small_label(ui, "STATUS");
                theme::key_value(ui, "BRANCH", &status.branch);
                theme::key_value(ui, "STAGED", &status.staged.to_string());
                theme::key_value(ui, "UNSTAGED", &status.unstaged.to_string());
                theme::key_value(ui, "UNTRACKED", &status.untracked.to_string());
                theme::key_value(ui, "CONFLICTED", &status.conflicted.to_string());

                if status.output_truncated || status.entry_limit_reached {
                    ui.label(
                        egui::RichText::new("STATUS OUTPUT TRUNCATED BY LOCAL LIMIT")
                            .color(WARN)
                            .strong()
                            .monospace(),
                    );
                }

                ui.add_space(6.0);
                egui::ScrollArea::vertical()
                    .max_height(280.0)
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        if status.entries.is_empty() {
                            ui.label(
                                egui::RichText::new("(working tree clean)")
                                    .color(OK)
                                    .monospace(),
                            );
                        }

                        for entry in &status.entries {
                            ui.horizontal_wrapped(|ui| {
                                ui.label(
                                    egui::RichText::new(&entry.code)
                                        .color(if entry.code == "??" { WARN } else { TEXT })
                                        .monospace()
                                        .strong(),
                                );
                                ui.label(
                                    egui::RichText::new(&entry.path)
                                        .color(MUTED)
                                        .monospace(),
                                );
                            });
                        }
                    });
            });
            ui.add_space(8.0);
        }

        if let (Some(label), Some(output)) = (
            self.repository_git.output_label,
            self.repository_git.output.as_ref(),
        ) {
            theme::panel(ui, |ui| {
                theme::small_label(ui, label);
                if output.truncated {
                    ui.label(
                        egui::RichText::new("OUTPUT TRUNCATED BY LOCAL 1 MiB LIMIT")
                            .color(WARN)
                            .strong()
                            .monospace(),
                    );
                }

                let text = if output.text.trim().is_empty() {
                    "(no output)"
                } else {
                    output.text.as_str()
                };

                egui::ScrollArea::vertical()
                    .max_height(320.0)
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        ui.label(
                            egui::RichText::new(text)
                                .color(TEXT)
                                .monospace(),
                        );
                    });
            });
        }

        ui.add_space(8.0);
        ui.label(
            egui::RichText::new(
                "Repository selection and Git output are ephemeral. Generic session state stores only the REPOSITORIES navigation view.",
            )
            .color(MUTED)
            .size(11.0),
        );
    }

    fn backup_view(&mut self, ui: &mut egui::Ui) {
        theme::section_title(ui, PixelIcon::Backup, "BACKUP / RESTORE");
        ui.label(
            egui::RichText::new(
                "Generic local Workspace snapshots. Scope: data, repositories, identity, config and state/autosave. backups itself and state/session.v1 are excluded. This is separate from encrypted Secret Backup / Restore.",
            )
            .color(MUTED),
        );
        ui.add_space(10.0);

        let mut create_requested = false;
        let mut refresh_requested = false;
        let mut select_backup: Option<PathBuf> = None;
        let mut verify_requested = false;
        let mut restore_requested = false;

        theme::panel(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                if ui.button("[CREATE BACKUP]").clicked() {
                    create_requested = true;
                }
                if ui.button("[REFRESH LIST]").clicked() {
                    refresh_requested = true;
                }
                theme::key_value(
                    ui,
                    "SNAPSHOTS",
                    &self.workspace_backup.backups.len().to_string(),
                );
            });

            ui.add_space(4.0);
            ui.label(
                egui::RichText::new(
                    "CREATE stores a recovery draft first when the currently open Editor buffer is dirty, so that draft is included in state/autosave.",
                )
                .color(MUTED)
                .size(11.0),
            );
        });

        if create_requested {
            match self.persist_dirty_editor_recovery_for_backup() {
                Ok(()) => match self.workspace_backup.create(&self.workspace_root) {
                    Ok(created) => {
                        self.notice = format!("BACKUP CREATED / {}", created.name);
                    }
                    Err(error) => {
                        self.notice = format!("BACKUP CREATE FAILED: {error}");
                    }
                },
                Err(error) => {
                    self.notice = format!("BACKUP PREPARE FAILED: {error}");
                }
            }
        }

        if refresh_requested {
            match self.workspace_backup.refresh_list(&self.workspace_root) {
                Ok(count) => self.notice = format!("BACKUP LIST REFRESHED / {count} FOUND"),
                Err(error) => self.notice = format!("BACKUP LIST FAILED: {error}"),
            }
        }

        ui.add_space(8.0);
        theme::panel(ui, |ui| {
            theme::small_label(ui, "LOCAL SNAPSHOTS");
            egui::ScrollArea::vertical()
                .max_height(210.0)
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    if self.workspace_backup.backups.is_empty() {
                        ui.label(
                            egui::RichText::new("(no workspace backups)")
                                .color(MUTED)
                                .monospace(),
                        );
                    }

                    for backup in self.workspace_backup.backups.clone() {
                        let selected =
                            self.workspace_backup.selected.as_ref() == Some(&backup.relative_path);
                        ui.horizontal_wrapped(|ui| {
                            if ui
                                .selectable_label(selected, format!("[{}]", backup.name))
                                .clicked()
                            {
                                select_backup = Some(backup.relative_path.clone());
                            }
                            ui.label(
                                egui::RichText::new(format_bytes(backup.bytes))
                                    .color(MUTED)
                                    .monospace()
                                    .size(11.0),
                            );
                        });
                    }
                });
        });

        if let Some(path) = select_backup {
            self.workspace_backup.select(path);
            self.notice = "BACKUP SELECTED".to_string();
        }

        ui.add_space(8.0);
        let selected = self.workspace_backup.selected.clone();

        theme::panel(ui, |ui| {
            theme::small_label(ui, "SELECTED SNAPSHOT");
            match &selected {
                Some(path) => ui.label(
                    egui::RichText::new(file_browser::display_path(path))
                        .color(TEXT)
                        .monospace(),
                ),
                None => ui.label(egui::RichText::new("(none)").color(MUTED).monospace()),
            };

            ui.add_space(6.0);
            if ui
                .add_enabled(selected.is_some(), egui::Button::new("[VERIFY BACKUP]"))
                .clicked()
            {
                verify_requested = true;
            }

            if let Some(summary) = self.workspace_backup.summary {
                ui.add_space(6.0);
                theme::key_value(ui, "DIRECTORIES", &summary.directories.to_string());
                theme::key_value(ui, "FILES", &summary.files.to_string());
                theme::key_value(
                    ui,
                    "FILE BYTES",
                    &format_bytes(summary.total_file_bytes),
                );
                theme::key_value(
                    ui,
                    "ARCHIVE SIZE",
                    &format_bytes(summary.archive_bytes),
                );
                ui.label(
                    egui::RichText::new("FORMAT + FILE HASHES + ARCHIVE HASH VERIFIED")
                        .color(OK)
                        .strong()
                        .monospace(),
                );
            }
        });

        if verify_requested {
            match self.workspace_backup.verify(&self.workspace_root) {
                Ok(_) => self.notice = "BACKUP VERIFIED".to_string(),
                Err(error) => self.notice = format!("BACKUP VERIFY FAILED: {error}"),
            }
        }

        if let Some(error) = &self.workspace_backup.error {
            ui.add_space(8.0);
            theme::panel(ui, |ui| {
                ui.label(
                    egui::RichText::new("BACKUP ERROR")
                        .color(BAD)
                        .strong()
                        .monospace(),
                );
                ui.label(egui::RichText::new(error).color(MUTED).monospace());
            });
        }

        ui.add_space(8.0);
        theme::panel(ui, |ui| {
            theme::small_label(ui, "RESTORE / DESTRUCTIVE");

            ui.label(
                egui::RichText::new(
                    "Restore replaces data/, repositories/, identity/, config/ and state/autosave/ with the selected verified snapshot. Current backups/ and state/session.v1 remain in place.",
                )
                .color(WARN),
            );
            ui.label(
                egui::RichText::new(
                    "The selected archive is fully extracted and integrity-verified before target replacement. Existing managed roots are held in rollback staging while replacement runs.",
                )
                .color(MUTED)
                .size(11.0),
            );

            ui.add_space(6.0);
            theme::small_label(ui, "TYPE RESTORE TO ENABLE");
            ui.add(
                egui::TextEdit::singleline(&mut self.workspace_backup.restore_confirmation)
                    .font(egui::TextStyle::Monospace)
                    .desired_width(220.0)
                    .hint_text("RESTORE"),
            );

            let editor_safe = !self.editor.dirty && self.editor.pending_recovery.is_none();
            if !editor_safe {
                ui.label(
                    egui::RichText::new(
                        "Resolve the currently open Editor's dirty/pending recovery state before restoring.",
                    )
                    .color(WARN)
                    .strong(),
                );
            }

            let can_restore = selected.is_some()
                && self.workspace_backup.restore_confirmation == "RESTORE"
                && editor_safe;

            if ui
                .add_enabled(can_restore, egui::Button::new("[RESTORE SELECTED BACKUP]"))
                .clicked()
            {
                restore_requested = true;
            }
        });

        if restore_requested {
            if let Some(path) = selected {
                match workspace_backup::restore_backup(&self.workspace_root, &path) {
                    Ok(summary) => {
                        self.reset_after_workspace_restore(path, summary);
                        self.view = View::Backup;
                        self.notice = "WORKSPACE RESTORED / UI STATE RESET".to_string();
                    }
                    Err(error) => {
                        self.workspace_backup.summary = None;
                        self.workspace_backup.error = Some(error.to_string());
                        self.workspace_backup.restore_confirmation.clear();
                        self.notice = format!("WORKSPACE RESTORE FAILED: {error}");
                    }
                }
            }
        }

        ui.add_space(8.0);
        ui.label(
            egui::RichText::new(
                "Backup selection, verification summary and RESTORE confirmation are ephemeral. Generic session state stores only the BACKUP navigation view.",
            )
            .color(MUTED)
            .size(11.0),
        );
    }

    fn workspace(&mut self, ui: &mut egui::Ui) {
        let summary = workspace::summary(&self.workspace_root);

        theme::section_title(ui, PixelIcon::Workspace, "WORKSPACE");
        theme::panel(ui, |ui| {
            theme::key_value(ui, "PATH", &self.workspace_root.display().to_string());
            theme::key_value(
                ui,
                "STATE",
                if summary.initialized {
                    "READY"
                } else {
                    "NOT INITIALIZED"
                },
            );
            theme::key_value(
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
            let enabled = self.lan_access.is_enabled();
            theme::status_badge(
                ui,
                if enabled {
                    "LAN CONTROL ON"
                } else {
                    "LAN CONTROL OFF"
                },
                if enabled { WARN } else { OK },
            );
            ui.add_space(6.0);
            ui.label(
                "LAN intent remains process-local and resets OFF on every launch.",
            );
            ui.add_space(6.0);
            ui.label(
                "The gateway remains bound to 127.0.0.1. Enabling this control does not create a LAN listener; device pairing is prepared before any LAN listener is allowed.",
            );
            ui.add_space(10.0);

            if enabled {
                if theme::pixel_button(
                    ui,
                    PixelIcon::Mobile,
                    "DISABLE LAN CONTROL",
                    false,
                    true,
                    230.0,
                )
                .clicked()
                {
                    self.lan_access.disable();
                    self.device_pairing.cancel();
                    self.pairing_invitation = None;
                    self.paired_device = None;
                    self.notice = "LAN CONTROL OFF / PAIRING CLEARED / LOOPBACK ONLY".to_string();
                }
            } else if theme::pixel_button(
                ui,
                PixelIcon::Mobile,
                "ENABLE FOR THIS RUN",
                false,
                true,
                230.0,
            )
            .clicked()
            {
                self.lan_access.enable_for_this_run();
                self.notice = "LAN CONTROL ON / LISTENER STILL LOCKED".to_string();
            }

            ui.add_space(14.0);
            theme::small_label(ui, "DEVICE PAIRING");
            ui.add_space(4.0);

            if !self.lan_access.is_enabled() {
                ui.label(
                    egui::RichText::new("Enable LAN control before starting a pairing session.")
                        .color(MUTED),
                );
                return;
            }

            if let Some(paired) = self.paired_device.as_ref() {
                theme::status_badge(ui, "PAIRED FOR THIS RUN", WARN);
                theme::key_value(ui, "DEVICE", paired.device_name());
                theme::key_value(ui, "KEY", "Ed25519");
                theme::key_value(ui, "FINGERPRINT", &paired.fingerprint_hex());
                ui.label(
                    egui::RichText::new(
                        "This approval is process-local only. Persistent trusted-device storage is the next M3 task.",
                    )
                    .color(MUTED),
                );
                ui.add_space(6.0);
                if ui.button("CLEAR RUN-LOCAL PAIRING").clicked() {
                    self.paired_device = None;
                    self.device_pairing.cancel();
                    self.pairing_invitation = None;
                    self.notice = "RUN-LOCAL PAIRING CLEARED".to_string();
                }
                return;
            }

            let now = Instant::now();
            let status = self.device_pairing.status(now);

            match status {
                PairingStatus::Idle => {
                    self.pairing_invitation = None;
                    ui.label(
                        egui::RichText::new(
                            "PAIRING CORE READY / TRANSPORT LOCKED. No non-loopback listener exists yet.",
                        )
                        .color(MUTED),
                    );
                    ui.add_space(6.0);
                    if ui.button("START DEVICE PAIRING").clicked() {
                        match self.device_pairing.begin(&self.lan_access, now) {
                            Ok(invitation) => {
                                self.pairing_invitation = Some(invitation);
                                self.notice =
                                    "PAIRING STARTED / CODE IS SHORT-LIVED / LISTENER STILL LOCKED"
                                        .to_string();
                            }
                            Err(error) => {
                                self.notice = format!("PAIRING START FAILED: {error}");
                            }
                        }
                    }
                }
                PairingStatus::Waiting {
                    attempts_remaining,
                    remaining,
                } => {
                    theme::status_badge(ui, "WAITING FOR DEVICE", WARN);
                    if let Some(invitation) = self.pairing_invitation.as_ref() {
                        theme::key_value(ui, "PAIRING CODE", invitation.code().expose_secret());
                        theme::key_value(
                            ui,
                            "EXPIRES",
                            &format!("{}s", remaining.as_secs().saturating_add(1)),
                        );
                        theme::key_value(
                            ui,
                            "PROOF ATTEMPTS",
                            &attempts_remaining.to_string(),
                        );
                    } else {
                        ui.label(
                            egui::RichText::new(
                                "Pairing is active, but the one-time display code is no longer available. Cancel and start again.",
                            )
                            .color(BAD),
                        );
                    }
                    ui.label(
                        egui::RichText::new(
                            "The code is entered on the device. The future transport submits only an HMAC proof plus an Ed25519 proof-of-possession; the raw code is not a LAN request field.",
                        )
                        .color(MUTED),
                    );
                    ui.add_space(6.0);
                    if ui.button("CANCEL PAIRING").clicked() {
                        self.device_pairing.cancel();
                        self.pairing_invitation = None;
                        self.notice = "PAIRING CANCELLED".to_string();
                    }
                }
                PairingStatus::AwaitingApproval {
                    summary,
                    remaining,
                } => {
                    self.pairing_invitation = None;
                    theme::status_badge(ui, "APPROVAL REQUIRED", WARN);
                    theme::key_value(ui, "DEVICE", summary.device_name());
                    theme::key_value(ui, "FINGERPRINT", &summary.fingerprint_hex());
                    theme::key_value(
                        ui,
                        "EXPIRES",
                        &format!("{}s", remaining.as_secs().saturating_add(1)),
                    );
                    ui.label(
                        egui::RichText::new(
                            "Cryptographic proof succeeded. Explicit desktop approval is still required, and approval remains process-local until trusted-device persistence exists.",
                        )
                        .color(MUTED),
                    );
                    ui.add_space(6.0);
                    ui.horizontal(|ui| {
                        if ui.button("APPROVE DEVICE FOR THIS RUN").clicked() {
                            match self.device_pairing.approve(&self.lan_access, now) {
                                Ok(candidate) => {
                                    self.notice =
                                        format!("DEVICE PAIRED FOR THIS RUN: {}", candidate.device_name());
                                    self.paired_device = Some(candidate);
                                }
                                Err(error) => {
                                    self.notice = format!("PAIRING APPROVAL FAILED: {error}");
                                }
                            }
                        }
                        if ui.button("REJECT DEVICE").clicked() {
                            self.device_pairing.reject_candidate();
                            self.pairing_invitation = None;
                            self.notice = "PAIRING CANDIDATE REJECTED".to_string();
                        }
                    });
                }
            }
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
                    View::HashVerify => self.hash_verify_view(ui),
                    View::TextJson => self.text_json_view(ui),
                    View::Compare => self.compare_view(ui),
                    View::Search => self.search_view(ui),
                    View::Repositories => self.repositories_view(ui),
                    View::Backup => self.backup_view(ui),
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
        ToolId::TextJson => PixelIcon::Editor,
        ToolId::Compare => PixelIcon::Compare,
        ToolId::Search => PixelIcon::Files,
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    const SHA256_ABC: &str =
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";
    const SHA256_EMPTY: &str =
        "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";

    fn temporary_workspace(label: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "dangomushi-hash-ui-{label}-{}-{unique}",
            std::process::id()
        ));
        fs::create_dir_all(&root).expect("workspace");
        root
    }

    #[test]
    fn hash_ui_state_calculates_match_and_mismatch() {
        let root = temporary_workspace("state");
        fs::write(root.join("sample.bin"), b"abc").expect("seed");

        let mut state = HashVerifyState::new();
        state.set_target_path(Path::new("sample.bin"));
        state.calculate(&root).expect("calculate");

        assert_eq!(state.status, Some(HashResultState::Calculated));
        assert_eq!(
            state.actual.expect("actual").sha256.to_string(),
            SHA256_ABC
        );

        state.expected = SHA256_ABC.to_string();
        assert!(state.verify(&root).expect("verify match"));
        assert_eq!(state.status, Some(HashResultState::Match));

        state.expected = SHA256_EMPTY.to_string();
        assert!(!state.verify(&root).expect("verify mismatch"));
        assert_eq!(state.status, Some(HashResultState::Mismatch));

        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn changing_hash_target_clears_stale_result() {
        let root = temporary_workspace("target");
        fs::write(root.join("first.bin"), b"abc").expect("first");
        fs::write(root.join("second.bin"), b"other").expect("second");

        let mut state = HashVerifyState::new();
        state.set_target_path(Path::new("first.bin"));
        state.calculate(&root).expect("calculate");
        assert!(state.actual.is_some());

        state.set_target_path(Path::new("second.bin"));
        assert!(state.actual.is_none());
        assert!(state.status.is_none());
        assert!(state.error.is_none());

        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn text_json_ui_state_loads_and_transforms_without_writing_source() {
        let root = temporary_workspace("text-json");
        let path = root.join("sample.json");
        fs::write(&path, "{ \"b\" : 2, \"a\" : 1 }\r\n").expect("seed");

        let mut state = TextJsonState::new();
        state
            .load_file(&root, Path::new("sample.json"))
            .expect("load");

        let original = fs::read_to_string(&path).expect("source before");
        let result = text_json::compact_json(&state.input);
        state
            .apply_text_result(result, "JSON / COMPACT")
            .expect("compact");

        assert_eq!(state.output, "{\"b\":2,\"a\":1}");
        assert_eq!(fs::read_to_string(&path).expect("source after"), original);

        state.use_output_as_input();
        assert_eq!(state.input, "{\"b\":2,\"a\":1}");
        assert!(state.output.is_empty());

        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn compare_ui_state_compares_files_without_writing_them() {
        let root = temporary_workspace("compare");
        let left = root.join("left.txt");
        let right = root.join("right.txt");
        fs::write(&left, "one\ntwo\n").expect("left");
        fs::write(&right, "one\nTWO\n").expect("right");

        let left_before = fs::read(&left).expect("left before");
        let right_before = fs::read(&right).expect("right before");

        let mut state = FileCompareState::new();
        state.set_left_path(Path::new("left.txt"));
        state.set_right_path(Path::new("right.txt"));

        assert!(!state.compare(&root).expect("compare"));
        let result = state.result.expect("result");
        assert_eq!(result.first_difference_byte, Some(4));
        assert_eq!(
            result.text.and_then(|text| text.first_differing_line),
            Some(2)
        );

        assert_eq!(fs::read(&left).expect("left after"), left_before);
        assert_eq!(fs::read(&right).expect("right after"), right_before);

        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn document_search_ui_state_finds_documents_without_writing_source() {
        let root = temporary_workspace("search");
        fs::create_dir_all(root.join("data/documents")).expect("documents");
        let path = root.join("data/documents/note.txt");
        fs::write(&path, "alpha\nneedle here\nomega\n").expect("seed");
        let before = fs::read(&path).expect("before");

        let mut state = DocumentSearchState::new();
        state.query = "needle".to_string();
        assert_eq!(state.search(&root).expect("search"), 1);

        let report = state.report.expect("report");
        assert_eq!(report.matches.len(), 1);
        assert_eq!(report.matches[0].line, 2);
        assert_eq!(fs::read(&path).expect("after"), before);

        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn backup_view_maps_to_navigation_only_session_state() {
        assert_eq!(View::Backup.session_view(), SessionView::Backup);
        assert_eq!(View::from_session(SessionView::Backup), View::Backup);
    }

    #[test]
    fn backup_ui_state_creates_and_verifies_snapshot() {
        let root = temporary_workspace("backup");
        workspace::initialize(&root).expect("workspace init");
        fs::write(root.join("data/documents/note.txt"), "snapshot").expect("document");

        let mut state = BackupUiState::new(&root);
        let created = state.create(&root).expect("create");
        assert_eq!(state.selected.as_ref(), Some(&created.relative_path));

        let summary = state.verify(&root).expect("verify");
        assert_eq!(summary.files, 1);
        assert!(summary.directories >= 5);

        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn repositories_view_maps_to_navigation_only_session_state() {
        assert_eq!(
            View::Repositories.session_view(),
            SessionView::Repositories
        );
        assert_eq!(
            View::from_session(SessionView::Repositories),
            View::Repositories
        );
    }

    #[test]
    fn search_view_maps_to_navigation_only_session_state() {
        assert_eq!(View::Search.session_view(), SessionView::Search);
        assert_eq!(View::from_session(SessionView::Search), View::Search);
    }

    #[test]
    fn compare_view_maps_to_navigation_only_session_state() {
        assert_eq!(View::Compare.session_view(), SessionView::Compare);
        assert_eq!(View::from_session(SessionView::Compare), View::Compare);
    }

    #[test]
    fn text_json_view_maps_to_navigation_only_session_state() {
        assert_eq!(View::TextJson.session_view(), SessionView::TextJson);
        assert_eq!(View::from_session(SessionView::TextJson), View::TextJson);
    }

    #[test]
    fn hash_verify_view_maps_to_navigation_only_session_state() {
        assert_eq!(
            View::HashVerify.session_view(),
            SessionView::HashVerify
        );
        assert_eq!(
            View::from_session(SessionView::HashVerify),
            View::HashVerify
        );
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

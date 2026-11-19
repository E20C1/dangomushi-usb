use crate::clipboard_hygiene::{
    ClipboardPolicy, ClipboardPoll, ClipboardSecret, SecretClipboard, SystemClipboard,
};
use crate::device_pairing::{DevicePairing, PairedDeviceCandidate, PairingInvitation, PairingStatus};
use crate::device_permissions::{self, DevicePermissionStore, PermissionScope};
use crate::device_revocation;
use crate::document_search;
use crate::file_browser::{self, EntryKind, FileEntry};
use crate::file_compare;
use crate::hash_verify;
use crate::internet_p2p_policy;
use crate::lan_access_control::LanEnableControl;
use crate::network_safety::{self, NetworkEnvironment, NetworkSafetyAssessment};
use crate::offline_docs;
use crate::password_generator::{self, GeneratedPassword, GeneratorConfig, MAX_PASSWORD_LENGTH, MIN_PASSWORD_LENGTH};
use crate::peer_identity;
use crate::repository_git;
use crate::session_state::{self, SessionState, SessionView};
use crate::text_editor;
use crate::text_json;
use crate::tool_registry::{self, ToolId, ToolStatus};
use crate::trusted_devices::{self, TrustedDevice};
use crate::trusted_peers;
use crate::workspace;
use crate::workspace_backup;
use crossterm::cursor::Show;
use crossterm::event::{
    self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEvent, KeyEventKind,
    KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};
use crossterm::execute;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, Paragraph, Wrap};
use ratatui::{Frame, Terminal};
use std::io::{self, stdout, Stdout};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

const AUTOSAVE_DELAY: Duration = Duration::from_secs(2);
const EVENT_POLL: Duration = Duration::from_millis(100);

type TuiTerminal = Terminal<CrosstermBackend<Stdout>>;

pub fn run() -> io::Result<()> {
    let mut terminal = TerminalGuard::enter()?;
    let mut app = App::new();
    let result = app.run(&mut terminal.terminal);
    drop(terminal);
    result
}

struct TerminalGuard {
    terminal: TuiTerminal,
}

impl TerminalGuard {
    fn enter() -> io::Result<Self> {
        enable_raw_mode()?;
        let mut out = stdout();
        if let Err(error) = execute!(out, EnterAlternateScreen, EnableMouseCapture) {
            let _ = disable_raw_mode();
            return Err(error);
        }
        let backend = CrosstermBackend::new(out);
        let terminal = Terminal::new(backend)?;
        Ok(Self { terminal })
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
        let _ = execute!(
            self.terminal.backend_mut(),
            DisableMouseCapture,
            LeaveAlternateScreen,
            Show
        );
        let _ = self.terminal.show_cursor();
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum View {
    Home,
    Tools,
    PasswordGenerator,
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
    const PRIMARY: [Self; 7] = [
        Self::Home,
        Self::Tools,
        Self::Files,
        Self::Editor,
        Self::Workspace,
        Self::Mobile,
        Self::System,
    ];

    fn label(self) -> &'static str {
        match self {
            Self::Home => "HOME",
            Self::Tools => "TOOLS",
            Self::PasswordGenerator => "PASSWORD",
            Self::Files => "FILES",
            Self::Editor => "EDITOR",
            Self::HashVerify => "HASH / VERIFY",
            Self::TextJson => "TEXT / JSON",
            Self::Compare => "COMPARE",
            Self::Search => "SEARCH",
            Self::Repositories => "REPOSITORIES",
            Self::Backup => "BACKUP",
            Self::Workspace => "WORKSPACE",
            Self::Mobile => "MOBILE",
            Self::System => "SYSTEM",
        }
    }

    fn session_view(self) -> SessionView {
        match self {
            Self::Home => SessionView::Home,
            Self::Tools | Self::PasswordGenerator => SessionView::Tools,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Zone {
    Navigation,
    Content,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ControlId {
    Nav(View),
    Tool(usize),
    FileUp,
    FileRefresh,
    FileEntry(usize),
    FileOpen,
    FileHash,
    FileTextJson,
    FileCompareA,
    FileCompareB,
    EditorEdit,
    EditorSave,
    EditorDiscard,
    EditorRestoreRecovery,
    EditorDiscardRecovery,
    HashTarget,
    HashExpected,
    HashUseSelection,
    HashCalculate,
    HashVerify,
    HashClear,
    TextInput,
    TextLoadSelection,
    TextNormalize,
    TextClean,
    JsonValidate,
    JsonPretty,
    JsonCompact,
    TextUseOutput,
    TextClear,
    PasswordLengthDown,
    PasswordLengthUp,
    PasswordUpper,
    PasswordLower,
    PasswordDigits,
    PasswordSymbols,
    PasswordRequireEach,
    PasswordAmbiguous,
    PasswordSymbolSet,
    PasswordGenerate,
    PasswordCopy,
    PasswordClear,
    CompareLeft,
    CompareRight,
    CompareUseA,
    CompareUseB,
    CompareSwap,
    CompareRun,
    CompareClear,
    SearchQuery,
    SearchRun,
    SearchClear,
    SearchResult(usize),
    RepoRefresh,
    RepoEntry(usize),
    RepoStatus,
    RepoLog,
    RepoWorktree,
    RepoStaged,
    BackupCreate,
    BackupRefresh,
    BackupEntry(usize),
    BackupVerify,
    BackupConfirm,
    BackupRestore,
    WorkspaceInitialize,
    MobileRefreshNetwork,
    MobileToggleLan,
    PeerInitialize,
    PeerRefresh,
    DeviceEdit(usize),
    DeviceRevoke(usize),
    PermissionToggle(PermissionScope),
    PermissionSave,
    PermissionCancel,
    RevocationConfirm,
    RevocationCommit,
    RevocationCancel,
    PairingStart,
    PairingCancel,
    PairingApprove,
    PairingReject,
    PairingTrust,
    PairingClear,
    SystemPrevDoc,
    SystemNextDoc,
}

#[derive(Debug, Clone)]
struct Control {
    id: ControlId,
    zone: Zone,
    label: String,
    enabled: bool,
}

impl Control {
    fn nav(view: View) -> Self {
        Self {
            id: ControlId::Nav(view),
            zone: Zone::Navigation,
            label: view.label().to_string(),
            enabled: true,
        }
    }

    fn content(id: ControlId, label: impl Into<String>, enabled: bool) -> Self {
        Self {
            id,
            zone: Zone::Content,
            label: label.into(),
            enabled,
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct HitTarget {
    rect: Rect,
    control_index: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EditTarget {
    Editor,
    HashTarget,
    HashExpected,
    TextInput,
    PasswordSymbolSet,
    CompareLeft,
    CompareRight,
    SearchQuery,
    BackupConfirm,
    RevocationConfirm,
}

struct EditMode {
    target: EditTarget,
    cursor: usize,
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
            Err(error) => self.error = Some(error.to_string()),
        }
    }

    fn up(&mut self, root: &Path) {
        if !self.current_dir.as_os_str().is_empty() {
            self.navigate(root, file_browser::parent(&self.current_dir));
        }
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

    fn selected_entry(&self) -> Option<&FileEntry> {
        let selected = self.selected.as_deref()?;
        self.entries
            .iter()
            .find(|entry| entry.relative_path.as_path() == selected)
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
        self.recovery_status = if self.dirty {
            "RECOVERY PENDING".to_string()
        } else {
            "CLEAN".to_string()
        };
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
        let path = self
            .path
            .as_ref()
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "no editor file is open"))?;
        text_editor::write_recovery(root, path, &self.buffer)?;
        self.last_change = None;
        self.recovery_status = "RECOVERY STORED".to_string();
        Ok(true)
    }

    fn save(&mut self, root: &Path) -> io::Result<()> {
        let path = self
            .path
            .as_ref()
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "no editor file is open"))?;
        let disk_now = text_editor::read_text(root, path)?;
        if disk_now != self.saved_text {
            text_editor::write_recovery(root, path, &self.buffer)?;
            return Err(io::Error::new(
                io::ErrorKind::Other,
                "file changed on disk; recovery stored instead of overwriting",
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
        let path = self
            .path
            .as_ref()
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "no editor file is open"))?;
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

    fn clear_result(&mut self) {
        self.actual = None;
        self.status = None;
        self.error = None;
    }

    fn set_target_path(&mut self, path: &Path) {
        self.target = path.to_string_lossy().into_owned();
        self.clear_result();
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

    fn clear_result(&mut self) {
        self.result = None;
        self.error = None;
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

    fn refresh_list(&mut self, root: &Path) -> Result<usize, repository_git::RepositoryGitError> {
        match repository_git::list_repositories(root) {
            Ok(repositories) => {
                let count = repositories.len();
                self.repositories = repositories;
                self.error = None;
                Ok(count)
            }
            Err(error) => {
                self.repositories.clear();
                self.selected = None;
                self.status = None;
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

    fn refresh_list(&mut self, root: &Path) -> Result<usize, workspace_backup::WorkspaceBackupError> {
        match workspace_backup::list_backups(root) {
            Ok(backups) => {
                let count = backups.len();
                self.backups = backups;
                self.error = None;
                Ok(count)
            }
            Err(error) => {
                self.backups.clear();
                self.selected = None;
                self.summary = None;
                self.error = Some(error.to_string());
                Err(error)
            }
        }
    }
}

struct PasswordState {
    config: GeneratorConfig,
    generated: Option<GeneratedPassword>,
    error: Option<String>,
}

impl PasswordState {
    fn new() -> Self {
        Self {
            config: GeneratorConfig::default(),
            generated: None,
            error: None,
        }
    }

    fn clear(&mut self) {
        self.generated = None;
        self.error = None;
    }
}

struct App {
    view: View,
    workspace_root: PathBuf,
    browser: FileBrowserState,
    editor: EditorState,
    hash: HashVerifyState,
    text_json: TextJsonState,
    compare: FileCompareState,
    search: DocumentSearchState,
    repositories: RepositoryUiState,
    backup: BackupUiState,
    password: PasswordState,
    secret_clipboard: Option<SecretClipboard<SystemClipboard>>,
    lan_access: LanEnableControl,
    network_safety: NetworkSafetyAssessment,
    device_pairing: DevicePairing,
    pairing_invitation: Option<PairingInvitation>,
    paired_device: Option<PairedDeviceCandidate>,
    trusted_devices: Vec<TrustedDevice>,
    trusted_devices_error: Option<String>,
    permission_store: DevicePermissionStore,
    permission_store_error: Option<String>,
    permission_editor_target: Option<[u8; 32]>,
    permission_editor_scopes: Vec<PermissionScope>,
    pending_revocation: Option<[u8; 32]>,
    revocation_confirmation: String,
    peer_identity: Option<peer_identity::PeerIdentitySummary>,
    peer_identity_error: Option<String>,
    trusted_peers: Vec<trusted_peers::TrustedPeer>,
    trusted_peers_error: Option<String>,
    offline_doc_index: usize,
    notice: String,
    focus_index: usize,
    edit: Option<EditMode>,
    hits: Vec<HitTarget>,
    should_quit: bool,
    session_saved: SessionState,
    session_failed: Option<SessionState>,
}

impl App {
    fn new() -> Self {
        let workspace_root = workspace::default_workspace_root();
        let mut app = Self {
            view: View::Home,
            browser: FileBrowserState::new(&workspace_root),
            editor: EditorState::new(),
            hash: HashVerifyState::new(),
            text_json: TextJsonState::new(),
            compare: FileCompareState::new(),
            search: DocumentSearchState::new(),
            repositories: RepositoryUiState::new(&workspace_root),
            backup: BackupUiState::new(&workspace_root),
            password: PasswordState::new(),
            secret_clipboard: SystemClipboard::new().ok().map(SecretClipboard::new),
            lan_access: LanEnableControl::default(),
            network_safety: network_safety::assess_current_network(),
            device_pairing: DevicePairing::default(),
            pairing_invitation: None,
            paired_device: None,
            trusted_devices: Vec::new(),
            trusted_devices_error: None,
            permission_store: DevicePermissionStore::default(),
            permission_store_error: None,
            permission_editor_target: None,
            permission_editor_scopes: Vec::new(),
            pending_revocation: None,
            revocation_confirmation: String::new(),
            peer_identity: None,
            peer_identity_error: None,
            trusted_peers: Vec::new(),
            trusted_peers_error: None,
            offline_doc_index: 0,
            notice: "READY".to_string(),
            focus_index: 0,
            edit: None,
            hits: Vec::new(),
            should_quit: false,
            session_saved: SessionState::default(),
            session_failed: None,
            workspace_root,
        };
        app.restore_saved_session();
        app.refresh_trusted_devices();
        app.refresh_peer_identity();
        app.session_saved = app.session_snapshot();
        app
    }

    fn run(&mut self, terminal: &mut TuiTerminal) -> io::Result<()> {
        while !self.should_quit {
            terminal.draw(|frame| self.render(frame))?;

            if self.editor.maybe_autosave(&self.workspace_root)? {
                self.notice = "RECOVERY AUTOSAVED".to_string();
            }
            self.poll_secret_clipboard();
            self.persist_session_if_changed();

            if event::poll(EVENT_POLL)? {
                match event::read()? {
                    Event::Key(key) if key.kind == KeyEventKind::Press => self.handle_key(key),
                    Event::Mouse(mouse) => self.handle_mouse(mouse),
                    Event::Resize(_, _) => {}
                    _ => {}
                }
            }
        }
        Ok(())
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
            || self
                .session_failed
                .as_ref()
                .is_some_and(|failed| failed == &current)
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

    fn refresh_trusted_devices(&mut self) {
        if !workspace::summary(&self.workspace_root).initialized {
            self.trusted_devices.clear();
            self.trusted_devices_error = None;
            self.permission_store = DevicePermissionStore::default();
            self.permission_store_error = None;
            return;
        }
        match trusted_devices::load(&self.workspace_root) {
            Ok(store) => {
                self.trusted_devices = store.devices().to_vec();
                self.trusted_devices_error = None;
            }
            Err(error) => {
                self.trusted_devices.clear();
                self.trusted_devices_error = Some(error.to_string());
                self.notice = format!("TRUST STORE FAILED: {error}");
                return;
            }
        }
        match device_permissions::load(&self.workspace_root) {
            Ok(store) => {
                self.permission_store = store;
                self.permission_store_error = None;
            }
            Err(error) => {
                self.permission_store = DevicePermissionStore::default();
                self.permission_store_error = Some(error.to_string());
            }
        }
    }

    fn refresh_peer_identity(&mut self) {
        if !workspace::summary(&self.workspace_root).initialized {
            self.peer_identity = None;
            self.peer_identity_error = None;
            self.trusted_peers.clear();
            self.trusted_peers_error = None;
            return;
        }
        match peer_identity::load(&self.workspace_root) {
            Ok(Some(identity)) => {
                self.peer_identity = Some(identity.summary());
                self.peer_identity_error = None;
            }
            Ok(None) => {
                self.peer_identity = None;
                self.peer_identity_error = None;
            }
            Err(error) => {
                self.peer_identity = None;
                self.peer_identity_error = Some(error.to_string());
            }
        }
        match trusted_peers::load(&self.workspace_root) {
            Ok(store) => {
                self.trusted_peers = store.peers().to_vec();
                self.trusted_peers_error = None;
            }
            Err(error) => {
                self.trusted_peers.clear();
                self.trusted_peers_error = Some(error.to_string());
            }
        }
    }

    fn controls(&mut self) -> Vec<Control> {
        let mut controls: Vec<Control> = View::PRIMARY.into_iter().map(Control::nav).collect();
        let mut push = |id, label: String, enabled| {
            controls.push(Control::content(id, label, enabled));
        };

        match self.view {
            View::Home | View::Tools => {
                for (index, tool) in tool_registry::all().iter().enumerate() {
                    push(
                        ControlId::Tool(index),
                        format!("{}  [{}]", tool.label, tool.status.label()),
                        tool.status == ToolStatus::Ready,
                    );
                }
            }
            View::PasswordGenerator => {
                push(ControlId::PasswordLengthDown, format!("LENGTH - ({})", self.password.config.length), self.password.config.length > MIN_PASSWORD_LENGTH);
                push(ControlId::PasswordLengthUp, format!("LENGTH + ({})", self.password.config.length), self.password.config.length < MAX_PASSWORD_LENGTH);
                push(ControlId::PasswordUpper, format!("[{}] UPPERCASE", mark(self.password.config.uppercase)), true);
                push(ControlId::PasswordLower, format!("[{}] LOWERCASE", mark(self.password.config.lowercase)), true);
                push(ControlId::PasswordDigits, format!("[{}] DIGITS", mark(self.password.config.digits)), true);
                push(ControlId::PasswordSymbols, format!("[{}] SYMBOLS", mark(self.password.config.symbols)), true);
                push(ControlId::PasswordRequireEach, format!("[{}] REQUIRE EACH GROUP", mark(self.password.config.require_each_selected_group)), true);
                push(ControlId::PasswordAmbiguous, format!("[{}] EXCLUDE AMBIGUOUS", mark(self.password.config.exclude_ambiguous)), true);
                push(ControlId::PasswordSymbolSet, format!("SYMBOL SET: {}", self.password.config.symbol_set), self.password.config.symbols);
                push(ControlId::PasswordGenerate, "GENERATE".to_string(), true);
                push(
                    ControlId::PasswordCopy,
                    "COPY / AUTO-CLEAR 30s".to_string(),
                    self.password.generated.is_some() && self.secret_clipboard.is_some(),
                );
                push(ControlId::PasswordClear, "CLEAR OUTPUT".to_string(), self.password.generated.is_some());
            }
            View::Files => {
                push(ControlId::FileUp, "UP".to_string(), !self.browser.current_dir.as_os_str().is_empty());
                push(ControlId::FileRefresh, "REFRESH".to_string(), true);
                for (index, entry) in self.browser.entries.iter().enumerate() {
                    push(
                        ControlId::FileEntry(index),
                        format!("{}  {}", entry.kind.label(), entry.name),
                        !matches!(entry.kind, EntryKind::Symlink | EntryKind::Other),
                    );
                }
                let selected = self.browser.selected_entry();
                push(ControlId::FileOpen, "OPEN".to_string(), selected.is_some());
                let is_file = selected.is_some_and(|entry| entry.kind == EntryKind::File);
                push(ControlId::FileHash, "HASH / VERIFY".to_string(), is_file);
                push(ControlId::FileTextJson, "TEXT / JSON".to_string(), is_file);
                push(ControlId::FileCompareA, "COMPARE AS A".to_string(), is_file);
                push(ControlId::FileCompareB, "COMPARE AS B".to_string(), is_file);
            }
            View::Editor => {
                let open = self.editor.path.is_some();
                push(ControlId::EditorEdit, "EDIT BUFFER".to_string(), open && self.editor.pending_recovery.is_none());
                push(ControlId::EditorSave, "SAVE".to_string(), open && self.editor.dirty && self.editor.pending_recovery.is_none());
                push(ControlId::EditorDiscard, "DISCARD CHANGES".to_string(), open && self.editor.dirty);
                push(ControlId::EditorRestoreRecovery, "RESTORE RECOVERY".to_string(), self.editor.pending_recovery.is_some());
                push(ControlId::EditorDiscardRecovery, "DISCARD RECOVERY".to_string(), self.editor.pending_recovery.is_some());
            }
            View::HashVerify => {
                push(ControlId::HashTarget, format!("TARGET: {}", self.hash.target), true);
                push(ControlId::HashExpected, format!("EXPECTED: {}", self.hash.expected), true);
                push(ControlId::HashUseSelection, "USE FILES SELECTION".to_string(), self.selected_file_path().is_some());
                push(ControlId::HashCalculate, "CALCULATE SHA-256".to_string(), !self.hash.target.trim().is_empty());
                push(ControlId::HashVerify, "VERIFY".to_string(), !self.hash.target.trim().is_empty() && !self.hash.expected.trim().is_empty());
                push(ControlId::HashClear, "CLEAR".to_string(), true);
            }
            View::TextJson => {
                push(ControlId::TextInput, "EDIT INPUT".to_string(), true);
                push(ControlId::TextLoadSelection, "LOAD FILES SELECTION".to_string(), self.selected_file_path().is_some());
                push(ControlId::TextNormalize, "NORMALIZE LF".to_string(), true);
                push(ControlId::TextClean, "CLEAN LINES".to_string(), true);
                push(ControlId::JsonValidate, "VALIDATE JSON".to_string(), true);
                push(ControlId::JsonPretty, "PRETTY JSON".to_string(), true);
                push(ControlId::JsonCompact, "COMPACT JSON".to_string(), true);
                push(ControlId::TextUseOutput, "USE OUTPUT AS INPUT".to_string(), !self.text_json.output.is_empty());
                push(ControlId::TextClear, "CLEAR".to_string(), true);
            }
            View::Compare => {
                push(ControlId::CompareLeft, format!("FILE A: {}", self.compare.left), true);
                push(ControlId::CompareRight, format!("FILE B: {}", self.compare.right), true);
                push(ControlId::CompareUseA, "USE FILES SELECTION AS A".to_string(), self.selected_file_path().is_some());
                push(ControlId::CompareUseB, "USE FILES SELECTION AS B".to_string(), self.selected_file_path().is_some());
                push(ControlId::CompareSwap, "SWAP A / B".to_string(), true);
                push(ControlId::CompareRun, "COMPARE FILES".to_string(), !self.compare.left.trim().is_empty() && !self.compare.right.trim().is_empty());
                push(ControlId::CompareClear, "CLEAR".to_string(), true);
            }
            View::Search => {
                push(ControlId::SearchQuery, format!("QUERY: {}", self.search.query), true);
                push(ControlId::SearchRun, "SEARCH DOCUMENTS".to_string(), !self.search.query.is_empty());
                push(ControlId::SearchClear, "CLEAR".to_string(), true);
                if let Some(report) = self.search.report.as_ref() {
                    for (index, result) in report.matches.iter().enumerate() {
                        push(
                            ControlId::SearchResult(index),
                            format!("OPEN {}:{} {}", file_browser::display_path(&result.relative_path), result.line, result.preview),
                            true,
                        );
                    }
                }
            }
            View::Repositories => {
                push(ControlId::RepoRefresh, "REFRESH LIST".to_string(), true);
                for (index, repo) in self.repositories.repositories.iter().enumerate() {
                    push(ControlId::RepoEntry(index), format!("REPO {}", repo.name), true);
                }
                let selected = self.repositories.selected.is_some();
                push(ControlId::RepoStatus, "STATUS".to_string(), selected);
                push(ControlId::RepoLog, "RECENT COMMITS".to_string(), selected);
                push(ControlId::RepoWorktree, "WORKTREE DIFF STAT".to_string(), selected);
                push(ControlId::RepoStaged, "STAGED DIFF STAT".to_string(), selected);
            }
            View::Backup => {
                push(ControlId::BackupCreate, "CREATE BACKUP".to_string(), true);
                push(ControlId::BackupRefresh, "REFRESH LIST".to_string(), true);
                for (index, backup) in self.backup.backups.iter().enumerate() {
                    push(ControlId::BackupEntry(index), format!("{} ({})", backup.name, format_bytes(backup.bytes)), true);
                }
                push(ControlId::BackupVerify, "VERIFY BACKUP".to_string(), self.backup.selected.is_some());
                push(ControlId::BackupConfirm, format!("CONFIRM: {}", self.backup.restore_confirmation), self.backup.selected.is_some());
                let editor_safe = !self.editor.dirty && self.editor.pending_recovery.is_none();
                push(ControlId::BackupRestore, "RESTORE SELECTED BACKUP".to_string(), self.backup.selected.is_some() && self.backup.restore_confirmation == "RESTORE" && editor_safe);
            }
            View::Workspace => {
                push(ControlId::WorkspaceInitialize, "INITIALIZE WORKSPACE".to_string(), !workspace::summary(&self.workspace_root).initialized);
            }
            View::Mobile => {
                push(ControlId::MobileRefreshNetwork, "REFRESH NETWORK SAFETY".to_string(), true);
                push(ControlId::MobileToggleLan, if self.lan_access.is_enabled() { "DISABLE LAN CONTROL" } else { "ENABLE FOR THIS RUN" }.to_string(), true);
                push(ControlId::PeerInitialize, "INITIALIZE PEER IDENTITY".to_string(), self.peer_identity.is_none() && workspace::summary(&self.workspace_root).initialized);
                push(ControlId::PeerRefresh, "REFRESH PEER / TRUST".to_string(), true);
                for (index, device) in self.trusted_devices.iter().enumerate() {
                    push(ControlId::DeviceEdit(index), format!("EDIT PERMISSIONS: {}", device.device_name()), self.permission_store_error.is_none());
                    push(ControlId::DeviceRevoke(index), format!("REVOKE DEVICE: {}", device.device_name()), true);
                }
                if self.permission_editor_target.is_some() {
                    for scope in PermissionScope::ALL {
                        push(ControlId::PermissionToggle(scope), format!("[{}] {} [{}]", mark(self.permission_editor_scopes.contains(&scope)), scope.label(), scope.as_str()), true);
                    }
                    push(ControlId::PermissionSave, "SAVE PERMISSIONS".to_string(), true);
                    push(ControlId::PermissionCancel, "CANCEL PERMISSION EDIT".to_string(), true);
                }
                if self.pending_revocation.is_some() {
                    push(ControlId::RevocationConfirm, format!("TYPE REVOKE TO CONFIRM: {}", self.revocation_confirmation), true);
                    push(ControlId::RevocationCommit, "CONFIRM REVOCATION".to_string(), self.revocation_confirmation.trim() == "REVOKE");
                    push(ControlId::RevocationCancel, "CANCEL REVOCATION".to_string(), true);
                }
                match self.device_pairing.status(Instant::now()) {
                    PairingStatus::Idle => {
                        self.pairing_invitation = None;
                        push(ControlId::PairingStart, "START DEVICE PAIRING".to_string(), self.lan_access.is_enabled());
                    }
                    PairingStatus::Waiting { .. } => {
                        push(ControlId::PairingCancel, "CANCEL PAIRING".to_string(), true);
                    }
                    PairingStatus::AwaitingApproval { .. } => {
                        self.pairing_invitation = None;
                        push(ControlId::PairingApprove, "APPROVE DEVICE FOR THIS RUN".to_string(), true);
                        push(ControlId::PairingReject, "REJECT DEVICE".to_string(), true);
                    }
                }
                if self.paired_device.is_some() {
                    push(ControlId::PairingTrust, "TRUST DEVICE PERSISTENTLY".to_string(), workspace::summary(&self.workspace_root).initialized);
                    push(ControlId::PairingClear, "CLEAR RUN-LOCAL PAIRING".to_string(), true);
                }
            }
            View::System => {
                let docs = offline_docs::documents();
                push(ControlId::SystemPrevDoc, "PREVIOUS DOCUMENT".to_string(), !docs.is_empty());
                push(ControlId::SystemNextDoc, "NEXT DOCUMENT".to_string(), !docs.is_empty());
            }
        }
        controls
    }

    fn handle_key(&mut self, key: KeyEvent) {
        if self.edit.is_some() {
            self.handle_edit_key(key);
            return;
        }
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
            self.should_quit = true;
            return;
        }
        match key.code {
            KeyCode::Char('q') => self.should_quit = true,
            KeyCode::Up => self.move_focus(-1),
            KeyCode::Down => self.move_focus(1),
            KeyCode::Left => self.jump_zone(Zone::Navigation),
            KeyCode::Right | KeyCode::Tab => self.jump_zone(Zone::Content),
            KeyCode::Enter => self.activate_focused(),
            KeyCode::Esc => {
                if !matches!(self.view, View::Home) {
                    self.set_view(View::Home);
                }
            }
            KeyCode::F(1) => self.set_view(View::Home),
            KeyCode::F(2) => self.set_view(View::Tools),
            KeyCode::F(3) => self.set_view(View::Files),
            KeyCode::F(4) => self.set_view(View::Editor),
            KeyCode::F(5) => self.set_view(View::Workspace),
            KeyCode::F(6) => self.set_view(View::Mobile),
            KeyCode::F(7) => self.set_view(View::System),
            _ => {}
        }
    }

    fn handle_mouse(&mut self, mouse: MouseEvent) {
        match mouse.kind {
            MouseEventKind::Down(MouseButton::Left) => {
                if let Some(hit) = self
                    .hits
                    .iter()
                    .find(|hit| contains(hit.rect, mouse.column, mouse.row))
                    .copied()
                {
                    self.focus_index = hit.control_index;
                    self.activate_focused();
                }
            }
            MouseEventKind::ScrollUp => self.move_focus(-1),
            MouseEventKind::ScrollDown => self.move_focus(1),
            _ => {}
        }
    }

    fn move_focus(&mut self, delta: isize) {
        let controls = self.controls();
        if controls.is_empty() {
            self.focus_index = 0;
            return;
        }
        let len = controls.len() as isize;
        let mut index = self.focus_index.min(controls.len() - 1) as isize;
        for _ in 0..controls.len() {
            index = (index + delta).rem_euclid(len);
            if controls[index as usize].enabled {
                self.focus_index = index as usize;
                break;
            }
        }
    }

    fn jump_zone(&mut self, zone: Zone) {
        let controls = self.controls();
        if let Some((index, _)) = controls
            .iter()
            .enumerate()
            .find(|(_, control)| control.zone == zone && control.enabled)
        {
            self.focus_index = index;
        }
    }

    fn activate_focused(&mut self) {
        let controls = self.controls();
        let Some(control) = controls.get(self.focus_index.min(controls.len().saturating_sub(1))) else {
            return;
        };
        if control.enabled {
            self.activate(control.id);
        }
    }

    fn set_view(&mut self, view: View) {
        if self.view == View::PasswordGenerator && view != View::PasswordGenerator {
            self.password.clear();
        }
        self.view = view;
        self.edit = None;
        self.focus_index = View::PRIMARY.len();
    }

    fn activate(&mut self, id: ControlId) {
        match id {
            ControlId::Nav(view) => self.set_view(view),
            ControlId::Tool(index) => {
                if let Some(tool) = tool_registry::all().get(index) {
                    self.launch_tool(tool.id);
                }
            }
            ControlId::FileUp => {
                self.browser.up(&self.workspace_root);
                self.notice = "FILES UP".to_string();
            }
            ControlId::FileRefresh => {
                self.browser.refresh(&self.workspace_root);
                self.notice = "FILES REFRESHED".to_string();
            }
            ControlId::FileEntry(index) => {
                if let Some(entry) = self.browser.entries.get(index).cloned() {
                    self.browser.selected = Some(entry.relative_path.clone());
                    if entry.kind == EntryKind::Directory {
                        self.browser.navigate(&self.workspace_root, entry.relative_path);
                    } else if entry.kind == EntryKind::File {
                        self.open_in_editor(entry.relative_path);
                    }
                }
            }
            ControlId::FileOpen => self.open_selected(),
            ControlId::FileHash => {
                if let Some(path) = self.selected_file_path() {
                    self.hash.set_target_path(&path);
                    self.set_view(View::HashVerify);
                }
            }
            ControlId::FileTextJson => {
                if let Some(path) = self.selected_file_path() {
                    match text_editor::read_text(&self.workspace_root, &path) {
                        Ok(text) => {
                            self.text_json.input = text;
                            self.text_json.clear_result();
                            self.set_view(View::TextJson);
                            self.notice = "TEXT / JSON FILE LOADED".to_string();
                        }
                        Err(error) => self.notice = format!("TEXT / JSON LOAD FAILED: {error}"),
                    }
                }
            }
            ControlId::FileCompareA => {
                if let Some(path) = self.selected_file_path() {
                    self.compare.left = path.to_string_lossy().into_owned();
                    self.compare.clear_result();
                    self.set_view(View::Compare);
                }
            }
            ControlId::FileCompareB => {
                if let Some(path) = self.selected_file_path() {
                    self.compare.right = path.to_string_lossy().into_owned();
                    self.compare.clear_result();
                    self.set_view(View::Compare);
                }
            }
            ControlId::EditorEdit => self.begin_edit(EditTarget::Editor),
            ControlId::EditorSave => match self.editor.save(&self.workspace_root) {
                Ok(()) => self.notice = "FILE SAVED".to_string(),
                Err(error) => self.notice = format!("SAVE FAILED: {error}"),
            },
            ControlId::EditorDiscard => {
                if !self.editor.confirm_discard {
                    self.editor.confirm_discard = true;
                    self.notice = "PRESS DISCARD AGAIN TO CONFIRM".to_string();
                } else {
                    match self.editor.discard_changes(&self.workspace_root) {
                        Ok(()) => self.notice = "CHANGES DISCARDED".to_string(),
                        Err(error) => self.notice = format!("RELOAD FAILED: {error}"),
                    }
                }
            }
            ControlId::EditorRestoreRecovery => {
                self.editor.restore_pending_recovery();
                self.notice = "RECOVERY RESTORED".to_string();
            }
            ControlId::EditorDiscardRecovery => match self.editor.discard_pending_recovery(&self.workspace_root) {
                Ok(()) => self.notice = "RECOVERY DISCARDED".to_string(),
                Err(error) => self.notice = format!("RECOVERY ERROR: {error}"),
            },
            ControlId::HashTarget => self.begin_edit(EditTarget::HashTarget),
            ControlId::HashExpected => self.begin_edit(EditTarget::HashExpected),
            ControlId::HashUseSelection => {
                if let Some(path) = self.selected_file_path() {
                    self.hash.set_target_path(&path);
                }
            }
            ControlId::HashCalculate => {
                self.hash.error = None;
                match hash_verify::hash_workspace_file(&self.workspace_root, Path::new(self.hash.target.trim())) {
                    Ok(actual) => {
                        self.hash.actual = Some(actual);
                        self.hash.status = Some(HashResultState::Calculated);
                        self.notice = "SHA-256 CALCULATED".to_string();
                    }
                    Err(error) => {
                        self.hash.actual = None;
                        self.hash.status = None;
                        self.hash.error = Some(error.to_string());
                        self.notice = format!("HASH FAILED: {error}");
                    }
                }
            }
            ControlId::HashVerify => {
                self.hash.error = None;
                match hash_verify::verify_workspace_file(&self.workspace_root, Path::new(self.hash.target.trim()), &self.hash.expected) {
                    Ok(result) => {
                        self.hash.actual = Some(result.actual);
                        self.hash.status = Some(if result.matches { HashResultState::Match } else { HashResultState::Mismatch });
                        self.notice = if result.matches { "SHA-256 MATCH" } else { "SHA-256 MISMATCH" }.to_string();
                    }
                    Err(error) => {
                        self.hash.error = Some(error.to_string());
                        self.notice = format!("VERIFY FAILED: {error}");
                    }
                }
            }
            ControlId::HashClear => {
                self.hash = HashVerifyState::new();
                self.notice = "HASH / VERIFY CLEARED".to_string();
            }
            ControlId::TextInput => self.begin_edit(EditTarget::TextInput),
            ControlId::TextLoadSelection => {
                if let Some(path) = self.selected_file_path() {
                    match text_editor::read_text(&self.workspace_root, &path) {
                        Ok(text) => {
                            self.text_json.input = text;
                            self.text_json.clear_result();
                            self.notice = "TEXT / JSON FILE LOADED".to_string();
                        }
                        Err(error) => self.notice = format!("TEXT / JSON LOAD FAILED: {error}"),
                    }
                }
            }
            ControlId::TextNormalize => self.apply_text_transform(text_json::normalize_line_endings, "TEXT LF NORMALIZED"),
            ControlId::TextClean => self.apply_text_transform(text_json::clean_text_lines, "TEXT LINES CLEANED"),
            ControlId::JsonValidate => match text_json::validate_json(&self.text_json.input) {
                Ok(kind) => {
                    self.text_json.output.clear();
                    self.text_json.status = Some(format!("JSON VALID / {}", kind.label()));
                    self.text_json.error = None;
                    self.notice = format!("JSON VALID / {}", kind.label());
                }
                Err(error) => {
                    self.text_json.error = Some(error.to_string());
                    self.notice = format!("JSON INVALID: {error}");
                }
            },
            ControlId::JsonPretty => self.apply_text_transform(text_json::pretty_json, "JSON PRETTY READY"),
            ControlId::JsonCompact => self.apply_text_transform(text_json::compact_json, "JSON COMPACT READY"),
            ControlId::TextUseOutput => {
                self.text_json.input = std::mem::take(&mut self.text_json.output);
                self.text_json.status = None;
                self.text_json.error = None;
                self.notice = "OUTPUT MOVED TO INPUT".to_string();
            }
            ControlId::TextClear => {
                self.text_json = TextJsonState::new();
                self.notice = "TEXT / JSON CLEARED".to_string();
            }
            ControlId::PasswordLengthDown => self.password.config.length = self.password.config.length.saturating_sub(1).max(MIN_PASSWORD_LENGTH),
            ControlId::PasswordLengthUp => self.password.config.length = (self.password.config.length + 1).min(MAX_PASSWORD_LENGTH),
            ControlId::PasswordUpper => self.password.config.uppercase = !self.password.config.uppercase,
            ControlId::PasswordLower => self.password.config.lowercase = !self.password.config.lowercase,
            ControlId::PasswordDigits => self.password.config.digits = !self.password.config.digits,
            ControlId::PasswordSymbols => self.password.config.symbols = !self.password.config.symbols,
            ControlId::PasswordRequireEach => self.password.config.require_each_selected_group = !self.password.config.require_each_selected_group,
            ControlId::PasswordAmbiguous => self.password.config.exclude_ambiguous = !self.password.config.exclude_ambiguous,
            ControlId::PasswordSymbolSet => self.begin_edit(EditTarget::PasswordSymbolSet),
            ControlId::PasswordGenerate => match password_generator::generate(&self.password.config) {
                Ok(generated) => {
                    let len = generated.len();
                    self.password.generated = Some(generated);
                    self.password.error = None;
                    self.notice = format!("PASSWORD GENERATED / {len} CHARS");
                }
                Err(error) => {
                    self.password.generated = None;
                    self.password.error = Some(error.to_string());
                    self.notice = format!("PASSWORD GENERATION FAILED: {error}");
                }
            },
            ControlId::PasswordCopy => {
                if let (Some(generated), Some(clipboard)) = (
                    self.password.generated.as_ref(),
                    self.secret_clipboard.as_mut(),
                ) {
                    match clipboard.copy(
                        ClipboardSecret::from_generated_password(generated),
                        ClipboardPolicy::default(),
                    ) {
                        Ok(()) => {
                            self.notice =
                                "PASSWORD COPIED / CLIPBOARD CLEAR IN 30s".to_string();
                        }
                        Err(error) => self.notice = format!("CLIPBOARD FAILED: {error}"),
                    }
                }
            }
            ControlId::PasswordClear => {
                self.password.clear();
                self.notice = "PASSWORD OUTPUT CLEARED".to_string();
            }
            ControlId::CompareLeft => self.begin_edit(EditTarget::CompareLeft),
            ControlId::CompareRight => self.begin_edit(EditTarget::CompareRight),
            ControlId::CompareUseA => {
                if let Some(path) = self.selected_file_path() {
                    self.compare.left = path.to_string_lossy().into_owned();
                    self.compare.clear_result();
                }
            }
            ControlId::CompareUseB => {
                if let Some(path) = self.selected_file_path() {
                    self.compare.right = path.to_string_lossy().into_owned();
                    self.compare.clear_result();
                }
            }
            ControlId::CompareSwap => {
                std::mem::swap(&mut self.compare.left, &mut self.compare.right);
                self.compare.clear_result();
            }
            ControlId::CompareRun => match file_compare::compare_workspace_files(
                &self.workspace_root,
                Path::new(self.compare.left.trim()),
                Path::new(self.compare.right.trim()),
            ) {
                Ok(result) => {
                    let equal = result.equal;
                    self.compare.result = Some(result);
                    self.compare.error = None;
                    self.notice = if equal { "FILES EQUAL" } else { "FILES DIFFERENT" }.to_string();
                }
                Err(error) => {
                    self.compare.result = None;
                    self.compare.error = Some(error.to_string());
                    self.notice = format!("COMPARE FAILED: {error}");
                }
            },
            ControlId::CompareClear => {
                self.compare = FileCompareState::new();
                self.notice = "COMPARE CLEARED".to_string();
            }
            ControlId::SearchQuery => self.begin_edit(EditTarget::SearchQuery),
            ControlId::SearchRun => match document_search::search_documents(&self.workspace_root, &self.search.query) {
                Ok(report) => {
                    let count = report.matches.len();
                    self.search.report = Some(report);
                    self.search.error = None;
                    self.notice = format!("SEARCH COMPLETE / {count} RESULT(S)");
                }
                Err(error) => {
                    self.search.report = None;
                    self.search.error = Some(error.to_string());
                    self.notice = format!("SEARCH FAILED: {error}");
                }
            },
            ControlId::SearchClear => {
                self.search = DocumentSearchState::new();
                self.notice = "SEARCH CLEARED".to_string();
            }
            ControlId::SearchResult(index) => {
                if let Some(report) = self.search.report.as_ref() {
                    if let Some(result) = report.matches.get(index) {
                        self.open_in_editor(result.relative_path.clone());
                    }
                }
            }
            ControlId::RepoRefresh => match self.repositories.refresh_list(&self.workspace_root) {
                Ok(count) => self.notice = format!("REPOSITORY LIST REFRESHED / {count} FOUND"),
                Err(error) => self.notice = format!("REPOSITORY LIST FAILED: {error}"),
            },
            ControlId::RepoEntry(index) => {
                if let Some(repo) = self.repositories.repositories.get(index) {
                    self.repositories.selected = Some(repo.relative_path.clone());
                    self.repositories.status = None;
                    self.repositories.output = None;
                    self.notice = "REPOSITORY SELECTED".to_string();
                }
            }
            ControlId::RepoStatus => self.repo_status(),
            ControlId::RepoLog => self.repo_output("RECENT COMMITS", repository_git::recent_commits),
            ControlId::RepoWorktree => self.repo_output("WORKTREE DIFF STAT", repository_git::worktree_diff_stat),
            ControlId::RepoStaged => self.repo_output("STAGED DIFF STAT", repository_git::staged_diff_stat),
            ControlId::BackupCreate => self.create_backup(),
            ControlId::BackupRefresh => match self.backup.refresh_list(&self.workspace_root) {
                Ok(count) => self.notice = format!("BACKUP LIST REFRESHED / {count} FOUND"),
                Err(error) => self.notice = format!("BACKUP LIST FAILED: {error}"),
            },
            ControlId::BackupEntry(index) => {
                if let Some(backup) = self.backup.backups.get(index) {
                    self.backup.selected = Some(backup.relative_path.clone());
                    self.backup.summary = None;
                    self.backup.restore_confirmation.clear();
                    self.notice = "BACKUP SELECTED".to_string();
                }
            }
            ControlId::BackupVerify => self.verify_backup(),
            ControlId::BackupConfirm => self.begin_edit(EditTarget::BackupConfirm),
            ControlId::BackupRestore => self.restore_backup(),
            ControlId::WorkspaceInitialize => match workspace::initialize(&self.workspace_root) {
                Ok(()) => {
                    self.browser.refresh(&self.workspace_root);
                    self.refresh_trusted_devices();
                    self.refresh_peer_identity();
                    self.notice = "WORKSPACE INITIALIZED".to_string();
                }
                Err(error) => self.notice = format!("INIT FAILED: {error}"),
            },
            ControlId::MobileRefreshNetwork => {
                self.network_safety = network_safety::assess_current_network();
                let exposure = network_safety::evaluate(&self.lan_access, self.network_safety);
                self.notice = format!("NETWORK SAFETY: {} / {}", self.network_safety.environment().label(), exposure.label());
            }
            ControlId::MobileToggleLan => {
                if self.lan_access.is_enabled() {
                    self.lan_access.disable();
                    self.device_pairing.cancel();
                    self.pairing_invitation = None;
                    self.paired_device = None;
                    self.notice = "LAN CONTROL OFF / PAIRING CLEARED".to_string();
                } else {
                    self.lan_access.enable_for_this_run();
                    self.network_safety = network_safety::assess_current_network();
                    let exposure = network_safety::evaluate(&self.lan_access, self.network_safety);
                    self.notice = format!("LAN CONTROL ON / {}", exposure.label());
                }
            }
            ControlId::PeerInitialize => match peer_identity::initialize(&self.workspace_root) {
                Ok(identity) => {
                    self.peer_identity = Some(identity.summary());
                    self.refresh_peer_identity();
                    self.notice = "PEER IDENTITY INITIALIZED / NO TRANSPORT OPENED".to_string();
                }
                Err(peer_identity::PeerIdentityError::AlreadyInitialized) => {
                    self.refresh_peer_identity();
                    self.notice = "PEER IDENTITY ALREADY INITIALIZED".to_string();
                }
                Err(error) => self.notice = format!("PEER IDENTITY FAILED: {error}"),
            },
            ControlId::PeerRefresh => {
                self.refresh_peer_identity();
                self.refresh_trusted_devices();
                self.notice = "PEER / TRUST STATE REFRESHED".to_string();
            }
            ControlId::DeviceEdit(index) => {
                if let Some(device) = self.trusted_devices.get(index) {
                    self.permission_editor_target = Some(*device.fingerprint());
                    self.permission_editor_scopes = self
                        .permission_store
                        .policy_for(device.fingerprint())
                        .map(|policy| policy.scopes().to_vec())
                        .unwrap_or_default();
                    self.notice = format!("PERMISSION EDIT: {}", device.device_name());
                }
            }
            ControlId::DeviceRevoke(index) => {
                if let Some(device) = self.trusted_devices.get(index) {
                    self.pending_revocation = Some(*device.fingerprint());
                    self.revocation_confirmation.clear();
                    self.notice = format!("REVOCATION SELECTED: {}", device.device_name());
                }
            }
            ControlId::PermissionToggle(scope) => {
                if self.permission_editor_scopes.contains(&scope) {
                    self.permission_editor_scopes.retain(|existing| existing != &scope);
                } else {
                    self.permission_editor_scopes.push(scope);
                    self.permission_editor_scopes.sort_unstable();
                }
            }
            ControlId::PermissionSave => self.save_permissions(),
            ControlId::PermissionCancel => {
                self.permission_editor_target = None;
                self.permission_editor_scopes.clear();
                self.notice = "PERMISSION EDIT CANCELLED".to_string();
            }
            ControlId::RevocationConfirm => self.begin_edit(EditTarget::RevocationConfirm),
            ControlId::RevocationCommit => self.commit_revocation(),
            ControlId::RevocationCancel => {
                self.pending_revocation = None;
                self.revocation_confirmation.clear();
                self.notice = "DEVICE REVOCATION CANCELLED".to_string();
            }
            ControlId::PairingStart => {
                let now = Instant::now();
                match self.device_pairing.begin(&self.lan_access, now) {
                    Ok(invitation) => {
                        self.pairing_invitation = Some(invitation);
                        self.notice = "PAIRING STARTED / LISTENER STILL LOCKED".to_string();
                    }
                    Err(error) => self.notice = format!("PAIRING START FAILED: {error}"),
                }
            }
            ControlId::PairingCancel => {
                self.device_pairing.cancel();
                self.pairing_invitation = None;
                self.notice = "PAIRING CANCELLED".to_string();
            }
            ControlId::PairingApprove => {
                let now = Instant::now();
                match self.device_pairing.approve(&self.lan_access, now) {
                    Ok(candidate) => {
                        self.notice = format!("DEVICE PAIRED FOR THIS RUN: {}", candidate.device_name());
                        self.paired_device = Some(candidate);
                    }
                    Err(error) => self.notice = format!("PAIRING APPROVAL FAILED: {error}"),
                }
            }
            ControlId::PairingReject => {
                self.device_pairing.reject_candidate();
                self.pairing_invitation = None;
                self.notice = "PAIRING CANDIDATE REJECTED".to_string();
            }
            ControlId::PairingTrust => self.trust_paired_device(),
            ControlId::PairingClear => {
                self.paired_device = None;
                self.device_pairing.cancel();
                self.pairing_invitation = None;
                self.notice = "RUN-LOCAL PAIRING CLEARED".to_string();
            }
            ControlId::SystemPrevDoc => {
                let len = offline_docs::documents().len();
                if len > 0 {
                    self.offline_doc_index = (self.offline_doc_index + len - 1) % len;
                }
            }
            ControlId::SystemNextDoc => {
                let len = offline_docs::documents().len();
                if len > 0 {
                    self.offline_doc_index = (self.offline_doc_index + 1) % len;
                }
            }
        }
    }

    fn poll_secret_clipboard(&mut self) {
        let Some(clipboard) = self.secret_clipboard.as_mut() else {
            return;
        };
        if !clipboard.has_pending_clear() {
            return;
        }
        match clipboard.poll() {
            Ok(ClipboardPoll::Idle | ClipboardPoll::Pending) => {}
            Ok(ClipboardPoll::Cleared) => {
                self.notice = "CLIPBOARD CLEARED".to_string();
            }
            Ok(ClipboardPoll::PreservedExternalContent) => {
                self.notice = "CLIPBOARD CHANGED / EXTERNAL CONTENT PRESERVED".to_string();
            }
            Err(error) => {
                self.notice = format!("CLIPBOARD FAILED: {error}");
            }
        }
    }

    fn selected_file_path(&self) -> Option<PathBuf> {
        self.browser
            .selected_entry()
            .filter(|entry| entry.kind == EntryKind::File)
            .map(|entry| entry.relative_path.clone())
    }

    fn open_selected(&mut self) {
        let Some(entry) = self.browser.selected_entry().cloned() else {
            return;
        };
        match entry.kind {
            EntryKind::Directory => self.browser.navigate(&self.workspace_root, entry.relative_path),
            EntryKind::File => self.open_in_editor(entry.relative_path),
            _ => {}
        }
    }

    fn open_in_editor(&mut self, path: PathBuf) {
        match self.editor.open(&self.workspace_root, path) {
            Ok(()) => {
                self.set_view(View::Editor);
                self.notice = "EDITOR READY".to_string();
            }
            Err(error) => self.notice = format!("OPEN FAILED: {error}"),
        }
    }

    fn launch_tool(&mut self, tool: ToolId) {
        let view = match tool {
            ToolId::Files => View::Files,
            ToolId::Editor => View::Editor,
            ToolId::HashVerify => View::HashVerify,
            ToolId::TextJson => View::TextJson,
            ToolId::PasswordGenerator => View::PasswordGenerator,
            ToolId::Compare => View::Compare,
            ToolId::Search => View::Search,
            ToolId::Repositories => {
                let _ = self.repositories.refresh_list(&self.workspace_root);
                View::Repositories
            }
            ToolId::Backup => {
                let _ = self.backup.refresh_list(&self.workspace_root);
                View::Backup
            }
        };
        self.set_view(view);
        self.notice = format!("{} READY", view.label());
    }

    fn apply_text_transform(
        &mut self,
        operation: fn(&str) -> Result<String, text_json::TextJsonError>,
        success: &str,
    ) {
        match operation(&self.text_json.input) {
            Ok(output) => {
                self.text_json.output = output;
                self.text_json.status = Some(success.to_string());
                self.text_json.error = None;
                self.notice = success.to_string();
            }
            Err(error) => {
                self.text_json.output.clear();
                self.text_json.error = Some(error.to_string());
                self.notice = format!("TEXT / JSON FAILED: {error}");
            }
        }
    }

    fn repo_status(&mut self) {
        let Some(selected) = self.repositories.selected.as_deref() else {
            return;
        };
        match repository_git::repository_status(&self.workspace_root, selected) {
            Ok(status) => {
                self.repositories.status = Some(status);
                self.repositories.output = None;
                self.repositories.error = None;
                self.notice = "GIT STATUS COMPLETE".to_string();
            }
            Err(error) => {
                self.repositories.error = Some(error.to_string());
                self.notice = format!("GIT STATUS FAILED: {error}");
            }
        }
    }

    fn repo_output(
        &mut self,
        label: &'static str,
        operation: fn(&Path, &Path) -> Result<repository_git::GitTextOutput, repository_git::RepositoryGitError>,
    ) {
        let Some(selected) = self.repositories.selected.as_deref() else {
            return;
        };
        match operation(&self.workspace_root, selected) {
            Ok(output) => {
                self.repositories.output_label = Some(label);
                self.repositories.output = Some(output);
                self.repositories.status = None;
                self.repositories.error = None;
                self.notice = format!("{label} COMPLETE");
            }
            Err(error) => {
                self.repositories.error = Some(error.to_string());
                self.notice = format!("{label} FAILED: {error}");
            }
        }
    }

    fn persist_dirty_editor_recovery_for_backup(&mut self) -> io::Result<()> {
        if !self.editor.dirty {
            return Ok(());
        }
        let path = self
            .editor
            .path
            .as_ref()
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "no editor file is open"))?;
        text_editor::write_recovery(&self.workspace_root, path, &self.editor.buffer)?;
        self.editor.last_change = None;
        self.editor.recovery_status = "RECOVERY STORED FOR BACKUP".to_string();
        Ok(())
    }

    fn create_backup(&mut self) {
        if let Err(error) = self.persist_dirty_editor_recovery_for_backup() {
            self.notice = format!("BACKUP PREPARE FAILED: {error}");
            return;
        }
        match workspace_backup::create_backup(&self.workspace_root) {
            Ok(created) => {
                let _ = self.backup.refresh_list(&self.workspace_root);
                self.backup.selected = Some(created.relative_path);
                self.notice = format!("BACKUP CREATED / {}", created.name);
            }
            Err(error) => self.notice = format!("BACKUP CREATE FAILED: {error}"),
        }
    }

    fn verify_backup(&mut self) {
        let Some(selected) = self.backup.selected.as_deref() else {
            return;
        };
        match workspace_backup::verify_backup(&self.workspace_root, selected) {
            Ok(summary) => {
                self.backup.summary = Some(summary);
                self.backup.error = None;
                self.notice = "BACKUP VERIFIED".to_string();
            }
            Err(error) => {
                self.backup.summary = None;
                self.backup.error = Some(error.to_string());
                self.notice = format!("BACKUP VERIFY FAILED: {error}");
            }
        }
    }

    fn restore_backup(&mut self) {
        let Some(selected) = self.backup.selected.clone() else {
            return;
        };
        match workspace_backup::restore_backup(&self.workspace_root, &selected) {
            Ok(summary) => {
                self.browser = FileBrowserState::new(&self.workspace_root);
                self.editor = EditorState::new();
                self.hash = HashVerifyState::new();
                self.text_json = TextJsonState::new();
                self.compare = FileCompareState::new();
                self.search = DocumentSearchState::new();
                self.repositories = RepositoryUiState::new(&self.workspace_root);
                self.backup = BackupUiState::new(&self.workspace_root);
                self.refresh_trusted_devices();
                self.refresh_peer_identity();
                self.notice = format!(
                    "WORKSPACE RESTORED / {} FILES / {}",
                    summary.files,
                    file_browser::display_path(&selected)
                );
            }
            Err(error) => {
                self.backup.summary = None;
                self.backup.error = Some(error.to_string());
                self.backup.restore_confirmation.clear();
                self.notice = format!("WORKSPACE RESTORE FAILED: {error}");
            }
        }
    }

    fn save_permissions(&mut self) {
        let Some(fingerprint) = self.permission_editor_target else {
            return;
        };
        let target_name = self
            .trusted_devices
            .iter()
            .find(|device| device.fingerprint() == &fingerprint)
            .map(|device| device.device_name().to_string())
            .unwrap_or_else(|| "selected device".to_string());
        match device_permissions::set_scopes(
            &self.workspace_root,
            &fingerprint,
            &self.permission_editor_scopes,
        ) {
            Ok(policy) => {
                let count = policy.scopes().len();
                self.permission_editor_target = None;
                self.permission_editor_scopes.clear();
                self.refresh_trusted_devices();
                self.notice = format!("DEVICE PERMISSIONS SAVED: {target_name} / {count} SCOPE(S)");
            }
            Err(error) => self.notice = format!("PERMISSION SAVE FAILED: {error}"),
        }
    }

    fn commit_revocation(&mut self) {
        let Some(fingerprint) = self.pending_revocation else {
            return;
        };
        match device_revocation::revoke(&self.workspace_root, &fingerprint) {
            Ok(revoked) => {
                if self
                    .paired_device
                    .as_ref()
                    .is_some_and(|paired| paired.fingerprint() == revoked.fingerprint())
                {
                    self.paired_device = None;
                    self.device_pairing.cancel();
                    self.pairing_invitation = None;
                }
                self.pending_revocation = None;
                self.revocation_confirmation.clear();
                self.refresh_trusted_devices();
                self.notice = format!("DEVICE KEY REVOKED: {}", revoked.device_name());
            }
            Err(device_revocation::DeviceRevocationError::AlreadyRevoked) => {
                self.pending_revocation = None;
                self.revocation_confirmation.clear();
                self.refresh_trusted_devices();
                self.notice = "DEVICE KEY ALREADY REVOKED".to_string();
            }
            Err(error) => self.notice = format!("DEVICE REVOCATION FAILED: {error}"),
        }
    }

    fn trust_paired_device(&mut self) {
        let Some(paired) = self.paired_device.clone() else {
            return;
        };
        match trusted_devices::register_candidate(&self.workspace_root, &paired) {
            Ok(_) => {
                self.refresh_trusted_devices();
                self.notice = format!("DEVICE PUBLIC KEY TRUSTED: {} / DEFAULT DENY", paired.device_name());
            }
            Err(trusted_devices::TrustedDeviceError::AlreadyTrusted) => {
                self.refresh_trusted_devices();
                self.notice = "DEVICE KEY ALREADY TRUSTED".to_string();
            }
            Err(trusted_devices::TrustedDeviceError::RevokedDevice) => {
                self.paired_device = None;
                self.device_pairing.cancel();
                self.pairing_invitation = None;
                self.notice = "DEVICE KEY REVOKED / ROTATE KEY AND PAIR AGAIN".to_string();
            }
            Err(error) => self.notice = format!("TRUST STORE FAILED: {error}"),
        }
    }

    fn begin_edit(&mut self, target: EditTarget) {
        let cursor = self.edit_string(target).len();
        self.edit = Some(EditMode { target, cursor });
    }

    fn handle_edit_key(&mut self, key: KeyEvent) {
        let Some(mut edit) = self.edit.take() else {
            return;
        };
        if key.code == KeyCode::Esc {
            return;
        }
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('s') {
            if edit.target == EditTarget::Editor {
                match self.editor.save(&self.workspace_root) {
                    Ok(()) => self.notice = "FILE SAVED".to_string(),
                    Err(error) => self.notice = format!("SAVE FAILED: {error}"),
                }
            }
            self.edit = Some(edit);
            return;
        }

        let multiline = matches!(edit.target, EditTarget::Editor | EditTarget::TextInput);
        match key.code {
            KeyCode::Enter if !multiline => return,
            KeyCode::Enter => {
                self.edit_insert(edit.target, &mut edit.cursor, "\n");
            }
            KeyCode::Char(ch) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                let mut encoded = [0; 4];
                let text = ch.encode_utf8(&mut encoded);
                self.edit_insert(edit.target, &mut edit.cursor, text);
            }
            KeyCode::Backspace => self.edit_backspace(edit.target, &mut edit.cursor),
            KeyCode::Delete => self.edit_delete(edit.target, edit.cursor),
            KeyCode::Left => edit.cursor = prev_boundary(self.edit_string(edit.target), edit.cursor),
            KeyCode::Right => edit.cursor = next_boundary(self.edit_string(edit.target), edit.cursor),
            KeyCode::Home => edit.cursor = 0,
            KeyCode::End => edit.cursor = self.edit_string(edit.target).len(),
            _ => {}
        }
        if edit.target == EditTarget::Editor {
            self.editor.mark_changed();
        } else {
            self.on_field_changed(edit.target);
        }
        self.edit = Some(edit);
    }

    fn edit_string(&self, target: EditTarget) -> &str {
        match target {
            EditTarget::Editor => &self.editor.buffer,
            EditTarget::HashTarget => &self.hash.target,
            EditTarget::HashExpected => &self.hash.expected,
            EditTarget::TextInput => &self.text_json.input,
            EditTarget::PasswordSymbolSet => &self.password.config.symbol_set,
            EditTarget::CompareLeft => &self.compare.left,
            EditTarget::CompareRight => &self.compare.right,
            EditTarget::SearchQuery => &self.search.query,
            EditTarget::BackupConfirm => &self.backup.restore_confirmation,
            EditTarget::RevocationConfirm => &self.revocation_confirmation,
        }
    }

    fn edit_string_mut(&mut self, target: EditTarget) -> &mut String {
        match target {
            EditTarget::Editor => &mut self.editor.buffer,
            EditTarget::HashTarget => &mut self.hash.target,
            EditTarget::HashExpected => &mut self.hash.expected,
            EditTarget::TextInput => &mut self.text_json.input,
            EditTarget::PasswordSymbolSet => &mut self.password.config.symbol_set,
            EditTarget::CompareLeft => &mut self.compare.left,
            EditTarget::CompareRight => &mut self.compare.right,
            EditTarget::SearchQuery => &mut self.search.query,
            EditTarget::BackupConfirm => &mut self.backup.restore_confirmation,
            EditTarget::RevocationConfirm => &mut self.revocation_confirmation,
        }
    }

    fn edit_insert(&mut self, target: EditTarget, cursor: &mut usize, text: &str) {
        let value = self.edit_string_mut(target);
        let at = (*cursor).min(value.len());
        value.insert_str(at, text);
        *cursor = at + text.len();
    }

    fn edit_backspace(&mut self, target: EditTarget, cursor: &mut usize) {
        let start = prev_boundary(self.edit_string(target), *cursor);
        if start < *cursor {
            self.edit_string_mut(target).replace_range(start..*cursor, "");
            *cursor = start;
        }
    }

    fn edit_delete(&mut self, target: EditTarget, cursor: usize) {
        let end = next_boundary(self.edit_string(target), cursor);
        if end > cursor {
            self.edit_string_mut(target).replace_range(cursor..end, "");
        }
    }

    fn on_field_changed(&mut self, target: EditTarget) {
        match target {
            EditTarget::HashTarget | EditTarget::HashExpected => self.hash.clear_result(),
            EditTarget::TextInput => self.text_json.clear_result(),
            EditTarget::CompareLeft | EditTarget::CompareRight => self.compare.clear_result(),
            EditTarget::SearchQuery => {
                self.search.report = None;
                self.search.error = None;
            }
            _ => {}
        }
    }

    fn render(&mut self, frame: &mut Frame) {
        self.hits.clear();
        let area = frame.area();
        let vertical = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(3),
                Constraint::Min(8),
                Constraint::Length(2),
            ])
            .split(area);

        self.render_header(frame, vertical[0]);

        let main = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Length(22), Constraint::Min(30)])
            .split(vertical[1]);

        let controls = self.controls();
        if self.focus_index >= controls.len() {
            self.focus_index = controls.len().saturating_sub(1);
        }
        self.render_navigation(frame, main[0], &controls);
        self.render_content(frame, main[1], &controls);
        self.render_footer(frame, vertical[2]);
    }

    fn render_header(&self, frame: &mut Frame, area: Rect) {
        let title = Line::from(vec![
            Span::styled(" DANGOMUSHI ", Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
            Span::styled(
                format!("{} / TUI ", env!("CARGO_PKG_VERSION")),
                Style::default().fg(Color::DarkGray),
            ),
            Span::styled(
                format!("  {}", self.notice),
                notice_style(&self.notice),
            ),
        ]);
        frame.render_widget(
            Paragraph::new(title).block(Block::default().borders(Borders::ALL)),
            area,
        );
    }

    fn render_navigation(&mut self, frame: &mut Frame, area: Rect, controls: &[Control]) {
        let nav: Vec<(usize, &Control)> = controls
            .iter()
            .enumerate()
            .filter(|(_, control)| control.zone == Zone::Navigation)
            .collect();
        let items: Vec<ListItem> = nav
            .iter()
            .map(|(index, control)| {
                let prefix = if *index == self.focus_index { "> " } else { "  " };
                ListItem::new(format!("{prefix}{}", control.label)).style(if *index == self.focus_index {
                    Style::default().fg(Color::Black).bg(Color::White)
                } else if control.id == ControlId::Nav(self.view) {
                    Style::default().fg(Color::Green)
                } else {
                    Style::default()
                })
            })
            .collect();
        frame.render_widget(
            List::new(items).block(Block::default().title(" NAVIGATION ").borders(Borders::ALL)),
            area,
        );
        for (row, (index, _)) in nav.iter().enumerate() {
            let y = area.y.saturating_add(1 + row as u16);
            if y < area.bottom().saturating_sub(1) {
                self.hits.push(HitTarget {
                    rect: Rect::new(area.x + 1, y, area.width.saturating_sub(2), 1),
                    control_index: *index,
                });
            }
        }
    }

    fn render_content(&mut self, frame: &mut Frame, area: Rect, controls: &[Control]) {
        let sections = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(2), Constraint::Percentage(55), Constraint::Percentage(45)])
            .split(area);
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled(
                format!(" {} ", self.view.label()),
                Style::default().add_modifier(Modifier::BOLD),
            ))),
            sections[0],
        );

        let content_controls: Vec<(usize, &Control)> = controls
            .iter()
            .enumerate()
            .filter(|(_, control)| control.zone == Zone::Content)
            .collect();
        let max_rows = sections[1].height.saturating_sub(2) as usize;
        let focus_pos = content_controls
            .iter()
            .position(|(index, _)| *index == self.focus_index)
            .unwrap_or(0);
        let start = focus_pos.saturating_sub(max_rows.saturating_sub(1));
        let visible = content_controls.iter().skip(start).take(max_rows);

        let mut items = Vec::new();
        let mut visible_indexes = Vec::new();
        for (index, control) in visible {
            let focused = *index == self.focus_index;
            let prefix = if focused { "> " } else { "  " };
            let style = if !control.enabled {
                Style::default().fg(Color::DarkGray)
            } else if focused {
                Style::default().fg(Color::Black).bg(Color::White)
            } else {
                Style::default()
            };
            items.push(ListItem::new(format!("{prefix}{}", control.label)).style(style));
            visible_indexes.push(*index);
        }
        frame.render_widget(
            List::new(items).block(Block::default().title(" CONTROLS ").borders(Borders::ALL)),
            sections[1],
        );
        for (row, index) in visible_indexes.into_iter().enumerate() {
            let y = sections[1].y.saturating_add(1 + row as u16);
            if y < sections[1].bottom().saturating_sub(1) {
                self.hits.push(HitTarget {
                    rect: Rect::new(
                        sections[1].x + 1,
                        y,
                        sections[1].width.saturating_sub(2),
                        1,
                    ),
                    control_index: index,
                });
            }
        }

        let detail = self.detail_text();
        frame.render_widget(
            Paragraph::new(detail)
                .block(Block::default().title(" STATE / RESULT ").borders(Borders::ALL))
                .wrap(Wrap { trim: false }),
            sections[2],
        );
    }

    fn render_footer(&self, frame: &mut Frame, area: Rect) {
        let editing = self.edit.as_ref().map(|edit| format!("EDIT {:?} / Esc done", edit.target));
        let text = editing.unwrap_or_else(|| {
            "↑↓ focus  ← nav  →/Tab content  Enter activate/click  Esc home  F1-F7 views  q quit".to_string()
        });
        frame.render_widget(
            Paragraph::new(text).style(Style::default().fg(Color::DarkGray)),
            area,
        );
    }

    fn detail_text(&self) -> String {
        match self.view {
            View::Home => {
                let summary = workspace::summary(&self.workspace_root);
                format!(
                    "Runtime: TUI\nWorkspace: {} ({}/{})\nNetwork: {}\n\nAll interactive controls use one Activate path for keyboard Enter and mouse click.",
                    if summary.initialized { "READY" } else { "NOT INITIALIZED" },
                    summary.present_dirs,
                    summary.total_dirs,
                    if self.lan_access.is_enabled() { "LAN CONTROL ON" } else { "LAN CONTROL OFF" },
                )
            }
            View::Tools => "Built-in tools are dispatched explicitly to the existing Rust core.".to_string(),
            View::PasswordGenerator => {
                let generated = self.password.generated.as_ref().map(GeneratedPassword::expose_secret).unwrap_or("(no password generated)");
                format!(
                    "Generated password is process-local and is not written to Workspace/session state.\n\n{}\n{}",
                    generated,
                    self.password.error.as_deref().unwrap_or("")
                )
            }
            View::Files => {
                let selected = self.browser.selected_entry().map(|entry| {
                    format!("{} / {} / {} bytes", file_browser::display_path(&entry.relative_path), entry.kind.label(), entry.size_bytes)
                }).unwrap_or_else(|| "(none)".to_string());
                format!(
                    "Directory: {}\nSelected: {}\n{}",
                    file_browser::display_path(&self.browser.current_dir),
                    selected,
                    self.browser.error.as_deref().unwrap_or("")
                )
            }
            View::Editor => format!(
                "File: {}\nState: {} / {}\n\n{}",
                self.editor.path.as_deref().map(file_browser::display_path).unwrap_or_else(|| "(none)".to_string()),
                if self.editor.dirty { "DIRTY" } else { "CLEAN" },
                self.editor.recovery_status,
                self.editor.buffer
            ),
            View::HashVerify => {
                let result = self.hash.actual.map(|actual| format!("SHA-256: {}\nBytes: {}", actual.sha256, actual.bytes_hashed)).unwrap_or_else(|| "No result".to_string());
                format!("{}\n{}", result, self.hash.error.as_deref().unwrap_or(""))
            }
            View::TextJson => format!(
                "Status: {}\nError: {}\n\nINPUT:\n{}\n\nOUTPUT:\n{}",
                self.text_json.status.as_deref().unwrap_or("-"),
                self.text_json.error.as_deref().unwrap_or("-"),
                self.text_json.input,
                self.text_json.output
            ),
            View::Compare => {
                if let Some(result) = self.compare.result {
                    format!(
                        "{}\nA bytes: {} / B bytes: {}\nFirst difference: {:?}\nText: {:?}",
                        if result.equal { "EQUAL" } else { "DIFFERENT" },
                        result.left_bytes,
                        result.right_bytes,
                        result.first_difference_byte,
                        result.text
                    )
                } else {
                    self.compare.error.clone().unwrap_or_else(|| "No result".to_string())
                }
            }
            View::Search => {
                if let Some(report) = self.search.report.as_ref() {
                    format!(
                        "Matches: {}\nFiles considered: {} / searched: {}\nSkipped large: {} / non-UTF8: {} / symlinks: {}",
                        report.matches.len(),
                        report.files_considered,
                        report.files_searched,
                        report.skipped_large,
                        report.skipped_non_utf8,
                        report.skipped_symlinks
                    )
                } else {
                    self.search.error.clone().unwrap_or_else(|| "No search yet".to_string())
                }
            }
            View::Repositories => {
                if let Some(status) = self.repositories.status.as_ref() {
                    let mut text = format!(
                        "Branch: {}\nStaged {} / unstaged {} / untracked {} / conflicts {}\n",
                        status.branch, status.staged, status.unstaged, status.untracked, status.conflicted
                    );
                    for entry in status.entries.iter().take(20) {
                        text.push_str(&format!("{} {}\n", entry.code, entry.path));
                    }
                    text
                } else if let Some(output) = self.repositories.output.as_ref() {
                    format!("{}\n{}", self.repositories.output_label.unwrap_or("OUTPUT"), output.text)
                } else {
                    self.repositories.error.clone().unwrap_or_else(|| "Select a repository".to_string())
                }
            }
            View::Backup => {
                let selected = self.backup.selected.as_deref().map(file_browser::display_path).unwrap_or_else(|| "(none)".to_string());
                let summary = self.backup.summary.map(|summary| format!(
                    "Verified: {} dirs / {} files / {} / archive {}",
                    summary.directories,
                    summary.files,
                    format_bytes(summary.total_file_bytes),
                    format_bytes(summary.archive_bytes)
                )).unwrap_or_default();
                format!("Selected: {}\n{}\n{}", selected, summary, self.backup.error.as_deref().unwrap_or(""))
            }
            View::Workspace => {
                let summary = workspace::summary(&self.workspace_root);
                format!(
                    "Path: {}\nState: {}\nDirectories: {}/{}",
                    self.workspace_root.display(),
                    if summary.initialized { "READY" } else { "NOT INITIALIZED" },
                    summary.present_dirs,
                    summary.total_dirs
                )
            }
            View::Mobile => self.mobile_detail(),
            View::System => self.system_detail(),
        }
    }

    fn mobile_detail(&self) -> String {
        let exposure = network_safety::evaluate(&self.lan_access, self.network_safety);
        let mut text = format!(
            "LAN: {}\nNetwork: {} / {} / {}\nExposure: {}\n\nLAN intent remains process-local and resets OFF on every launch.\nEnabling this control does not create a LAN listener. MOBILE LISTENER STILL LOCKED.\nDIRECT PEER LAN READY / EXPLICIT ONE-SHOT\nENCRYPTED TRANSFER CORE READY\nRECEIVED FILE INTEGRITY READY\nInternet P2P threat review is complete: {}\nReconnection uses a fresh Ed25519 challenge; No persistent bearer token is created.\n\nTrusted devices: {}\nTrusted peers: {}\n",
            if self.lan_access.is_enabled() { "ON (run-local)" } else { "OFF" },
            self.network_safety.environment().label(),
            self.network_safety.source().label(),
            self.network_safety.connected_networks(),
            exposure.label(),
            internet_p2p_policy::current_decision().label(),
            self.trusted_devices.len(),
            self.trusted_peers.len()
        );
        if let Some(identity) = self.peer_identity {
            text.push_str(&format!("Peer identity: {}\n", identity.fingerprint_hex()));
        }
        if let Some(error) = self.trusted_devices_error.as_ref() {
            text.push_str(&format!("Trust store: {error}\n"));
        }
        if let Some(error) = self.permission_store_error.as_ref() {
            text.push_str(&format!("Permissions: {error}\n"));
        }
        if self.paired_device.is_some() {
            if let Some(paired) = self.paired_device.as_ref() {
                text.push_str(&format!("\nPairing: PAIRED FOR THIS RUN\nDevice: {}\nFingerprint: {}", paired.device_name(), paired.fingerprint_hex()));
            }
        } else if let Some(invitation) = self.pairing_invitation.as_ref() {
            text.push_str(&format!("\nPAIRING CORE READY / TRANSPORT LOCKED\nPairing invitation active\nPAIRING CODE: {}", invitation.code().expose_secret()));
        } else {
            text.push_str("\nPAIRING CORE READY / TRANSPORT LOCKED\nPairing: IDLE OR AWAITING TRANSPORT PROOF");
        }
        text
    }

    fn system_detail(&self) -> String {
        match offline_docs::summary() {
            Ok(summary) => {
                let documents = offline_docs::documents();
                let document = documents.get(self.offline_doc_index);
                format!(
                    "UI: Ratatui / Crossterm\nFile browser: workspace-rooted / read-only\nEditor: UTF-8 / explicit save / recovery\nGateway: 127.0.0.1 only\n\nOFFLINE DOCUMENTATION\nEMBEDDED / VERIFIED INVENTORY\nRead-only content is compiled into this executable.\nOffline docs: {} docs / {} bytes / {}\n\n{}\n\n{}",
                    summary.document_count,
                    summary.total_bytes,
                    summary.sha256_hex(),
                    document.map(|doc| doc.path).unwrap_or("(none)"),
                    document.map(|doc| doc.content).unwrap_or("")
                )
            }
            Err(error) => format!("OFFLINE DOCS INVALID: {error}"),
        }
    }
}

fn mark(value: bool) -> char {
    if value { 'x' } else { ' ' }
}

fn contains(rect: Rect, x: u16, y: u16) -> bool {
    x >= rect.x && x < rect.right() && y >= rect.y && y < rect.bottom()
}

fn prev_boundary(value: &str, cursor: usize) -> usize {
    let mut result = 0;
    for (index, _) in value.char_indices() {
        if index >= cursor {
            break;
        }
        result = index;
    }
    result
}

fn next_boundary(value: &str, cursor: usize) -> usize {
    value[cursor.min(value.len())..]
        .char_indices()
        .nth(1)
        .map(|(offset, _)| cursor + offset)
        .unwrap_or(value.len())
}

fn notice_style(notice: &str) -> Style {
    if notice.contains("FAILED") || notice.contains("MISMATCH") || notice.contains("ERROR") {
        Style::default().fg(Color::Red)
    } else if notice.contains("WARN") || notice.contains("PUBLIC") {
        Style::default().fg(Color::Yellow)
    } else {
        Style::default().fg(Color::Green)
    }
}

fn format_bytes(bytes: u64) -> String {
    const KIB: u64 = 1024;
    const MIB: u64 = 1024 * KIB;
    const GIB: u64 = 1024 * MIB;
    if bytes >= GIB {
        format!("{:.2} GiB", bytes as f64 / GIB as f64)
    } else if bytes >= MIB {
        format!("{:.2} MiB", bytes as f64 / MIB as f64)
    } else if bytes >= KIB {
        format!("{:.2} KiB", bytes as f64 / KIB as f64)
    } else {
        format!("{bytes} B")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn primary_navigation_has_seven_stable_entries() {
        assert_eq!(View::PRIMARY.len(), 7);
        assert_eq!(View::PRIMARY[0], View::Home);
        assert_eq!(View::PRIMARY[6], View::System);
    }

    #[test]
    fn text_cursor_helpers_follow_utf8_boundaries() {
        let text = "aあb";
        assert_eq!(next_boundary(text, 0), 1);
        assert_eq!(next_boundary(text, 1), 4);
        assert_eq!(prev_boundary(text, 4), 1);
        assert_eq!(prev_boundary(text, 1), 0);
    }

    #[test]
    fn mouse_hit_test_is_half_open() {
        let rect = Rect::new(10, 5, 4, 2);
        assert!(contains(rect, 10, 5));
        assert!(contains(rect, 13, 6));
        assert!(!contains(rect, 14, 6));
        assert!(!contains(rect, 13, 7));
    }
}

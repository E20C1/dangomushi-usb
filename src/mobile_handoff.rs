use crate::device_permissions::PermissionScope;
use crate::device_reconnection::AuthenticatedDeviceSession;
use crate::lan_access_control::LanEnableControl;
use crate::session_state::{SessionState, SessionView};
use crate::text_editor;
use sha2::{Digest, Sha256};
use std::error::Error;
use std::fmt;
use std::path::{Component, Path, PathBuf};

pub const MOBILE_EDITOR_REVISION_BYTES: usize = 32;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MobileSessionSnapshot {
    view: SessionView,
    browser_dir: PathBuf,
    browser_selected: Option<PathBuf>,
    editor_path: Option<PathBuf>,
}

impl MobileSessionSnapshot {
    pub const fn view(&self) -> SessionView {
        self.view
    }

    pub fn browser_dir(&self) -> &Path {
        &self.browser_dir
    }

    pub fn browser_selected(&self) -> Option<&Path> {
        self.browser_selected.as_deref()
    }

    pub fn editor_path(&self) -> Option<&Path> {
        self.editor_path.as_deref()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MobileEditorDocument {
    path: PathBuf,
    text: String,
    revision: [u8; MOBILE_EDITOR_REVISION_BYTES],
}

impl MobileEditorDocument {
    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    pub const fn revision(&self) -> &[u8; MOBILE_EDITOR_REVISION_BYTES] {
        &self.revision
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MobileHandoffError {
    AuthorizationUnavailable,
    PermissionDenied(PermissionScope),
    InvalidSessionState,
    RevisionConflict,
    EditorUnavailable,
}

impl fmt::Display for MobileHandoffError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::AuthorizationUnavailable => {
                formatter.write_str("mobile authorization state could not be verified")
            }
            Self::PermissionDenied(scope) => {
                write!(formatter, "mobile operation requires {}", scope.as_str())
            }
            Self::InvalidSessionState => {
                formatter.write_str("session handoff contains an invalid relative path")
            }
            Self::RevisionConflict => formatter.write_str(
                "editor file changed after handoff; mobile text was stored as recovery instead of overwriting newer disk content",
            ),
            Self::EditorUnavailable => {
                formatter.write_str("mobile editor operation could not be completed")
            }
        }
    }
}

impl Error for MobileHandoffError {}

pub fn capture_session(
    workspace_root: &Path,
    lan_control: &LanEnableControl,
    authenticated: &AuthenticatedDeviceSession,
    current: &SessionState,
) -> Result<MobileSessionSnapshot, MobileHandoffError> {
    require_scope(
        workspace_root,
        lan_control,
        authenticated,
        PermissionScope::SessionHandoff,
    )?;
    validate_session_state(current)?;

    Ok(MobileSessionSnapshot {
        view: current.view,
        browser_dir: current.browser_dir.clone(),
        browser_selected: current.browser_selected.clone(),
        editor_path: current.editor_path.clone(),
    })
}

pub fn open_editor_document(
    workspace_root: &Path,
    lan_control: &LanEnableControl,
    authenticated: &AuthenticatedDeviceSession,
    path: &Path,
) -> Result<MobileEditorDocument, MobileHandoffError> {
    require_scope(
        workspace_root,
        lan_control,
        authenticated,
        PermissionScope::SessionHandoff,
    )?;
    require_scope(
        workspace_root,
        lan_control,
        authenticated,
        PermissionScope::FilesRead,
    )?;

    let text =
        text_editor::read_text(workspace_root, path).map_err(|_| MobileHandoffError::EditorUnavailable)?;
    let revision = content_revision(text.as_bytes());

    Ok(MobileEditorDocument {
        path: path.to_path_buf(),
        text,
        revision,
    })
}

pub fn save_editor_document(
    workspace_root: &Path,
    lan_control: &LanEnableControl,
    authenticated: &AuthenticatedDeviceSession,
    path: &Path,
    expected_revision: &[u8; MOBILE_EDITOR_REVISION_BYTES],
    text: &str,
) -> Result<[u8; MOBILE_EDITOR_REVISION_BYTES], MobileHandoffError> {
    require_scope(
        workspace_root,
        lan_control,
        authenticated,
        PermissionScope::SessionHandoff,
    )?;
    require_scope(
        workspace_root,
        lan_control,
        authenticated,
        PermissionScope::FilesWrite,
    )?;

    let disk_text =
        text_editor::read_text(workspace_root, path).map_err(|_| MobileHandoffError::EditorUnavailable)?;
    if &content_revision(disk_text.as_bytes()) != expected_revision {
        text_editor::write_recovery(workspace_root, path, text)
            .map_err(|_| MobileHandoffError::EditorUnavailable)?;
        return Err(MobileHandoffError::RevisionConflict);
    }

    text_editor::save_text(workspace_root, path, text)
        .map_err(|_| MobileHandoffError::EditorUnavailable)?;
    Ok(content_revision(text.as_bytes()))
}

fn require_scope(
    workspace_root: &Path,
    lan_control: &LanEnableControl,
    authenticated: &AuthenticatedDeviceSession,
    scope: PermissionScope,
) -> Result<(), MobileHandoffError> {
    match authenticated.authorize(workspace_root, lan_control, scope) {
        Ok(true) => Ok(()),
        Ok(false) => Err(MobileHandoffError::PermissionDenied(scope)),
        Err(_) => Err(MobileHandoffError::AuthorizationUnavailable),
    }
}

fn validate_session_state(state: &SessionState) -> Result<(), MobileHandoffError> {
    validate_relative(&state.browser_dir, true)?;
    validate_optional_relative(state.browser_selected.as_deref())?;
    validate_optional_relative(state.editor_path.as_deref())?;
    Ok(())
}

fn validate_optional_relative(path: Option<&Path>) -> Result<(), MobileHandoffError> {
    if let Some(path) = path {
        validate_relative(path, false)?;
    }
    Ok(())
}

fn validate_relative(path: &Path, allow_empty: bool) -> Result<(), MobileHandoffError> {
    if (!allow_empty && path.as_os_str().is_empty()) || path.is_absolute() {
        return Err(MobileHandoffError::InvalidSessionState);
    }

    if path
        .components()
        .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err(MobileHandoffError::InvalidSessionState);
    }

    Ok(())
}

fn content_revision(bytes: &[u8]) -> [u8; MOBILE_EDITOR_REVISION_BYTES] {
    let digest = Sha256::digest(bytes);
    let mut revision = [0u8; MOBILE_EDITOR_REVISION_BYTES];
    revision.copy_from_slice(&digest);
    revision
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn revision_is_content_based_and_deterministic() {
        assert_eq!(content_revision(b"same"), content_revision(b"same"));
        assert_ne!(content_revision(b"same"), content_revision(b"different"));
    }

    #[test]
    fn session_validation_rejects_parent_traversal() {
        let state = SessionState {
            browser_dir: PathBuf::from("../outside"),
            ..SessionState::default()
        };
        assert_eq!(
            validate_session_state(&state),
            Err(MobileHandoffError::InvalidSessionState)
        );
    }
}

use crate::credential::Credential;
use crate::password_generator::GeneratedPassword;
use crate::recovery_codes::RecoveryCode;
use std::error::Error;
use std::fmt;
use std::time::{Duration, Instant};
use zeroize::Zeroize;

pub const DEFAULT_CLEAR_DELAY: Duration = Duration::from_secs(30);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClipboardPolicy {
    clear_after: Duration,
}

impl ClipboardPolicy {
    pub const fn new(clear_after: Duration) -> Self {
        Self { clear_after }
    }

    pub const fn clear_after(self) -> Duration {
        self.clear_after
    }
}

impl Default for ClipboardPolicy {
    fn default() -> Self {
        Self::new(DEFAULT_CLEAR_DELAY)
    }
}

#[must_use]
pub struct ClipboardSecret {
    value: String,
}

impl ClipboardSecret {
    pub fn from_generated_password(password: &GeneratedPassword) -> Self {
        Self::from_secret_text(password.expose_secret())
    }

    pub fn from_credential_password(credential: &Credential) -> Self {
        Self::from_secret_text(credential.password().expose_secret())
    }

    pub fn from_recovery_code(code: &RecoveryCode) -> Self {
        Self::from_secret_text(code.expose_secret())
    }

    fn from_secret_text(value: &str) -> Self {
        Self { value: value.to_owned() }
    }

    fn expose_secret(&self) -> &str {
        &self.value
    }
}

impl Drop for ClipboardSecret {
    fn drop(&mut self) {
        self.value.zeroize();
    }
}

impl fmt::Debug for ClipboardSecret {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("ClipboardSecret([REDACTED])")
    }
}

#[must_use]
pub struct ClipboardObservedText {
    value: String,
}

impl ClipboardObservedText {
    pub fn from_owned(value: String) -> Self {
        Self { value }
    }

    fn expose_secret(&self) -> &str {
        &self.value
    }
}

impl Drop for ClipboardObservedText {
    fn drop(&mut self) {
        self.value.zeroize();
    }
}

impl fmt::Debug for ClipboardObservedText {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("ClipboardObservedText([REDACTED])")
    }
}

#[must_use]
pub enum ClipboardRead {
    Text(ClipboardObservedText),
    Unavailable,
}

impl fmt::Debug for ClipboardRead {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Text(_) => formatter.write_str("ClipboardRead::Text([REDACTED])"),
            Self::Unavailable => formatter.write_str("ClipboardRead::Unavailable"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClipboardBackendError;

impl fmt::Display for ClipboardBackendError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("clipboard backend operation failed")
    }
}

impl Error for ClipboardBackendError {}

pub trait ClipboardBackend {
    fn set_text(&mut self, text: &str) -> Result<(), ClipboardBackendError>;
    fn read_text(&mut self) -> Result<ClipboardRead, ClipboardBackendError>;
    fn clear(&mut self) -> Result<(), ClipboardBackendError>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClipboardPoll {
    Idle,
    Pending,
    Cleared,
    PreservedExternalContent,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClipboardHygieneError {
    ClipboardUnavailable,
}

impl fmt::Display for ClipboardHygieneError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("clipboard operation failed")
    }
}

impl Error for ClipboardHygieneError {}

impl From<ClipboardBackendError> for ClipboardHygieneError {
    fn from(_: ClipboardBackendError) -> Self {
        Self::ClipboardUnavailable
    }
}

struct ClipboardLease {
    expected: ClipboardSecret,
    clear_at: Instant,
}

impl fmt::Debug for ClipboardLease {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("ClipboardLease([REDACTED])")
    }
}

pub struct SecretClipboard<B> {
    backend: B,
    active: Option<ClipboardLease>,
}

impl<B> SecretClipboard<B> {
    pub fn new(backend: B) -> Self {
        Self { backend, active: None }
    }

    pub fn has_pending_clear(&self) -> bool {
        self.active.is_some()
    }
}

impl<B: ClipboardBackend> SecretClipboard<B> {
    pub fn copy(
        &mut self,
        secret: ClipboardSecret,
        policy: ClipboardPolicy,
    ) -> Result<(), ClipboardHygieneError> {
        self.copy_at(secret, policy, Instant::now())
    }

    pub fn poll(&mut self) -> Result<ClipboardPoll, ClipboardHygieneError> {
        self.poll_at(Instant::now())
    }

    fn copy_at(
        &mut self,
        secret: ClipboardSecret,
        policy: ClipboardPolicy,
        now: Instant,
    ) -> Result<(), ClipboardHygieneError> {
        self.backend.set_text(secret.expose_secret())?;
        self.active = Some(ClipboardLease {
            expected: secret,
            clear_at: now + policy.clear_after(),
        });
        Ok(())
    }

    fn poll_at(&mut self, now: Instant) -> Result<ClipboardPoll, ClipboardHygieneError> {
        let Some(lease) = self.active.as_ref() else {
            return Ok(ClipboardPoll::Idle);
        };

        if now < lease.clear_at {
            return Ok(ClipboardPoll::Pending);
        }

        let expected = lease.expected.expose_secret();
        let should_clear = match self.backend.read_text()? {
            ClipboardRead::Text(observed) => observed.expose_secret() == expected,
            ClipboardRead::Unavailable => false,
        };

        if should_clear {
            self.backend.clear()?;
            self.active.take();
            Ok(ClipboardPoll::Cleared)
        } else {
            self.active.take();
            Ok(ClipboardPoll::PreservedExternalContent)
        }
    }
}

impl<B> fmt::Debug for SecretClipboard<B> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("SecretClipboard([REDACTED])")
    }
}

#[cfg(feature = "desktop")]
pub struct SystemClipboard {
    inner: arboard::Clipboard,
}

#[cfg(feature = "desktop")]
impl SystemClipboard {
    pub fn new() -> Result<Self, ClipboardHygieneError> {
        arboard::Clipboard::new()
            .map(|inner| Self { inner })
            .map_err(|_| ClipboardHygieneError::ClipboardUnavailable)
    }
}

#[cfg(feature = "desktop")]
impl ClipboardBackend for SystemClipboard {
    fn set_text(&mut self, text: &str) -> Result<(), ClipboardBackendError> {
        self.inner.set_text(text).map_err(|_| ClipboardBackendError)
    }

    fn read_text(&mut self) -> Result<ClipboardRead, ClipboardBackendError> {
        match self.inner.get_text() {
            Ok(text) => Ok(ClipboardRead::Text(ClipboardObservedText::from_owned(text))),
            Err(arboard::Error::ContentNotAvailable) => Ok(ClipboardRead::Unavailable),
            Err(_) => Err(ClipboardBackendError),
        }
    }

    fn clear(&mut self) -> Result<(), ClipboardBackendError> {
        self.inner.clear().map_err(|_| ClipboardBackendError)
    }
}

#[cfg(feature = "desktop")]
impl fmt::Debug for SystemClipboard {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("SystemClipboard")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::credential::SecretString;
    use crate::recovery_codes::RecoveryCodeSet;

    const SECRET_A: &str = "SYNTHETIC_CLIPBOARD_SECRET_ALPHA";
    const SECRET_B: &str = "SYNTHETIC_CLIPBOARD_SECRET_BETA";

    #[derive(Default)]
    struct FakeBackend {
        content: Option<String>,
        non_text: bool,
        fail_read: bool,
        fail_clear: bool,
        clear_count: usize,
    }

    impl ClipboardBackend for FakeBackend {
        fn set_text(&mut self, text: &str) -> Result<(), ClipboardBackendError> {
            self.content = Some(text.to_owned());
            self.non_text = false;
            Ok(())
        }

        fn read_text(&mut self) -> Result<ClipboardRead, ClipboardBackendError> {
            if self.fail_read {
                return Err(ClipboardBackendError);
            }
            if self.non_text {
                return Ok(ClipboardRead::Unavailable);
            }
            Ok(match self.content.as_ref() {
                Some(value) => ClipboardRead::Text(ClipboardObservedText::from_owned(value.clone())),
                None => ClipboardRead::Unavailable,
            })
        }

        fn clear(&mut self) -> Result<(), ClipboardBackendError> {
            if self.fail_clear {
                return Err(ClipboardBackendError);
            }
            self.clear_count += 1;
            self.content = None;
            self.non_text = false;
            Ok(())
        }
    }

    fn clipboard_secret(value: &str) -> ClipboardSecret {
        ClipboardSecret::from_secret_text(value)
    }

    #[test]
    fn default_policy_is_thirty_seconds() {
        assert_eq!(ClipboardPolicy::default().clear_after(), Duration::from_secs(30));
    }

    #[test]
    fn secret_types_have_redacted_debug_output() {
        let secret = clipboard_secret(SECRET_A);
        let observed = ClipboardObservedText::from_owned(SECRET_A.to_string());
        let controller = SecretClipboard::new(FakeBackend::default());
        assert_eq!(format!("{secret:?}"), "ClipboardSecret([REDACTED])");
        assert_eq!(format!("{observed:?}"), "ClipboardObservedText([REDACTED])");
        assert_eq!(format!("{controller:?}"), "SecretClipboard([REDACTED])");
        assert!(!format!("{secret:?}").contains(SECRET_A));
    }

    #[test]
    fn credential_and_recovery_code_sources_create_explicit_clipboard_copies() {
        let credential = Credential::new(
            SecretString::new("SYNTHETIC_SERVICE".to_string()),
            None,
            None,
            SecretString::new(SECRET_A.to_string()),
            None,
        ).expect("credential");
        let credential_copy = ClipboardSecret::from_credential_password(&credential);
        assert_eq!(credential_copy.expose_secret(), SECRET_A);

        let codes = RecoveryCodeSet::new(vec![SECRET_B.to_string()]).expect("recovery codes");
        let recovery_copy = ClipboardSecret::from_recovery_code(codes.code(0).expect("code"));
        assert_eq!(recovery_copy.expose_secret(), SECRET_B);
    }

    #[test]
    fn copy_waits_until_the_configured_deadline() {
        let now = Instant::now();
        let mut clipboard = SecretClipboard::new(FakeBackend::default());
        clipboard.copy_at(
            clipboard_secret(SECRET_A),
            ClipboardPolicy::new(Duration::from_secs(10)),
            now,
        ).expect("copy");
        assert_eq!(
            clipboard.poll_at(now + Duration::from_secs(9)).expect("poll"),
            ClipboardPoll::Pending
        );
        assert!(clipboard.has_pending_clear());
        assert_eq!(clipboard.backend.content.as_deref(), Some(SECRET_A));
    }

    #[test]
    fn unchanged_secret_is_cleared_after_deadline() {
        let now = Instant::now();
        let mut clipboard = SecretClipboard::new(FakeBackend::default());
        clipboard.copy_at(
            clipboard_secret(SECRET_A),
            ClipboardPolicy::new(Duration::from_secs(10)),
            now,
        ).expect("copy");
        assert_eq!(
            clipboard.poll_at(now + Duration::from_secs(10)).expect("poll"),
            ClipboardPoll::Cleared
        );
        assert!(!clipboard.has_pending_clear());
        assert!(clipboard.backend.content.is_none());
        assert_eq!(clipboard.backend.clear_count, 1);
    }

    #[test]
    fn changed_clipboard_content_is_never_cleared() {
        let now = Instant::now();
        let mut clipboard = SecretClipboard::new(FakeBackend::default());
        clipboard.copy_at(
            clipboard_secret(SECRET_A),
            ClipboardPolicy::new(Duration::from_secs(1)),
            now,
        ).expect("copy");
        clipboard.backend.content = Some("USER_REPLACEMENT_TEXT".to_string());

        assert_eq!(
            clipboard.poll_at(now + Duration::from_secs(1)).expect("poll"),
            ClipboardPoll::PreservedExternalContent
        );
        assert_eq!(clipboard.backend.content.as_deref(), Some("USER_REPLACEMENT_TEXT"));
        assert_eq!(clipboard.backend.clear_count, 0);
        assert!(!clipboard.has_pending_clear());
    }

    #[test]
    fn non_text_or_empty_clipboard_is_preserved_without_clear() {
        let now = Instant::now();
        let mut clipboard = SecretClipboard::new(FakeBackend::default());
        clipboard.copy_at(
            clipboard_secret(SECRET_A),
            ClipboardPolicy::new(Duration::ZERO),
            now,
        ).expect("copy");
        clipboard.backend.non_text = true;

        assert_eq!(
            clipboard.poll_at(now).expect("poll"),
            ClipboardPoll::PreservedExternalContent
        );
        assert_eq!(clipboard.backend.clear_count, 0);
    }

    #[test]
    fn read_failure_keeps_the_lease_for_retry() {
        let now = Instant::now();
        let mut clipboard = SecretClipboard::new(FakeBackend::default());
        clipboard.copy_at(
            clipboard_secret(SECRET_A),
            ClipboardPolicy::new(Duration::ZERO),
            now,
        ).expect("copy");
        clipboard.backend.fail_read = true;

        assert_eq!(
            clipboard.poll_at(now).unwrap_err(),
            ClipboardHygieneError::ClipboardUnavailable
        );
        assert!(clipboard.has_pending_clear());

        clipboard.backend.fail_read = false;
        assert_eq!(clipboard.poll_at(now).expect("retry"), ClipboardPoll::Cleared);
        assert!(!clipboard.has_pending_clear());
    }

    #[test]
    fn clear_failure_keeps_the_lease_for_retry() {
        let now = Instant::now();
        let mut clipboard = SecretClipboard::new(FakeBackend::default());
        clipboard.copy_at(
            clipboard_secret(SECRET_A),
            ClipboardPolicy::new(Duration::ZERO),
            now,
        ).expect("copy");
        clipboard.backend.fail_clear = true;

        assert_eq!(
            clipboard.poll_at(now).unwrap_err(),
            ClipboardHygieneError::ClipboardUnavailable
        );
        assert!(clipboard.has_pending_clear());
        assert_eq!(clipboard.backend.content.as_deref(), Some(SECRET_A));

        clipboard.backend.fail_clear = false;
        assert_eq!(clipboard.poll_at(now).expect("retry"), ClipboardPoll::Cleared);
    }

    #[test]
    fn copying_a_new_secret_replaces_the_previous_lease() {
        let now = Instant::now();
        let mut clipboard = SecretClipboard::new(FakeBackend::default());
        clipboard.copy_at(
            clipboard_secret(SECRET_A),
            ClipboardPolicy::new(Duration::from_secs(30)),
            now,
        ).expect("first copy");
        clipboard.copy_at(
            clipboard_secret(SECRET_B),
            ClipboardPolicy::new(Duration::from_secs(5)),
            now + Duration::from_secs(1),
        ).expect("second copy");

        assert_eq!(clipboard.backend.content.as_deref(), Some(SECRET_B));
        assert_eq!(
            clipboard.poll_at(now + Duration::from_secs(6)).expect("poll"),
            ClipboardPoll::Cleared
        );
    }

    #[test]
    fn errors_never_echo_secret_values() {
        let error = ClipboardHygieneError::ClipboardUnavailable;
        assert_eq!(error.to_string(), "clipboard operation failed");
        assert!(!error.to_string().contains(SECRET_A));
        assert_eq!(ClipboardBackendError.to_string(), "clipboard backend operation failed");
    }
}

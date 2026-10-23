use crate::credential::{Credential, CredentialError, SecretString};
use std::error::Error;
use std::fmt;

/// In-memory boundary for an already-existing credential entered or pasted by
/// the user.
///
/// Plain Strings are accepted only at construction and are immediately wrapped
/// in SecretString. This type has no persistence, UI, clipboard, session, or
/// generic autosave behavior.
#[must_use]
pub struct ExistingCredentialInput {
    service: SecretString,
    username_or_email: Option<SecretString>,
    url: Option<SecretString>,
    password: SecretString,
    notes: Option<SecretString>,
}

impl ExistingCredentialInput {
    pub fn new(
        service: String,
        username_or_email: Option<String>,
        url: Option<String>,
        password: String,
        notes: Option<String>,
    ) -> Self {
        Self {
            service: SecretString::new(service),
            username_or_email: username_or_email.map(SecretString::new),
            url: url.map(SecretString::new),
            password: SecretString::new(password),
            notes: notes.map(SecretString::new),
        }
    }

    pub fn into_credential(self) -> Result<Credential, ExistingCredentialInputError> {
        Credential::new(
            self.service,
            self.username_or_email,
            self.url,
            self.password,
            self.notes,
        )
        .map_err(ExistingCredentialInputError::from)
    }
}

impl fmt::Debug for ExistingCredentialInput {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("ExistingCredentialInput([REDACTED])")
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExistingCredentialInputError {
    ServiceRequired,
}

impl fmt::Display for ExistingCredentialInputError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ServiceRequired => {
                formatter.write_str("existing credential service is required")
            }
        }
    }
}

impl Error for ExistingCredentialInputError {}

impl From<CredentialError> for ExistingCredentialInputError {
    fn from(error: CredentialError) -> Self {
        match error {
            CredentialError::ServiceRequired => Self::ServiceRequired,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SERVICE_SENTINEL: &str = "SYNTHETIC_EXISTING_SERVICE";
    const USER_SENTINEL: &str = "SYNTHETIC_EXISTING_USER";
    const URL_SENTINEL: &str = "SYNTHETIC_EXISTING_URL";
    const PASSWORD_SENTINEL: &str = "SYNTHETIC_EXISTING_SECRET";
    const NOTES_SENTINEL: &str = "SYNTHETIC_EXISTING_NOTES";

    fn synthetic_input() -> ExistingCredentialInput {
        ExistingCredentialInput::new(
            SERVICE_SENTINEL.to_string(),
            Some(USER_SENTINEL.to_string()),
            Some(URL_SENTINEL.to_string()),
            PASSWORD_SENTINEL.to_string(),
            Some(NOTES_SENTINEL.to_string()),
        )
    }

    #[test]
    fn existing_input_converges_on_same_credential_model() {
        let credential = synthetic_input()
            .into_credential()
            .expect("existing credential input");

        assert_eq!(credential.service().expose_secret(), SERVICE_SENTINEL);
        assert_eq!(
            credential
                .username_or_email()
                .expect("username/email")
                .expose_secret(),
            USER_SENTINEL
        );
        assert_eq!(
            credential.url().expect("url").expose_secret(),
            URL_SENTINEL
        );
        assert_eq!(
            credential.password().expose_secret(),
            PASSWORD_SENTINEL
        );
        assert_eq!(
            credential.notes().expect("notes").expose_secret(),
            NOTES_SENTINEL
        );
    }

    #[test]
    fn existing_password_is_preserved_exactly_without_policy_rewrite() {
        let credential = ExistingCredentialInput::new(
            SERVICE_SENTINEL.to_string(),
            None,
            None,
            String::new(),
            None,
        )
        .into_credential()
        .expect("existing credential");

        assert!(credential.password().is_empty());
    }

    #[test]
    fn input_debug_is_redacted() {
        let input = synthetic_input();
        let debug = format!("{input:?}");

        assert_eq!(debug, "ExistingCredentialInput([REDACTED])");

        for sentinel in [
            SERVICE_SENTINEL,
            USER_SENTINEL,
            URL_SENTINEL,
            PASSWORD_SENTINEL,
            NOTES_SENTINEL,
        ] {
            assert!(!debug.contains(sentinel));
        }
    }

    #[test]
    fn validation_error_does_not_echo_secret_values() {
        let error = ExistingCredentialInput::new(
            "   ".to_string(),
            None,
            None,
            PASSWORD_SENTINEL.to_string(),
            None,
        )
        .into_credential()
        .expect_err("blank service must be rejected");

        assert_eq!(error, ExistingCredentialInputError::ServiceRequired);
        assert_eq!(
            error.to_string(),
            "existing credential service is required"
        );
        assert!(!error.to_string().contains(PASSWORD_SENTINEL));
    }

    #[test]
    fn optional_fields_remain_optional() {
        let credential = ExistingCredentialInput::new(
            SERVICE_SENTINEL.to_string(),
            None,
            None,
            PASSWORD_SENTINEL.to_string(),
            None,
        )
        .into_credential()
        .expect("existing credential");

        assert!(credential.username_or_email().is_none());
        assert!(credential.url().is_none());
        assert!(credential.notes().is_none());
    }
}

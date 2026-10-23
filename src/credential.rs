use std::error::Error;
use std::fmt;

/// In-memory owned text that belongs to the secret-management subsystem.
///
/// This type deliberately does not implement Clone, Display, Deref, AsRef<str>,
/// or common serialization traits. Access to the contained text is explicit.
///
/// It is not a guarantee of complete memory erasure. See
/// docs/SECRET_DATA_BOUNDARY.md.
#[must_use]
pub struct SecretString {
    value: String,
}

impl SecretString {
    pub fn new(value: String) -> Self {
        Self { value }
    }

    pub fn expose_secret(&self) -> &str {
        &self.value
    }

    pub fn is_empty(&self) -> bool {
        self.value.is_empty()
    }

    pub fn len(&self) -> usize {
        self.value.len()
    }
}

impl fmt::Debug for SecretString {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("SecretString([REDACTED])")
    }
}

/// Source-agnostic in-memory Credential.
///
/// Generated passwords and existing passwords are intentionally not
/// distinguished by this model. A later input-boundary task will construct the
/// same Credential shape from either source before any Vault persistence exists.
#[must_use]
pub struct Credential {
    service: SecretString,
    username_or_email: Option<SecretString>,
    url: Option<SecretString>,
    password: SecretString,
    notes: Option<SecretString>,
}

impl Credential {
    pub fn new(
        service: SecretString,
        username_or_email: Option<SecretString>,
        url: Option<SecretString>,
        password: SecretString,
        notes: Option<SecretString>,
    ) -> Result<Self, CredentialError> {
        if service.expose_secret().trim().is_empty() {
            return Err(CredentialError::ServiceRequired);
        }

        Ok(Self {
            service,
            username_or_email,
            url,
            password,
            notes,
        })
    }

    pub fn service(&self) -> &SecretString {
        &self.service
    }

    pub fn username_or_email(&self) -> Option<&SecretString> {
        self.username_or_email.as_ref()
    }

    pub fn url(&self) -> Option<&SecretString> {
        self.url.as_ref()
    }

    pub fn password(&self) -> &SecretString {
        &self.password
    }

    pub fn notes(&self) -> Option<&SecretString> {
        self.notes.as_ref()
    }
}

impl fmt::Debug for Credential {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("Credential([REDACTED])")
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CredentialError {
    ServiceRequired,
}

impl fmt::Display for CredentialError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ServiceRequired => formatter.write_str("credential service is required"),
        }
    }
}

impl Error for CredentialError {}

#[cfg(test)]
mod tests {
    use super::*;

    const SERVICE_SENTINEL: &str = "SYNTHETIC_SERVICE_SENTINEL";
    const USER_SENTINEL: &str = "SYNTHETIC_USER_SENTINEL";
    const URL_SENTINEL: &str = "SYNTHETIC_URL_SENTINEL";
    const SECRET_SENTINEL: &str = "SYNTHETIC_SECRET_SENTINEL";
    const NOTES_SENTINEL: &str = "SYNTHETIC_NOTES_SENTINEL";

    fn synthetic_credential() -> Credential {
        Credential::new(
            SecretString::new(SERVICE_SENTINEL.to_string()),
            Some(SecretString::new(USER_SENTINEL.to_string())),
            Some(SecretString::new(URL_SENTINEL.to_string())),
            SecretString::new(SECRET_SENTINEL.to_string()),
            Some(SecretString::new(NOTES_SENTINEL.to_string())),
        )
        .expect("synthetic credential")
    }

    #[test]
    fn credential_owns_required_and_optional_fields() {
        let credential = synthetic_credential();

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
        assert_eq!(credential.password().expose_secret(), SECRET_SENTINEL);
        assert_eq!(
            credential.notes().expect("notes").expose_secret(),
            NOTES_SENTINEL
        );
    }

    #[test]
    fn credential_debug_never_contains_field_values() {
        let credential = synthetic_credential();
        let debug = format!("{credential:?}");

        assert_eq!(debug, "Credential([REDACTED])");

        for sentinel in [
            SERVICE_SENTINEL,
            USER_SENTINEL,
            URL_SENTINEL,
            SECRET_SENTINEL,
            NOTES_SENTINEL,
        ] {
            assert!(!debug.contains(sentinel));
        }
    }

    #[test]
    fn secret_string_debug_is_redacted() {
        let value = SecretString::new(SECRET_SENTINEL.to_string());
        let debug = format!("{value:?}");

        assert_eq!(debug, "SecretString([REDACTED])");
        assert!(!debug.contains(SECRET_SENTINEL));
    }

    #[test]
    fn service_is_required_without_echoing_input() {
        let error = Credential::new(
            SecretString::new("   ".to_string()),
            None,
            None,
            SecretString::new(SECRET_SENTINEL.to_string()),
            None,
        )
        .expect_err("blank service must be rejected");

        assert_eq!(error, CredentialError::ServiceRequired);
        assert_eq!(error.to_string(), "credential service is required");
        assert!(!error.to_string().contains(SECRET_SENTINEL));
    }

    #[test]
    fn optional_fields_may_be_absent() {
        let credential = Credential::new(
            SecretString::new(SERVICE_SENTINEL.to_string()),
            None,
            None,
            SecretString::new(SECRET_SENTINEL.to_string()),
            None,
        )
        .expect("credential");

        assert!(credential.username_or_email().is_none());
        assert!(credential.url().is_none());
        assert!(credential.notes().is_none());
    }
}

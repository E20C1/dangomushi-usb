use crate::credential::{Credential, CredentialError, SecretString};
use crate::password_generator::GeneratedPassword;
use std::error::Error;
use std::fmt;

/// In-memory boundary that converges a generated password on the same
/// source-agnostic Credential model used by existing-password input.
///
/// No persistence, UI, clipboard, session, autosave, or Vault serialization is
/// performed here.
#[must_use]
pub struct GeneratedCredentialInput {
    service: SecretString,
    username_or_email: Option<SecretString>,
    url: Option<SecretString>,
    password: GeneratedPassword,
    notes: Option<SecretString>,
}

impl GeneratedCredentialInput {
    pub fn new(
        service: String,
        username_or_email: Option<String>,
        url: Option<String>,
        password: GeneratedPassword,
        notes: Option<String>,
    ) -> Self {
        Self {
            service: SecretString::new(service),
            username_or_email: username_or_email.map(SecretString::new),
            url: url.map(SecretString::new),
            password,
            notes: notes.map(SecretString::new),
        }
    }

    pub fn into_credential(self) -> Result<Credential, GeneratedCredentialInputError> {
        let password = SecretString::new(self.password.into_owned_secret());

        Credential::new(
            self.service,
            self.username_or_email,
            self.url,
            password,
            self.notes,
        )
        .map_err(GeneratedCredentialInputError::from)
    }
}

impl fmt::Debug for GeneratedCredentialInput {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("GeneratedCredentialInput([REDACTED])")
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GeneratedCredentialInputError {
    ServiceRequired,
}

impl fmt::Display for GeneratedCredentialInputError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ServiceRequired => {
                formatter.write_str("generated credential service is required")
            }
        }
    }
}

impl Error for GeneratedCredentialInputError {}

impl From<CredentialError> for GeneratedCredentialInputError {
    fn from(error: CredentialError) -> Self {
        match error {
            CredentialError::ServiceRequired => Self::ServiceRequired,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::password_generator::{
        generate_with_source, GeneratorConfig, RandomSource, RandomSourceError,
    };

    const SERVICE_SENTINEL: &str = "SYNTHETIC_GENERATED_SERVICE";
    const USER_SENTINEL: &str = "SYNTHETIC_GENERATED_USER";
    const URL_SENTINEL: &str = "SYNTHETIC_GENERATED_URL";
    const NOTES_SENTINEL: &str = "SYNTHETIC_GENERATED_NOTES";

    struct CounterSource {
        next: u8,
    }

    impl CounterSource {
        fn new() -> Self {
            Self { next: 0 }
        }
    }

    impl RandomSource for CounterSource {
        fn fill_bytes(&mut self, destination: &mut [u8]) -> Result<(), RandomSourceError> {
            for byte in destination {
                *byte = self.next;
                self.next = self.next.wrapping_add(1);
            }
            Ok(())
        }
    }

    fn generated_password() -> GeneratedPassword {
        let config = GeneratorConfig {
            length: 24,
            uppercase: true,
            lowercase: true,
            digits: true,
            symbols: true,
            require_each_selected_group: true,
            exclude_ambiguous: true,
            ..GeneratorConfig::default()
        };
        let mut source = CounterSource::new();
        generate_with_source(&config, &mut source).expect("generated password")
    }

    #[test]
    fn generated_password_converges_without_reallocating_secret_string() {
        let generated = generated_password();
        let password_ptr = generated.expose_secret().as_ptr();
        let password_len = generated.len();

        let credential = GeneratedCredentialInput::new(
            SERVICE_SENTINEL.to_string(),
            Some(USER_SENTINEL.to_string()),
            Some(URL_SENTINEL.to_string()),
            generated,
            Some(NOTES_SENTINEL.to_string()),
        )
        .into_credential()
        .expect("generated credential input");

        assert_eq!(credential.service().expose_secret(), SERVICE_SENTINEL);
        assert_eq!(
            credential
                .username_or_email()
                .expect("username/email")
                .expose_secret(),
            USER_SENTINEL
        );
        assert_eq!(credential.url().expect("url").expose_secret(), URL_SENTINEL);
        assert_eq!(credential.password().len(), password_len);
        assert_eq!(credential.password().expose_secret().as_ptr(), password_ptr);
        assert_eq!(
            credential.notes().expect("notes").expose_secret(),
            NOTES_SENTINEL
        );
    }

    #[test]
    fn generated_input_debug_is_redacted() {
        let input = GeneratedCredentialInput::new(
            SERVICE_SENTINEL.to_string(),
            None,
            None,
            generated_password(),
            None,
        );

        let debug = format!("{input:?}");
        assert_eq!(debug, "GeneratedCredentialInput([REDACTED])");
        assert!(!debug.contains(SERVICE_SENTINEL));
    }

    #[test]
    fn validation_error_is_generic() {
        let error = GeneratedCredentialInput::new(
            "   ".to_string(),
            None,
            None,
            generated_password(),
            None,
        )
        .into_credential()
        .expect_err("blank service must fail");

        assert_eq!(error, GeneratedCredentialInputError::ServiceRequired);
        assert_eq!(error.to_string(), "generated credential service is required");
    }

    #[test]
    fn optional_fields_remain_optional() {
        let credential = GeneratedCredentialInput::new(
            SERVICE_SENTINEL.to_string(),
            None,
            None,
            generated_password(),
            None,
        )
        .into_credential()
        .expect("generated credential");

        assert!(credential.username_or_email().is_none());
        assert!(credential.url().is_none());
        assert!(credential.notes().is_none());
    }
}

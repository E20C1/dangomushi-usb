# Credential Model

Status: implemented as an in-memory M2-S core model; no UI or persistence is attached.

## Purpose

The Credential model establishes one source-agnostic in-memory shape for future secret workflows.

~~~text
Generate new password ----+
                          |
                          v
                       Credential
                          ^
                          |
Enter existing password --+
~~~

Both generated-password and existing-password input boundaries now converge on this same model. The Credential deliberately does not record which source produced the password.

## Fields

A Credential owns:

- Service
- Username or Email (optional)
- URL (optional)
- Password
- Notes (optional)

Once these fields are part of a decrypted Credential object, all of them use the same secret-aware ownership boundary, even when a field such as Service or URL would not normally be secret by itself.

## SecretString

Credential text is held by `SecretString`.

The type:

- owns its String,
- does not implement Clone,
- does not implement Display,
- does not implement Deref or AsRef<str>,
- does not derive serialization,
- exposes content only through the explicit `expose_secret()` method,
- returns a fixed redacted representation from Debug.

Credential itself also has a fixed redacted Debug representation.

This design reduces accidental formatting, cloning and generic serialization. It is not a claim that the bytes cannot exist in allocator, GUI, OS or crash-related copies. The memory-erasure limitations in `SECRET_DATA_BOUNDARY.md` remain authoritative.

## Validation

The model requires a non-blank Service.

No password-strength policy is applied here. The model must also be able to represent existing credentials rather than only passwords generated under DANGOMUSHI policy.

Errors report only structural validation failure and do not echo field contents.

## Persistence boundary

This module does not:

- write Workspace files,
- use Text Editor save/recovery,
- write session state,
- serialize plaintext JSON/TOML/CSV,
- encrypt or decrypt,
- access a Vault,
- access Clipboard,
- expose a native UI.

## Input convergence status

Both `ExistingCredentialInput` and `GeneratedCredentialInput` consume their source-specific inputs into this same Credential model. Persistence-format work remains separate from the model.

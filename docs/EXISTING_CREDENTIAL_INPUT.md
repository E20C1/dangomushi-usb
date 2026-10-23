# Add Existing Credential Input Boundary

Status: implemented as an in-memory M2-S input boundary; no UI or persistence is attached.

## Purpose

DANGOMUSHI must accept credentials that already exist. The Vault must not become generator-only.

This boundary establishes the existing-password path:

~~~text
entered / pasted Strings
        |
        v
ExistingCredentialInput
        |
        v
SecretString ownership
        |
        v
Credential
~~~

The input path produces the same source-agnostic `Credential` model that future generated-password integration will use.

## Ownership

`ExistingCredentialInput::new` accepts owned Strings because future UI entry/paste surfaces naturally produce owned text buffers.

Construction immediately wraps those Strings in `SecretString`. The boundary does not keep a second plaintext copy.

`into_credential(self)` consumes the input object and moves its `SecretString` values into `Credential`. No clone is required by this path.

Both `ExistingCredentialInput` and the resulting `Credential` have fixed redacted Debug output and do not implement value-revealing Display formatting.

This reduces accidental disclosure. It is not a guarantee of complete memory erasure; GUI/OS/allocator limitations remain governed by `SECRET_DATA_BOUNDARY.md`.

## Existing-password compatibility

The input boundary does not apply Password Generator policy to existing credentials.

In particular, it preserves an existing password exactly, including an empty password. This is intentional: importing an existing credential must not silently rewrite, reject, or strengthen its password.

Service remains structurally required because that validation belongs to the shared Credential model.

## Explicit non-goals

This module does not:

- provide a native input form,
- read Clipboard,
- persist plaintext,
- call Text Editor save/recovery,
- write session state,
- serialize JSON/TOML/CSV,
- encrypt or decrypt,
- access a Vault,
- connect the Password Generator to Credential.

## Convergence

The architecture is now prepared for:

~~~text
Password Generator -------+
                           |
                           v
                       Credential
                           ^
                           |
ExistingCredentialInput ---+
                           |
                           v
                 future encrypted Vault
~~~

Only the existing-input side is wired to Credential in this task. Generator -> Credential remains a later roadmap task after Vault unlock/storage work.

## Exactly One Next Task

**Encrypted Vault storage**

That task must first re-evaluate maintained Rust cryptography/secret-management crates from official documentation, define an authenticated encrypted format, and implement a dedicated Vault storage API with durable temp -> sync -> verification -> replace/rollback behavior.

It must remain separate from generic Workspace File Browser/Text Editor persistence.

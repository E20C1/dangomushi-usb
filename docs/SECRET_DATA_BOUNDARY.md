# DANGOMUSHI Secret Data Boundary

Status: **architectural contract established; Password Generator Core, both Credential input paths, encrypted Vault storage, and Vault unlock/lock core implemented; Recovery Codes/UI integration remain unimplemented**

This document defines the mandatory boundary for future password, credential, recovery-code and vault work.

It is intentionally stricter than the normal Workspace/Text Editor boundary. A feature that handles secret values MUST follow this contract before it is marked READY.

## Scope

Secret data includes, at minimum:

- generated passwords,
- existing passwords entered or pasted by the user,
- credential passwords,
- master passphrases or equivalent unlock secrets,
- recovery codes,
- decrypted credential fields or notes that may contain secrets,
- plaintext vault payloads and derived sensitive key material.

Service names, usernames/email addresses and URLs are not always secret by themselves, but once they are part of a decrypted Credential object they MUST be handled by the secret subsystem rather than routed through generic persistence helpers.

## Mandatory isolation rules

1. Secret values MUST NOT be routed through the normal Text Editor.
2. Secret values MUST NOT be written to the generic recovery drafts under `workspace/state/autosave/`.
3. `workspace/state/session.v1` MUST NOT contain passwords, credential contents, master passphrases, recovery codes, plaintext vault payloads, or other decrypted secret values.
4. Secret values MUST NOT be persisted as ordinary plaintext Workspace JSON, TOML, CSV, text files, or similar generic user files.
5. Secret values MUST NOT appear in `Debug`, `Display`, error messages, panic text, telemetry, diagnostic output, or logs. Future secret-holding types MUST avoid value-revealing formatter implementations.
6. If a future PASSWORDS screen participates in session restoration, the session MAY remember only non-secret navigation state such as that the PASSWORDS view was active. Field contents, generated passwords, unlock input, decrypted Credentials and Recovery Codes MUST NOT be restored through the generic session layer.
7. Encrypted Vault persistence MUST use a dedicated secret-storage API boundary. The normal File Browser and Text Editor APIs MUST NOT be used to read, edit, create, or save Vault contents.
8. Vault persistence MUST be designed for removable-storage interruption. The storage API MUST permit a sequence equivalent to: create new temporary ciphertext -> flush/sync -> reopen/verify -> preserve previous valid vault -> replace -> rollback or retain the previous vault on failure. Exact filesystem mechanics remain platform-specific.
9. Cryptography MUST NOT be invented inside DANGOMUSHI. Before Vault implementation, maintained Rust cryptography and secret-management crates MUST be re-evaluated from their official documentation, security properties, maintenance status, platform support, and offline-build implications.
10. DANGOMUSHI MUST NOT claim guaranteed complete memory erasure of secrets. Rust-side best-effort zeroization may later reduce exposure, but GUI toolkit buffers, allocator copies, OS services, crash mechanisms, swap/page files, and clipboard implementations can retain copies outside the core's direct control.

These rules apply even if a shortcut would make an early prototype easier.

## Generic Workspace boundaries are explicitly non-secret

The existing generic data flows remain useful for ordinary files:

~~~text
Text Editor
  -> explicit SAVE -> ordinary Workspace file
  -> autosave      -> workspace/state/autosave/

Session restoration
  -> workspace/state/session.v1
~~~

Those paths are **not secret storage**.

Future password/credential code MUST NOT call the generic `text_editor::write_recovery`, `text_editor::save_text`, or equivalent generic serialization path for secret values.

The current session schema is limited to:

~~~text
view
browser_dir
browser_selected
editor_path
~~~

Secret subsystem development must preserve that separation. If a future session schema adds a PASSWORDS view identifier, it must remain navigation-only.

## Vault API boundary

The encrypted Vault storage format is now defined in `VAULT_STORAGE.md`. User-facing placement remains disabled until Vault unlock/lock and PASSWORDS integration are implemented.

Before encrypted storage is implemented, the architecture must provide a dedicated boundary conceptually similar to:

~~~text
Secret UI / Secret Core
        |
        v
Credential / Recovery Code model
        |
        v
Vault Storage API
        |
        +-> authenticated encryption
        +-> durable temporary write
        +-> verification
        +-> replace / rollback
~~~

The File Browser and Text Editor are not participants in this flow.

If the encrypted Vault is physically stored beneath the portable DANGOMUSHI directory, its path MUST either be outside the generic Workspace browser root or be explicitly excluded from generic file/editor access before the Vault feature is enabled.

## Input convergence requirement

DANGOMUSHI must support both:

~~~text
Generate new password ----+
                          |
                          v
                    Credential flow -> encrypted Vault
                          ^
                          |
Enter existing password --+
~~~

The Vault must never be generator-only. An existing password entered or pasted by the user is a first-class input path and ultimately reaches the same Credential persistence flow as a generated password.

The Password Generator Core, source-agnostic Credential model, existing-password input boundary, and generated-password input boundary now converge in memory on the same Credential type. Credential payload serialization remains separate from these input boundaries.

## Error and diagnostic policy

Secret-bearing operations must report failures by operation and non-secret context only.

Acceptable shape:

~~~text
vault write failed: authentication verification failed
~~~

Unacceptable shape:

~~~text
vault write failed for password=<value>
~~~

Tests, fixtures, examples, snapshots and documentation MUST use structural placeholders or synthetic non-secret metadata. They MUST NOT contain password or recovery-code values presented as real credentials.

## Memory and clipboard boundary

Future secret types should minimize cloning and lifetime where practical, but DANGOMUSHI cannot promise perfect erasure from all memory.

Clipboard handling is a separate boundary because the clipboard is owned by the operating system or desktop environment. A future clipboard-hygiene task must distinguish between:

- placing a secret on the clipboard,
- clearing it after a configured interval,
- clearing only when the clipboard still appears to contain the value DANGOMUSHI placed there,
- cases where the OS, clipboard manager, remote session, or another process may already have retained a copy.

## Dependency order

The security-sensitive subsystem is developed separately from normal M2 Local Tools.

After this boundary, the intended order is:

1. **Password Generator Core**
   - OS-derived CSPRNG in production,
   - configurable length,
   - uppercase,
   - lowercase,
   - digits,
   - configurable symbol set,
   - optional requirement for at least one character from every selected group,
   - ambiguous-character exclusion option,
   - rejection sampling or an equivalent method that avoids modulo bias,
   - injectable randomness source for deterministic unit tests,
   - no UI dependency,
   - no Vault/storage dependency.
2. **Credential model**
   - Service,
   - Username or Email,
   - URL,
   - Password,
   - Notes,
   - secret-aware formatting and ownership rules.
3. **Add Existing Credential input boundary**
   - explicit entry/paste path for an already-existing password,
   - converges with generated-password Credentials before persistence.
4. **Encrypted Vault storage**
   - dedicated API,
   - authenticated encrypted format,
   - durable replace/rollback strategy.
5. **Vault unlock / lock**
6. **Generator -> Credential integration**
7. **Recovery Codes**
8. **Clipboard hygiene**
9. **Secret Backup / Restore**

Normal M2 tools such as HASH / VERIFY continue independently. The secret subsystem does not replace or block M2.

## Exactly One Next Task

**Recovery Codes**

That next task must establish a separate secret-aware Recovery Code model and its encrypted-Vault payload boundary without routing recovery values through generic files, Text Editor, autosave, session restoration, logs, or Clipboard.

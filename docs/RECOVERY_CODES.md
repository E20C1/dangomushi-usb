# Recovery Codes

Status: implemented as an M2-S secret-aware model and versioned Vault section payload. No UI, Clipboard behavior, or Backup / Restore is attached.

## Scope

Recovery Codes are treated as secrets independently from normal Credentials.

This task supports already-issued textual Recovery Codes. It does not invent a Recovery Code generator and does not normalize, reformat, deduplicate, or strengthen user-provided codes.

## In-memory model

`RecoveryCode` owns one String.

`RecoveryCodeSet` owns one or more RecoveryCode values.

The model:

- preserves code text exactly,
- requires at least one non-blank code,
- accepts at most 128 codes,
- limits each code to 4096 UTF-8 bytes,
- does not implement Clone or Display for secret-bearing types,
- returns fixed redacted Debug output,
- zeroizes each directly owned String on Drop as best effort.

Complete memory erasure is still not guaranteed. GUI buffers, allocator copies, OS services, crash capture, swap/page files, Clipboard managers, and other copies remain outside this core-owned boundary.

## Vault section payload

Recovery Codes are not written as JSON, TOML, CSV, ordinary text files, generic autosave drafts, or session state.

`RecoveryCodeSet::encode_vault_payload()` creates a `RecoveryCodeVaultPayload`: an explicitly secret plaintext buffer intended only for inclusion in authenticated encrypted Vault data.

The payload is versioned and length-delimited:

~~~text
magic          8 bytes  DGMRC001
version        2 bytes  little endian
code_count     4 bytes
repeat:
  code_length  4 bytes
  code_utf8    N bytes
~~~

The format rejects:

- unknown magic,
- unsupported version,
- zero or excessive code count,
- zero-length or oversized codes,
- truncated fields,
- invalid UTF-8,
- trailing bytes,
- payloads larger than 1 MiB.

The plaintext payload buffer has fixed redacted Debug output and zeroizes its directly owned byte buffer on Drop.

## Encrypted Vault boundary

On the write side:

~~~text
RecoveryCodeSet
    -> RecoveryCodeVaultPayload
    -> future Vault document/section assembly
    -> UnlockedVault / VaultStore
    -> XChaCha20Poly1305 ciphertext
~~~

On the read side, a decrypted `VaultPlaintext` can be parsed through:

~~~text
RecoveryCodeSet::from_vault_plaintext(...)
~~~

This task deliberately does not define the complete combined Vault document containing both Credentials and Recovery Codes. The Recovery Code payload is a typed versioned section so a future combined document can include it without making Recovery Codes ordinary Workspace files.

## Secret isolation

The Recovery Code module does not use:

- Text Editor,
- File Browser,
- generic autosave,
- session restoration,
- Clipboard,
- ordinary plaintext Workspace persistence,
- logs or value-revealing formatting.

## Non-goals

Not implemented here:

- Recovery Code generation,
- PASSWORDS UI,
- automatic association with a Credential/service,
- Clipboard copy,
- Clipboard timed clearing,
- Secret Backup / Restore,
- full Credential + Recovery Code Vault document schema.

## Exactly One Next Task

**Clipboard hygiene**

That task should establish explicit secret-copy ownership, configurable delayed clearing, and conditional clearing only when the Clipboard still appears to contain the value DANGOMUSHI placed there. It must document that OS/desktop Clipboard managers or other processes may retain copies.

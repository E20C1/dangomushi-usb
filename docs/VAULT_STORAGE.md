# Encrypted Vault Storage

Status: implemented as an M2-S storage core. Vault unlock/lock, Credential serialization, UI, Recovery Codes and Clipboard integration are not implemented.

## Official crate review

The storage implementation re-evaluated current official documentation before selecting cryptographic dependencies.

### Adopted now

- **RustCrypto chacha20poly1305 0.11**
  - authenticated encryption with associated data (AEAD),
  - includes XChaCha20Poly1305,
  - 256-bit key,
  - XChaCha20Poly1305 uses a 192-bit / 24-byte nonce,
  - RustCrypto documents an NCC Group audit with no significant findings,
  - crate MSRV is Rust 1.85, below DANGOMUSHI's Rust 1.95 floor.
- **zeroize 1.9**
  - used for best-effort clearing of storage-owned raw VaultKey and decrypted VaultPlaintext buffers on drop,
  - Rust 1.85+.

DANGOMUSHI still does not claim complete memory erasure. Copies may exist in allocators, OS buffers, GUI toolkits, crash capture, swap, or other processes.

### Evaluated and deferred

- **RustCrypto argon2 0.6**
  - current maintained RustCrypto Argon2 implementation,
  - supports Argon2id,
  - will be evaluated again in the next Vault unlock/lock task where Master Passphrase -> VaultKey derivation actually belongs.
- **secrecy 0.10**
  - provides secret wrapper types and zeroization,
  - not adopted in this task because DANGOMUSHI already has a deliberately narrow SecretString ownership boundary; replacing that model would be a separate refactor rather than encrypted-storage work.

No encryption algorithm is implemented by DANGOMUSHI itself.

## API boundary

The storage API is `VaultStore` in `src/vault_storage.rs`.

It accepts:

- a dedicated `VaultKey`,
- bounded public KDF metadata,
- an opaque plaintext byte payload.

It returns `VaultPlaintext` only after successful AEAD authentication.

The storage layer does not know how a Credential is serialized. That is intentional: this task establishes encrypted durable storage without prematurely defining Credential/Vault payload serialization or UI behavior.

The storage root is supplied explicitly to `VaultStore`. The API has no dependency on the normal Workspace File Browser, Text Editor, autosave, or session modules.

## File set

The dedicated storage directory uses only fixed filenames:

~~~text
vault.dgmv
vault.pending.dgmv
vault.previous.dgmv
~~~

The caller must place this dedicated directory outside the generic Workspace browser root before user-facing Vault functionality is enabled.

Symlink roots and symlink/non-regular fixed files are rejected.

## Format v1

The cleartext header is fixed-size and authenticated as AEAD associated data.

~~~text
magic                 8 bytes  DGMVLT01
format_version         2 bytes  little endian
cipher_id              1 byte   XChaCha20Poly1305
kdf_id                 1 byte   Argon2id metadata profile
kdf_salt              16 bytes
kdf_memory_kib         4 bytes
kdf_iterations         4 bytes
kdf_parallelism        4 bytes
nonce                  24 bytes
ciphertext_length      8 bytes
ciphertext + tag       variable
~~~

The KDF fields are public metadata needed by the unlock layer. They are not trusted merely because they can be parsed: they are size-bounded before use, and the entire header is authenticated as AAD once the correct VaultKey is available.

Pre-authentication KDF bounds are currently 256 MiB memory, 10 iterations, and parallelism 8, with the Argon2 structural requirement that memory is at least 8 KiB per lane. These caps limit resource consumption from unauthenticated metadata before a Vault key can authenticate the header.

Master Passphrase -> VaultKey derivation is implemented separately in `VAULT_UNLOCK.md`; the storage layer itself remains key-agnostic.

## Durable replacement

A save follows this sequence:

~~~text
plaintext
  -> XChaCha20Poly1305 encrypt with fresh OS nonce
  -> create_new vault.pending.dgmv
  -> write
  -> file sync_all
  -> directory metadata sync where supported
  -> reopen pending
  -> authenticate/decrypt
  -> compare with expected plaintext
  -> move current to vault.previous.dgmv
  -> move pending to vault.dgmv
  -> directory metadata sync
  -> reopen/authenticate current
  -> only then delete previous
~~~

If promotion fails after the previous Vault was preserved, the code attempts to restore the previous Vault.

On startup/load, interrupted layouts are repaired conservatively:

- valid current wins and stale pending/previous files are removed,
- missing current + valid pending promotes the pending Vault,
- invalid current + valid previous rolls back to previous,
- missing current + valid previous restores previous,
- unauthenticated data is never returned.

On Unix, directory metadata is fsynced through the directory handle. On Windows, DANGOMUSHI attempts to open and flush the directory using backup-semantics; the standard library does not provide an equally strong portable directory-fsync guarantee, so file contents are always sync_all'd and directory flushing is best-effort.

## Secret memory boundary

`VaultKey` and `VaultPlaintext`:

- do not implement Display,
- use fixed redacted Debug output,
- zeroize their directly owned byte buffers on drop.

This is defense-in-depth only and does not change the project's documented limitation that complete secret erasure cannot be guaranteed.

## Size boundary

Vault plaintext is currently limited to 64 MiB to prevent an untrusted file header from causing unbounded allocation.

## Non-goals in this task

Not implemented:

- Master Passphrase input,
- Argon2id key derivation,
- Vault unlock/lock session,
- Credential serialization,
- Generator -> Credential integration,
- PASSWORDS UI,
- Recovery Codes,
- Clipboard operations,
- generic Workspace browsing/editing of Vault contents,
- Backup / Restore.

## Exactly One Next Task

**Vault unlock / lock**

That task should derive a VaultKey from a Master Passphrase and the bounded public KDF metadata, own the unlocked key for a deliberate lifetime, and drop/zeroize storage-owned key material on lock. It must not yet add Generator -> Credential integration.

# Vault Unlock / Lock

Status: implemented as an M2-S core lifetime boundary. No PASSWORDS UI, Credential payload format, Generator integration, Recovery Codes, or Clipboard behavior is attached.

## KDF selection

Master Passphrase -> VaultKey derivation uses RustCrypto `argon2 0.6` with:

- Argon2id,
- Argon2 version 0x13 / version 19,
- a 32-byte raw output used as the XChaCha20Poly1305 VaultKey,
- the 16-byte salt and cost parameters stored in the authenticated Vault header.

RustCrypto documents `hash_password_into` for raw output and `Params::new(m_cost, t_cost, p_cost, output_len)` for explicit cost configuration.

The default new-Vault profile is:

~~~text
memory      19 MiB / 19456 KiB
iterations  2
parallelism 1
~~~

This matches RustCrypto's current default Argon2 parameters and OWASP's current minimum Argon2id configuration. The project may benchmark and strengthen the default later while retaining per-Vault metadata for compatibility.

The KDF metadata remains bounded before Argon2 is invoked. It is public, pre-authentication metadata and therefore must never be treated as trusted merely because it parsed successfully.

## Master Passphrase boundary

`MasterPassphrase::new(String)` consumes an owned String and transfers its allocation into a byte vector rather than cloning it.

The type:

- does not implement Clone,
- does not implement Display,
- has fixed redacted Debug output,
- zeroizes the directly owned byte buffer on Drop,
- is consumed by initialize/unlock operations.

After key derivation, the core explicitly drops the MasterPassphrase before Vault authentication/decryption continues.

This only reduces the lifetime of the core-owned copy. DANGOMUSHI still does not claim complete erasure from GUI buffers, allocator copies, swap, crash capture, OS services, or other locations.

## Unlocked lifetime

`UnlockedVault` owns exactly:

~~~text
VaultStore
VaultKey
VaultKdfMetadata
~~~

The VaultKey exists only for the lifetime of the `UnlockedVault` object.

`UnlockedVault::lock(self)` consumes the unlocked object, explicitly drops the VaultKey, and returns the non-secret `VaultStore`.

Dropping `UnlockedVault` without calling `lock` also drops the VaultKey through normal Rust ownership.

The type has a fixed redacted Debug representation and does not expose the key bytes.

## New Vault initialization

`UnlockedVault::initialize_new`:

1. refuses to overwrite a detected existing Vault,
2. generates a fresh 16-byte salt from the OS random source,
3. derives a 32-byte key with Argon2id,
4. drops the Master Passphrase,
5. stores the caller-provided opaque initial payload through the existing authenticated/durable VaultStore,
6. returns an unlocked lifetime holding the derived key.

Credential serialization is intentionally not defined here.

## Existing Vault unlock

`UnlockedVault::unlock_existing`:

1. reads bounded public KDF metadata,
2. derives the candidate key with Argon2id,
3. drops the Master Passphrase,
4. asks VaultStore to authenticate/decrypt the Vault,
5. returns an unlocked lifetime only if authentication succeeds.

Wrong passphrases and AEAD authentication failures are reduced to the same non-secret `vault unlock failed` error.

## KDF temporary memory

The Argon2 dependency is built with its `alloc` and `zeroize` features. DANGOMUSHI also zeroizes its directly owned derived-key staging buffer after transferring the derived value into `VaultKey`.

This is defense in depth, not a complete-memory-erasure guarantee.

## Non-goals

Still not implemented:

- PASSWORDS UI,
- session restoration of secret fields,
- Credential serialization,
- Generator -> Credential integration,
- Recovery Codes,
- Clipboard operations,
- Secret Backup / Restore.

## Exactly One Next Task

**Generator -> Credential integration**

That task should move a generated password into the existing source-agnostic Credential path without using generic Text Editor/autosave/session persistence. It must not yet add Recovery Codes or Clipboard behavior.

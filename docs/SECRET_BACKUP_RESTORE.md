# Secret Backup / Restore

Status: implemented as the final M2-S core boundary. It operates on authenticated encrypted Vault ciphertext only. No UI or generic Workspace Backup integration is attached.

## Backup representation

A secret backup is a versioned container:

~~~text
magic          8 bytes  DGMBAK01
version        2 bytes  little endian, currently 1
vault_length   8 bytes  little endian
vault_bytes    N bytes  existing DGMVLT01 Vault ciphertext
~~~

The inner `vault_bytes` are the same authenticated Vault representation already produced by `VaultStore`.

DANGOMUSHI does not decrypt Credentials, Recovery Codes, or the combined Vault plaintext to create a backup, and it does not add a second encryption construction.

## Backup creation

`create_backup(&UnlockedVault, destination)` requires an unlocked source Vault.

Before writing, the source `VaultStore` recovers its normal same-key interrupted state if needed, reads the current ciphertext, and authenticates it with the active Vault key.

The backup destination then uses:

~~~text
authenticated source ciphertext
    -> DGMBAK01 container
    -> create-new sibling .pending
    -> sync
    -> reread exact bytes
    -> create-new final backup
    -> sync
    -> reread exact bytes
    -> remove .pending
~~~

An existing final backup is never silently overwritten. A conflicting pending file is also rejected.

The caller is responsible for choosing a user-visible backup location. Future UI integration should prefer a dedicated location rather than routing secret backup creation through generic Workspace export helpers.

## Restore

`restore_backup(backup, VaultStore, MasterPassphrase)` reads and bounds-checks the backup, parses the inner Vault KDF metadata, derives the Vault key, authenticates the inner Vault before target mutation, then performs the normal pending/current/previous replacement flow.

A target containing `vault.pending.dgmv` or `vault.previous.dgmv` is rejected as ambiguous. For a stable target, an existing current Vault is preserved as rollback material until the restored current Vault re-authenticates successfully.

Restore returns an `UnlockedVault` using the verified backup key and metadata. A wrong passphrase or tampered backup fails before the target is replaced.

## Security boundary

Secret Backup / Restore does not use Text Editor, File Browser, generic autosave, session restoration, generic Workspace Backup / Restore, clipboard, plaintext JSON/TOML/CSV/text export, or logs containing secret values.

The backup itself is encrypted ciphertext, but possession still exposes public format/KDF metadata and gives an attacker material for offline passphrase guessing. Backup files therefore remain sensitive assets and should be protected like the primary encrypted Vault.

## Failure semantics

Errors are operation-level only. They do not include passphrases, decrypted Vault contents, Credential values, or Recovery Codes.

A destination with leftover normal Vault pending/previous files is deliberately not auto-reconciled during backup restore because the backup key may differ from the current Vault key. The normal Vault must first be recovered to a stable layout with its own credentials before restoring over it.

## M2-S completion

With Secret Backup / Restore implemented, the M2-S core sequence is complete.

The repository-wide Exactly One Next Task returns to normal M2:

**SHA-256 / integrity verification**

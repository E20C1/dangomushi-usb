# Generator -> Credential Integration

Status: implemented as an in-memory M2-S convergence boundary. No Credential serialization, Vault payload format, Recovery Codes, Clipboard handling, or PASSWORDS UI is attached.

## Purpose

DANGOMUSHI now has both password sources converging on the same source-agnostic Credential model:

~~~text
Password Generator --------+
                            |
                            v
                        Credential
                            ^
                            |
ExistingCredentialInput ----+
~~~

The Credential does not record whether its password was generated or entered manually.

## Ownership transfer

The generator returns `GeneratedPassword`, which owns its String.

The integration path uses a crate-private consuming method:

~~~text
GeneratedPassword
    -> into_owned_secret()
    -> SecretString
    -> Credential
~~~

The generated password is not converted through a public Display implementation, generic formatter, Text Editor, autosave, session state, or serialization helper.

The consuming path moves the String allocation into `SecretString`; the production integration does not require cloning the generated password.

`GeneratedCredentialInput` itself has fixed redacted Debug output and no Display implementation.

## Metadata fields

Service, Username or Email, URL, and Notes enter the same `SecretString` ownership boundary already used by ExistingCredentialInput.

Service remains the only structural requirement at this model layer.

## Explicit non-goals

This task does not:

- define a Credential serialization format,
- save a Credential into Vault ciphertext,
- expose a PASSWORDS UI,
- touch generic Text Editor/autosave/session persistence,
- implement Recovery Codes,
- read or write Clipboard,
- implement Secret Backup / Restore.

## Convergence invariant

Generated and existing passwords now terminate in the same `Credential` type before any future persistence-format work.

That prevents a generator-only Vault path and avoids maintaining parallel credential models.

## Exactly One Next Task

**Recovery Codes**

That task should establish a separate secret-aware Recovery Code model and encrypted-Vault payload boundary without routing recovery values through generic files, Text Editor, autosave, session restoration, logs, or Clipboard.

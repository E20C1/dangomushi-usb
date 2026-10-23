# TEXT / JSON Utilities

Status: **M2 Text / JSON utilities implemented and READY in the native Tool Launcher.**

## Scope

TEXT / JSON is a generic, non-secret, in-memory utility.

It accepts UTF-8 text from either:

- direct native UI input, or
- the currently selected regular file in FILES through the existing read-only Text Editor file boundary.

The utility does not create files, write transformed output back to source files, autosave utility contents, store utility contents in generic session state, use the Clipboard, or send content over the network.

Input is limited to 4 MiB. Transform output is limited to 16 MiB.

## Text operations

### NORMALIZE LF

Converts CRLF and bare CR line endings to LF.

Other text is preserved.

### CLEAN LINES

Performs LF normalization and removes only ASCII space and tab characters at the end of each line.

Leading whitespace, internal whitespace, and other Unicode characters are preserved.

## JSON operations

JSON syntax validation uses serde_json 1.0.154.

The native utility exposes:

- VALIDATE JSON
- PRETTY JSON
- COMPACT JSON

Validation errors report line and column only. Input content is not copied into the error string.

Pretty and compact formatting intentionally do not deserialize a JSON document into a Value and then serialize that Value as the transformation step. serde_json is used first to validate the complete input, after which DANGOMUSHI changes only insignificant JSON whitespace outside string literals.

This means formatting preserves token content such as:

- object key order,
- duplicate object keys,
- number spelling such as 1e2 or 1.00,
- escape spelling and whitespace inside JSON strings.

Pretty formatting uses two-space indentation.

## Secret-data boundary

This utility is not a secret-processing surface.

Passwords, Master Passphrases, Recovery Codes, decrypted Vault data, and similar secret values remain governed by docs/SECRET_DATA_BOUNDARY.md and must not be routed through TEXT / JSON.

The generic GUI toolkit and ordinary String buffers used here are not zeroizing secret containers.

## Session boundary

Session restoration stores only that the TEXT / JSON native view was active.

It does not store:

- utility input,
- transformed output,
- JSON validation state,
- source file contents.

## Exactly One Next Task

The repository-wide Exactly One Next Task is:

**File comparison**

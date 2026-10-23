# Clipboard Hygiene

Status: implemented as an M2-S secret-copy boundary with an injectable backend and a feature-gated desktop OS clipboard adapter. No PASSWORDS UI or Secret Backup / Restore integration is attached.

## Scope

Clipboard use is treated as deliberate secret disclosure to an operating-system service, not as secret storage.

The boundary supports explicit copies from a generated password, a Credential password, and a Recovery Code. It deliberately does not expose a generic plaintext Workspace-to-clipboard persistence path.

## Ownership

`ClipboardSecret` owns the DANGOMUSHI-side plaintext copy used for one clipboard placement and delayed-clear lease.

It does not implement Clone or Display, returns fixed redacted Debug output, and best-effort zeroizes its directly owned String on Drop.

`ClipboardObservedText` similarly owns and zeroizes the temporary text read back from the OS during compare-before-clear.

This does not guarantee complete erasure. The OS clipboard, clipboard managers, remote-session software, another process, GUI/toolkit buffers, allocators, crash capture, swap/page files, or other services may retain copies outside DANGOMUSHI's control.

## Delayed clearing

`ClipboardPolicy` carries the configured clear delay. The default is 30 seconds.

`SecretClipboard` owns a clipboard backend and at most one active lease:

~~~text
ClipboardSecret
    -> backend.set_text(...)
    -> active lease { expected secret, clear deadline }

deadline reached
    -> backend.read_text(...)
    -> compare with expected secret
       -> same           -> clear clipboard
       -> different      -> preserve clipboard
       -> empty/non-text -> preserve clipboard
~~~

Copying a new secret replaces the previous lease only after the new clipboard write succeeds. A backend read or clear failure leaves the lease active so a later poll can retry.

## Compare-before-clear limitation

Reading and clearing are separate operating-system operations. Another process can theoretically replace clipboard content after DANGOMUSHI reads it but before the clear operation executes.

The portable backend therefore does not claim an atomic compare-and-clear guarantee. The contract is intentionally phrased as clearing only when the clipboard still **appears** to contain the DANGOMUSHI value.

## Desktop backend

The production desktop adapter uses `arboard` 3.7.0 behind the existing `desktop` Cargo feature.

Image clipboard support is disabled. Linux `wayland-data-control` is enabled in addition to the normal X11/XWayland path. Core unit tests use an injected fake backend and never touch the real OS clipboard.

`SystemClipboard` is intended to be owned for the lifetime of `SecretClipboard`, which also avoids prematurely dropping the clipboard owner on Linux environments where the source process may remain responsible for serving clipboard data.

## Error boundary

Clipboard backend failures are reduced to generic errors such as `clipboard operation failed`. Clipboard contents are never included in error strings or Debug output.

## Non-goals

Not implemented here:

- PASSWORDS UI,
- Recovery Code UI,
- background OS service or daemon,
- clipboard history deletion,
- deletion from third-party clipboard managers,
- remote-session clipboard cleanup,
- generic Workspace copy helpers,
- Secret Backup / Restore.

## Exactly One Next Task

**Secret Backup / Restore**

That task must use a dedicated encrypted secret backup/restore boundary rather than generic Workspace backup or export helpers.

# Repository Status and Safe Git Operations

Status: **M2 Repository status and safe Git operations implemented and READY in the native Tool Launcher.**

## Scope

REPOSITORIES inspects local Git working trees stored as direct children of:

~~~text
workspace/repositories/
~~~

Only this shape is supported:

~~~text
workspace/repositories/<repository>/
├─ .git/        real directory
└─ working tree
~~~

The repository directory itself must be a real directory, not a symlink. The .git entry must also be a real directory, not a symlink or gitfile.

Nested arbitrary paths, repositories outside workspace/repositories, linked worktrees represented by a .git file, and bare repositories are outside this first boundary.

## Fixed read-only operations

The native UI exposes exactly four Git operations:

1. STATUS
2. RECENT COMMITS
3. WORKTREE DIFF STAT
4. STAGED DIFF STAT

They map to fixed Git argv assembled by the Rust core. The user cannot provide an arbitrary Git subcommand, shell fragment, option list, revision expression, pathspec, remote, or executable.

This task does not expose:

- clone,
- fetch,
- pull,
- push,
- add,
- commit,
- checkout,
- switch,
- restore,
- reset,
- clean,
- stash,
- merge,
- rebase,
- cherry-pick,
- tag creation/deletion,
- branch creation/deletion,
- remote modification,
- arbitrary shell execution.

No network Git operation is present.

## Status boundary

STATUS uses Git porcelain output intended for machine parsing and requests branch information.

The invocation disables optional locks so status does not perform its optional index-refresh write. Submodule recursion is disabled and submodule state is ignored for this surface.

The UI reports:

- branch/tracking summary,
- staged entry count,
- unstaged entry count,
- untracked entry count,
- conflict count,
- up to 500 status rows.

The tool does not stage, unstage, restore, discard, or otherwise mutate those paths.

## Log and diff boundaries

RECENT COMMITS returns at most 20 commits using an explicit one-line format.

WORKTREE DIFF STAT and STAGED DIFF STAT expose statistics only. They explicitly disable external diff helpers and textconv execution, ignore submodules, disable rename detection, and disable color output.

Full patch application, patch creation, external diff tools, and user-defined text conversion are outside this task.

## Process boundary

Git is executed directly through std::process::Command. No shell is used.

Each invocation:

- uses a fixed executable name: git,
- uses fixed argv selected by DANGOMUSHI,
- sets stdin to null,
- disables terminal prompting,
- removes Git environment variables that can redirect repository/index/object locations, pagers, editors, askpass/SSH helpers, external diff behavior, config parameters, and Windows stdio redirection,
- disables optional Git locks,
- disables replace objects,
- disables fsmonitor and submodule recursion,
- disables log signature display,
- retains at most 1 MiB of stdout and 1 MiB of stderr while continuing to drain both pipes,
- kills the Git process after 15 seconds.

stderr is not copied into normal UI error messages. Git failures are reported by operation only.

DANGOMUSHI does not override Git safe.directory ownership checks. Git may therefore reject a repository that Git itself considers unsafe.

## Repository discovery

Repository discovery considers at most 512 direct children of workspace/repositories.

A candidate is exposed only when:

- the repositories root is a real in-Workspace directory,
- the candidate is a real direct child directory,
- its canonical path remains below workspace/repositories,
- its .git entry is a real directory.

Symlink repository entries and symlink .git entries are not exposed.

## Session and secret boundaries

Selected repository, status rows, branch names, commit subjects, log output, diff output, and Git errors remain ephemeral.

Generic session restoration stores only that the REPOSITORIES view was active.

REPOSITORIES is an ordinary non-secret utility. Passwords, Master Passphrases, Recovery Codes, decrypted Vault data, and similar secret-subsystem plaintext must not be intentionally routed through repository files, commit messages, Git output, or this UI.

## External Git configuration caveat

The tool does not claim to sandbox Git itself or validate every repository-internal object. The boundary is instead:

- fixed local repository root,
- fixed read-only commands,
- no shell,
- no network commands,
- external diff/textconv disabled,
- fsmonitor disabled,
- bounded retained output,
- bounded runtime.

Future mutating Git operations, remote operations, worktree support, or stronger process sandboxing require a separate design review.

## Exactly One Next Task

The repository-wide Exactly One Next Task is:

**Backup and restore workflow**

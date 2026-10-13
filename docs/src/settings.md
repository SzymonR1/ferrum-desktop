# Settings

Ferrum stores application preferences on Linux at `$XDG_CONFIG_HOME/ferrum/settings.toml` when `XDG_CONFIG_HOME` is an absolute path. Otherwise, it uses `$HOME/.config/ferrum/settings.toml`. If neither location can be determined, Ferrum remains usable with defaults and reports that settings cannot be saved.

Preferences include `auto_connect_on_startup` (default `true`),
`registry_credential_helper` (`none`, `secret_service`, or `pass`; default
`none`), and the saved Compose registration list (default empty). The helper
setting stores only the selected enum, never credentials. When auto-connect is
disabled, startup remains disconnected with an explanation, and the user can
still connect manually with **Retry connection**; the preference takes effect
at the next startup. A configured registry helper is used only after a separate
explicit opt-in for each pull or push on the Images page. Changes to preferences
are written only after explicitly selecting **Save**; **Cancel** discards those
preference edits.

The Compose list is changed only by its explicit **Save for future sessions**
and **Forget saved registration** row actions. It stores a unique local label,
ordered absolute UTF-8 Compose file paths, and validated profiles only. It does
not store project contents, environment variables, credentials, Engine
identity, or commands. Forget removes only the saved registration; its entry
remains session-only until Ferrum exits. Neither action runs Compose/Docker or
contacts an Engine. On restart, saved entries are restored unselected and
unvalidated; explicitly selecting one clears stale results and requires
successful revalidation before operations.

Settings schema v2 reads v1 and versionless settings with an empty saved list.
Invalid saved registrations are skipped with a static warning. The settings
file is capped at 256 KiB on load and before save; at most 32 registrations are
retained. Each label is non-empty, trimmed, unique, and at most 256 UTF-8
bytes; each record has 1–16 unique absolute UTF-8 paths in order and profiles
within Compose's existing bounds. Invalid, oversized, or future-version files
fall back to defaults and produce a safe warning; Ferrum does not overwrite the
file until the user explicitly saves.

## Linux settings path

The Linux settings-path contract is defined by [ADR 0009](architecture/decisions.md#adr-0009-linux-settings-path-anchoring). [ADR 0010](architecture/decisions.md#adr-0010-linux-settings-directory-permissions) supersedes it only for newly created remaining-path directory permissions, and [ADR 0011](architecture/decisions.md#adr-0011-linux-settings-load-permissions-and-hard-link-rejection) supplements it for load-time directory and hard-link protections.

At initialization, `SettingsStore` chooses the absolute `XDG_CONFIG_HOME` path
when available, or falls back to the absolute `HOME` path. It pins a file
descriptor for the longest existing directory before the final `ferrum`
settings directory. Symlink aliases already present in the user-selected
XDG/HOME path at initialization are honored.

Ferrum traverses the remaining directory components relative to that descriptor
with `openat` and `O_NOFOLLOW`. Saves request `0700` when creating each missing
component with `mkdirat`. When that call succeeds, Ferrum opens the component
with `openat` and `O_NOFOLLOW`, then sets its mode to exactly `0700` through
that descriptor before continuing. If `mkdirat` reports an existing entry,
Ferrum opens it without normalizing its mode. A load never creates directories,
and a missing directory or settings file leaves the defaults in use.
The final `ferrum` directory rejects symlinks on load and save. Loads set the
already-open final directory to exactly `0700` before opening `settings.toml`,
even when that file is absent; they reject symlinked, non-regular, or multiply
linked (`nlink != 1`) entries before changing file permissions or reading.
Accepted single-link regular files are set to `0600` before reading. Saves
reject an existing symlink or non-regular settings-file entry before writing;
`renameat` never follows a destination symlink.

Directories created by Ferrum in the remaining settings path are exactly
`0700` after a successful save, including intermediate XDG components. The
final `ferrum` settings directory is also set to `0700` on save; existing
ancestor directory modes are not changed during traversal. Settings files and
save-time temporary files use owner-only (`0600`) permissions. Save creates its
temporary file with `openat`, then resets its mode through the opened descriptor
so a restrictive umask cannot weaken owner access. It atomically replaces the
settings-file entry with `renameat` and removes a failed temporary file with
`unlinkat`.
Replacing the selected absolute parent path after initialization cannot
redirect settings access away from the pinned directory.

The configured `XDG_CONFIG_HOME`/`HOME` location remains user-trusted. Ferrum
honors its existing symlink aliases and does not confine it to `HOME`. All
suffix components and subsequent file operations remain relative to the pinned
descriptor; concurrent changes to the absolute parent path cannot redirect
access, and `renameat` never follows a destination symlink.
The settings-file link-count check and later mode change are not atomic.
Restricting the final directory to `0700` excludes other unprivileged UIDs, but
same-UID processes and privileged actors remain inside the local filesystem
trust boundary.

ADR 0010 supersedes ADR 0009 only for permissions on newly created
remaining-path directories: Ferrum normalizes those it creates to `0700`, while
existing remaining-path directories are opened without normalization. The
pre-existing rule to set the final `ferrum` directory to `0700` on save remains
in effect. [ADR 0011](architecture/decisions.md#adr-0011-linux-settings-load-permissions-and-hard-link-rejection) supplements both decisions with load-time directory and single-link settings-file checks; save-time rules do not change.

Because `mkdirat` and `openat` are separate operations, an actor able to modify
the parent directory may replace a newly created name with a real directory
before it is opened, subject to sticky-directory rules. `O_NOFOLLOW` rejects
symlinks, not real-directory replacements. If an ordinary unprivileged process
opens a foreign-owned replacement, its descriptor-based permission change
fails with `EPERM`, so saving stops before creating a temporary file or writing
settings; an owner or a process with `CAP_FOWNER` can normalize it. If umask
removes owner-read permission required for the `O_RDONLY` open, opening likewise
fails before any settings write.

Ferrum does not persist credential material, secrets, endpoints, or remote
contexts. The registry helper preference is only the `none`,
`secret_service`, or `pass` enum. Pulls and pushes remain anonymous by
default; each Linux local-Docker operation requires its own explicit opt-in.

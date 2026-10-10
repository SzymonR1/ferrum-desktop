# ADR 0009: Linux settings path anchoring

- **Status:** Accepted for the Linux settings-path contract; issues #19 and #10 remain open.
- **Date:** 2026-10-09

## Context

Ferrum stores its startup preference under an absolute `XDG_CONFIG_HOME` path
when one is available, or under `$HOME/.config` otherwise. The configured
location is selected by the user and may contain symlink aliases. Reopening an
absolute path for each operation can allow a parent-path replacement to direct
later accesses somewhere else. The settings file also needs private permissions
and safe handling of missing, malformed, or future-version data.

This decision defines the Linux path-handling contract for the settings store.
It does not add settings support on other platforms or complete security issue
#19 or engine compatibility issue #10.

## Decision

- At initialization, select an absolute `XDG_CONFIG_HOME` path, falling back
  to absolute `HOME` if XDG is unavailable or relative. Pin a file descriptor
  for the longest existing directory before the final `ferrum` settings
  directory. Honor symlink aliases already present in the user-selected
  XDG/HOME path at that time.
- Traverse the remaining directory components relative to the retained
  descriptor with `openat` and `O_NOFOLLOW`. Save may create missing components
  with `mkdirat`, then open each component with `openat` and `O_NOFOLLOW`. A
  load never creates directories; missing directories or a missing settings
  file leave defaults in use.
- Reject symlinks at the final `ferrum` settings directory on load and save.
  Loading rejects symlinked or non-regular `settings.toml` entries. Saving
  uses `fstatat` with `AT_SYMLINK_NOFOLLOW` to reject an existing symlink or
  non-regular entry before writing. The `renameat` replacement never follows a
  symlink target if the directory entry changes concurrently.
- Apply `0700` only to the final app settings directory. Keep settings files
  and save-time temporary files at `0600`. Create temporary files with
  `openat`, then reset their mode through the opened descriptor so a
  restrictive umask cannot remove owner permissions. Replace settings with
  `renameat` and clean up failed temporary files with `unlinkat`. Keep every
  post-initialization filesystem operation relative to the retained descriptor.
- Preserve the existing schema migration, default values, and rejection of
  unknown fields. Do not persist credentials, secrets, endpoints, or remote
  contexts.

## Alternatives

### Resolve the full path for each operation

Rejected because replacing a selected parent path after initialization could
redirect a later load or save.

### Reject every symlink in the configured path or confine it to `HOME`

Rejected because existing symlink aliases in a user-selected XDG/HOME location
are intentional input. Ferrum trusts that selected location and does not treat
`HOME` as a sandbox boundary.

### Use path-based directory creation and file operations

Rejected because ordinary path traversal could follow a symlink introduced in
the components below the retained anchor. Descriptor-relative traversal with
`O_NOFOLLOW` checks those components as Ferrum opens them.

## Rationale

The retained descriptor fixes the directory from which Ferrum resolves the
remaining settings path. Descriptor-relative operations avoid returning to a
replaceable absolute parent path after initialization. No-follow checks reject
symlink substitutions below that anchor, while honoring aliases that the user
selected before initialization. Limiting `0700` to the final app directory
protects settings without changing permissions on its parent directories.

## Consequences

- A missing settings tree stays absent after load. Save creates only the
  remaining directories below the retained anchor.
- Replacing the selected absolute parent path after initialization cannot
  redirect access away from the pinned descriptor. Symlinks in remaining
  components or at the final settings directory are rejected; symlinked or
  non-regular file entries are rejected before load/save, and a concurrent
  replacement never causes `renameat` to follow a symlink target.
- The chosen XDG/HOME location remains user-trusted and may be outside `HOME`.
  This decision is Linux-only and makes no Windows, macOS, Podman, rootless, or
  remote-engine support claim.
- Temporary-directory regressions cover initialization with a symlink alias,
  parent-path replacement, missing-directory loads, safe save-time creation,
  final-component symlinks, and permissions limited to the final app directory
  and settings files.
- Temporary-file permissions are explicitly reset through the open descriptor,
  so restrictive umasks cannot remove owner read/write access.
- Issues #19 and #10 remain open. This ADR records the path decision and does
  not claim full issue acceptance or a release.

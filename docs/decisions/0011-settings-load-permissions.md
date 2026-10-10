# ADR 0011: Linux settings load permissions and hard-link rejection

- **Status:** Accepted; supplements ADR 0009 and ADR 0010 for load-time directory and file validation. Issues #19 and #10 remain open.
- **Date:** 2026-10-10

## Context

ADR 0009 anchors the Linux settings path to an open directory descriptor, and ADR 0010 defines permissions for newly created remaining-path directories. A settings file can still be a hard link to a different regular file. Changing permissions through the opened file descriptor changes that shared inode, including every name outside the Ferrum settings directory. An existing final `ferrum` directory may also have broader permissions than the settings store requires.

## Decision

- On load, after opening the existing final `ferrum` directory through the retained descriptor and before opening `settings.toml`, set that directory's mode to exactly `0700` through the descriptor. A missing directory remains absent; a failure to set permissions returns a read error without opening the settings file.
- Open `settings.toml` relative to the directory descriptor. Before changing the file's permissions or reading its contents, require that the opened entry is a regular file with exactly one hard link (`nlink == 1`). Reject symlinks, non-regular entries, and entries with any other link count as invalid without changing the file inode's permissions or reading it.
- For an accepted single-link regular file, retain the existing `0600` file-mode policy, bounded parsing, defaults, and versionless migration behavior.
- The link-count check and subsequent file-mode update are separate operations.
  The final `0700` directory limits access by different unprivileged UIDs, but
  same-UID processes or privileged actors can still create a hard link between
  those operations. Protection from actors already able to access the settings
  namespace as the same user or with `CAP_FOWNER` is outside this boundary.
- This ADR supplements ADR 0009 and ADR 0010 only for load-time permissions and file validation. It does not change path anchoring, save-time directory creation, save-time atomic replacement, or the existing save permission policy.

## Alternatives

### Continue changing permissions on every regular file

Rejected because a hard link is regular, but changing its mode affects the shared inode and can alter an unrelated sibling file's permissions.

### Leave the final settings directory permissions unchanged on load

Rejected because a final app directory with broad permissions can remain
enumerable or mutable by other local users; private file mode alone does not
protect its directory-entry namespace.

### Reject all settings files with existing legacy formats

Rejected because single-link versionless settings can be safely read and migrated in memory without weakening the hard-link policy.

## Rationale

The directory mode is normalized through the already-open descriptor before the settings entry is opened. File link count is checked on the opened inode before `fchmod` or `read`, preventing permission changes to an inode also named outside the settings tree. Existing single-link legacy files continue to load without a format break.

## Consequences

- Loading an existing final `ferrum` settings directory changes its mode to `0700`, even when `settings.toml` is absent.
- A settings entry whose link count is not exactly one is rejected and is not read or chmodded; its other hard-link names and contents are left unchanged.
- Accepted single-link regular settings files are normalized to `0600` before parsing, and versionless legacy settings continue to migrate in memory.
- This decision applies only to the Linux settings path and does not claim platform, engine, or release support.

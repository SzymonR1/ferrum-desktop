# ADR 0012: Atomic visible-log export

- **Status:** Accepted
- **Date:** 2026-10-10

## Context

Visible-log export previously opened and truncated the selected target directly. A selected path that was hard-linked elsewhere therefore overwrote both names, and an I/O failure after truncation could leave only partial output. The existing contract creates new files with mode `0600`, preserves ordinary rwx bits on writable existing regular files, and rejects final-component symlinks and non-regular targets.

## Decision

On the existing Linux local-engine slice, export through a unique sibling
temporary file. Open the selected parent directory with `O_PATH` so
descriptor-relative operations do not require directory read permission;
creating, replacing, and cleaning entries still require write and search
permission on the directory. Open an existing target without truncation or
symlink following; it must be a writable regular file. New targets use
owner-only mode `0600`; existing targets retain their ordinary rwx bits.

Write and sync the complete export before atomically renaming the temporary
entry over the selected directory entry. If writing or replacement fails
before the rename commits, leave the previous target unchanged and attempt to
remove the temporary file. Replacing one hard-linked name creates a new inode
at that name; other links continue to refer to the old contents. Symlinks and
non-regular targets are not followed or written.

Atomic replacement requires write and search permission on the containing
directory. The replacement inode is owned by the Ferrum process, and group
ownership follows directory creation policy. Extended ACLs and other xattrs are
not copied. Ordinary mode bits are preserved; setuid/setgid bits are not.
Same-UID processes can still race changes to the selected directory entry;
this is not a general parent-path containment guarantee. The file is synced
before rename, but the containing directory is not synced, so crash durability
is not guaranteed. A process crash may leave a sibling temporary file with the
existing target's ordinary mode bits or with `0600` for a new target.

The export remains limited to currently visible, retained plain-text rows. No engine, endpoint, or platform support is added.

## Alternatives

- **Continue truncating the target in place:** rejected because it modifies all hard-link aliases and can destroy prior contents if a later write fails.
- **Reject only targets with multiple links:** rejected because link-count checks are racy and do not protect existing contents from partial writes.
- **Force mode `0600` for every replacement:** rejected to preserve the established ordinary rwx mode behavior for existing targets; newly created exports remain `0600`.
- **Use a separate filesystem-wide transaction or add a file-export dependency:** rejected as unnecessary for a same-directory replacement and the existing Linux-only scope.

## Rationale

The sibling file is on the same filesystem, allowing `renameat` to publish a complete export atomically without writing through the selected inode. This keeps other hard links and the previous selected contents intact until commit while retaining the documented ordinary permission-bit behavior.

## Consequences

- A writable existing target may now fail to export when its containing directory does not permit temporary-file creation and rename.
- Replacing an existing file creates a new inode and therefore does not retain its extended ACLs or xattrs.
- Returned I/O errors before rename leave the selected target unchanged; temporary-file cleanup is best-effort. A process crash may leave a sibling temporary file with the existing target's ordinary mode bits or with `0600` for a new target.
- Existing final-symlink, non-regular-target, visible-row, and permission tests remain part of the contract.
- This decision does not complete Issue #19 and makes no claim about non-Linux platforms or unsupported engine endpoints.

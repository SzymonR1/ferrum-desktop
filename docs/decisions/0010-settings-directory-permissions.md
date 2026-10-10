# ADR 0010: Linux settings directory permissions

- **Status:** Accepted; supersedes ADR 0009 only for the permission policy for newly created directories in the remaining settings path. Issues #19 and #10 remain open.
- **Date:** 2026-10-09

## Context

ADR 0009 accepted descriptor-anchored Linux settings paths and the existing
permission policy for the final app settings directory and settings files. Saving
may also create missing directory components below the retained descriptor. A
restrictive umask filters the mode requested by `mkdirat`, so the permission
policy for those newly created components needs to be explicit without
rewriting the accepted ADR 0009 record.

## Decision

- This ADR supersedes ADR 0009 only for the permission policy for newly created
  directories in the remaining settings path. All other decisions in ADR 0009
  remain in force.
- When saving, request mode `0700` from `mkdirat` for each missing remaining
  path component. If `mkdirat` creates that component, open it relative to the
  retained directory descriptor using `openat` with `O_DIRECTORY` and
  `O_NOFOLLOW`, then set its mode to exactly `0700` through the opened
  descriptor before continuing traversal.
- Normalize a component under this policy only when `mkdirat` reports that it
  created the component. If the entry already exists, open it without changing
  its mode. Existing ancestor directory modes therefore remain unchanged. The
  pre-existing ADR 0009 rule that sets the final app `ferrum` settings directory
  to `0700` on save remains in force; it is separate from normalization of new
  components during traversal.
- The kernel applies the process umask to the `mkdirat` mode. Under umask
  `0200`, the initial mode is `0500`; owner-read remains available, so Ferrum
  can open the directory and restore `0700` through its descriptor. If the
  umask removes owner-read permission required for the `O_RDONLY` directory
  open, opening fails and saving returns an error before creating the temporary
  settings file or writing settings. Ferrum does not weaken the mode policy or
  fall back to path-based permission changes.
- `mkdirat` and the subsequent `openat` are separate operations. An actor with
  write and search permission on the parent directory may be able to remove the
  just-created entry and replace it with a real directory in that interval,
  subject to sticky-directory rules. `O_NOFOLLOW` rejects a symlink replacement
  but does not prove that an opened real directory is the inode created by
  `mkdirat`. If the replacement cannot be opened, saving fails. If it can be
  opened, normalization acts on the inode held by that descriptor. A process
  without ownership or `CAP_FOWNER` cannot change another owner's directory
  mode; `fchmod` fails with `EPERM` and saving stops during directory traversal,
  before temporary-file creation or settings writes. A process that owns the
  replacement or has `CAP_FOWNER` (including a privileged/root process) can
  change its mode. No interprocess lock is used.

## Alternatives

### Rely on the process umask

Rejected because a restrictive umask can remove required owner permissions from
new settings directories.

### Normalize every directory encountered during traversal

Rejected because it would change pre-existing ancestor permissions outside the
final app settings directory, contrary to the accepted path policy.

### Reopen or chmod the path by name after creation

Rejected because a path-based operation could follow a replacement entry and
would no longer be bound to the opened directory descriptor.

## Rationale

Normalizing only directories this process created repairs umask-filtered modes
without changing existing ancestors. Applying the mode through a no-follow
opened descriptor keeps the operation anchored to an opened inode and refuses
symlink substitutions. The residual real-directory replacement race is bounded
by descriptor permissions: an ordinary unprivileged process fails closed when
the opened replacement belongs to another owner, while privileged execution
can affect that inode and must be treated accordingly.

## Consequences

- Newly created remaining-path directories are exactly `0700` after successful
  traversal, including intermediate XDG components. Existing ancestor modes are
  preserved; the separate final app directory rule from ADR 0009 remains in
  effect.
- A restrictive umask that removes the owner-read bit needed for opening a new
  directory can make save fail safely before writing settings.
- A concurrent real-directory replacement can cause a safe save failure for an
  unprivileged non-owner, or be normalized when the process owns the replacement
  or has `CAP_FOWNER`. Symlink replacements remain rejected by `O_NOFOLLOW`.
- The settings path remains Linux-only. This ADR makes no Windows, macOS,
  Podman, rootless, or remote-engine support claim. Issues #19 and #10 remain
  open; this ADR does not claim full issue acceptance or a release.

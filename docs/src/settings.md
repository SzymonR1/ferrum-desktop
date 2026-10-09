# Settings

Ferrum stores application preferences on Linux at `$XDG_CONFIG_HOME/ferrum/settings.toml` when `XDG_CONFIG_HOME` is an absolute path. Otherwise, it uses `$HOME/.config/ferrum/settings.toml`. If neither location can be determined, Ferrum remains usable with defaults and reports that settings cannot be saved.

The only preference is `auto_connect_on_startup`, which defaults to `true`. When disabled, startup remains disconnected with an explanation, and the user can still connect manually with **Retry connection**. The setting takes effect at the next startup. Changes are written only after explicitly selecting **Save**; **Cancel** discards edits.

Missing files use defaults. Versionless legacy settings are migrated in memory
while preserving recognized values. Invalid, oversized, or future-version files
fall back to defaults and produce a safe warning; Ferrum does not overwrite the
file until the user explicitly saves.

## Linux settings path

The accepted Linux settings-path contract works as follows.

At initialization, `SettingsStore` chooses the absolute `XDG_CONFIG_HOME` path
when available, or falls back to the absolute `HOME` path. It pins a file
descriptor for the longest existing directory before the final `ferrum`
settings directory. Symlink aliases already present in the user-selected
XDG/HOME path at initialization are honored.

Ferrum traverses the remaining directory components relative to that descriptor
with `openat` and `O_NOFOLLOW`. It creates missing components with `mkdirat`
only when saving. A load never creates directories, and a missing directory or
settings file leaves the defaults in use. The final `ferrum` directory rejects
symlinks on load and save. Loads reject symlinked or non-regular `settings.toml`
entries before reading or changing their permissions. Saves reject an existing
symlink or non-regular settings-file entry before writing; `renameat` never
follows a destination symlink. Opened settings files are verified to be regular
files before permissions are changed.
Only the final `ferrum` directory receives owner-only (`0700`) permissions.
Settings files and save-time temporary files use owner-only (`0600`) permissions.
Save creates the temporary file with `openat`, atomically replaces the settings
file with `renameat`, and removes a failed temporary file with `unlinkat`.
All filesystem operations after initialization use the retained descriptor.
Replacing the selected absolute parent path after initialization cannot
redirect settings access away from the pinned directory.

The configured XDG/HOME location remains user-trusted. Ferrum honors its
existing symlink aliases and does not confine it to `HOME`. All suffix
components are opened relative to the pinned descriptor; an entry changed
concurrently cannot redirect `renameat` to a symlink target.
Ferrum does not persist credentials, secrets, endpoints, or remote contexts. Credentials and remote connection contexts are not supported by this Linux-only settings slice.

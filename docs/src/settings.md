# Settings

Ferrum stores application preferences on Linux at `$XDG_CONFIG_HOME/ferrum/settings.toml` when `XDG_CONFIG_HOME` is an absolute path. Otherwise, it uses `$HOME/.config/ferrum/settings.toml`. If neither location can be determined, Ferrum remains usable with defaults and reports that settings cannot be saved.

The only preference is `auto_connect_on_startup`, which defaults to `true`. When disabled, startup remains disconnected with an explanation, and the user can still connect manually with **Retry connection**. The setting takes effect at the next startup. Changes are written only after explicitly selecting **Save**; **Cancel** discards edits.

Missing files use defaults. Versionless legacy settings are migrated in memory
while preserving recognized values. Invalid, oversized, or future-version files
fall back to defaults and produce a safe warning; Ferrum does not overwrite the
file until the user explicitly saves. The final settings-path component is
opened without following symlinks, and Ferrum verifies the opened descriptor is
a regular file before changing its permissions. Non-regular settings files are
rejected. Parent path components follow the selected XDG/HOME location.

The final settings directory is opened without following a symlink before
Ferrum applies owner-only (`0700`) permissions; a symlinked final directory is
rejected. Existing settings files are restricted to owner-only (`0600`) when
loaded. Save creates the private temporary file with `openat`, atomically
replaces the target with `renameat`, and removes failed temporary files with
`unlinkat`, all relative to the opened directory descriptor. Replacing the
final directory path after it is opened cannot redirect these save operations.

No-follow applies to the final settings-file and settings-directory components.
Configured XDG/HOME parent components are resolved before opening the directory
and are not descriptor-contained.

Ferrum does not persist credentials, secrets, endpoints, or remote contexts. Credentials and remote connection contexts are not supported by this Linux-only settings slice.

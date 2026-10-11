# Embedded terminal

**Status:** Experimental; Linux local Docker-compatible Unix sockets only.

## Prerequisites

Ferrum must be connected to a Linux local Docker-compatible Unix socket, and the selected container must be Running. The container must contain the chosen shell: `/bin/sh` is the default and `/bin/bash` is also offered. Ferrum does not install a shell or automatically fall back if a selected shell is missing.

## Opening and using a session

Open **Container details**, choose a shell, and select **Open terminal**. Click the terminal surface or **Focus terminal** before typing. Ferrum starts a container exec session with a TTY, so stdout and stderr are combined. Resizing the terminal view updates the TTY dimensions.

- Type to send input; Enter, Tab, Backspace, Escape, arrows, Home/End, and Delete are translated to terminal input. `Ctrl+C` sends ETX (`0x03`) to the TTY, which interrupts the foreground process when the terminal line discipline enables that behavior.
- `Ctrl+Shift+C` copies terminal text. Use **Paste from clipboard** to request a paste explicitly; there is no keyboard paste shortcut. Clipboard delivery is accepted only while the terminal is focused and for up to three seconds after the request. When the terminal enables bracketed paste, Ferrum wraps pasted bytes accordingly. A single input or paste payload is limited to 64 KiB.
- While the terminal is focused, Page Up/Page Down and mouse-wheel input navigate scrollback in the normal screen. Page Up/Page Down are passed to alternate-screen applications; mouse-wheel input is not forwarded.
- Scrollback is bounded to 10,000 rows.

## Safety and limitations

Terminal output is untrusted and may include sensitive or deceptive text. OSC
52 clipboard writes and terminal-title requests are ignored. Ferrum caps raw
OSC strings at 1 KiB before parsing. For an oversized string, the parser retains
only the capped prefix while Ferrum discards the tail; the actual BEL, CAN, SUB,
or ESC terminator is passed through before rendering resumes.
Ferrum never copies output to or reads clipboard contents
automatically; clipboard operations require explicit user action. When exec
creation fails, Ferrum displays a typed error without the daemon response body.
Terminal bytes and engine response bodies are not included in diagnostics.

Closing the terminal, switching the engine connection, or exiting Ferrum ends the exec session only; it does not stop or remove the container. The terminal feature is experimental and has not been visually or screen-reader validated.

The terminal view is available only through the selected local Unix socket on Linux. Remote endpoints, Podman/rootless engines, Windows, and macOS are not supported by this slice; issue [#10](https://github.com/SzymonR1/ferrum-desktop/issues/10) remains open for Podman/rootless validation and the broader engine matrix. See the [platform support matrix](../platform-support.md).

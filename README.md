# OmniConsole

A cross-platform terminal for network engineers: SSH, Telnet and serial console in tabs in one window, with saved devices.

Runs on Windows (x64, ARM64), macOS (Apple Silicon, Intel) and Linux (x64, ARM64).

## Features

- SSH with password and keyboard-interactive login, plus legacy algorithms for older Cisco gear (diffie-hellman-group1/14-sha1, ssh-rsa, AES-CBC, 3DES)
- Telnet (handles option negotiation and window size) and serial console (port picker, baud rate)
- Every session opens as a tab in one window
- Saved devices grouped by site, with search
- Passwords live in the OS credential store (Windows Credential Manager, macOS Keychain, Linux Secret Service), never in files
- Host keys are trusted on first use, with a warning if one changes
- Quick connect bar: `ssh admin@10.0.0.1`, `telnet 10.0.0.5 2001`, `COM3 9600`, `/dev/ttyUSB0`
- Copy on select, right-click paste, a confirmation before multi-line pastes, find in scrollback, save output to a text file
- Dark and light themes

Planned: read-only Cisco diagnostics collection and an offline AI analyzer that recommends fixes but never changes anything.

## Shortcuts

| Keys | Action |
| --- | --- |
| Ctrl/Cmd+Shift+T | New tab (quick connect) |
| Ctrl/Cmd+Shift+W | Close tab |
| Ctrl+Tab | Next tab |
| Ctrl/Cmd+Shift+B | Toggle sidebar |
| Ctrl/Cmd+Shift+F | Find |
| Ctrl/Cmd+Shift+C / V | Copy / paste |
| Ctrl/Cmd+Shift+S | Save output |
| Ctrl/Cmd + / - / 0 | Font size |

## Build

You need Node 22+, Rust stable and the [Tauri prerequisites](https://v2.tauri.app/start/prerequisites/). On Linux, also install `libudev-dev`.

```sh
npm ci
npm run tauri dev     # run in development
npm run tauri build   # build installers
```

Every push builds installers for all six platforms through GitHub Actions. Download them from the run's Artifacts.

## Stack

Tauri 2 (Rust) backend with russh, serialport and keyring; React and xterm.js frontend.

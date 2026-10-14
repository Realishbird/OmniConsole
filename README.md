# OmniConsole

A terminal for network engineers. SSH, Telnet and serial consoles in tabs, saved devices, health checks, config backups, an AI assistant that proposes commands you review before they run, and reviewed deployments with backup and rollback. One app on Windows (x64, ARM64), macOS (Apple Silicon, Intel) and Linux (x64, ARM64).

Nothing changes a device without you: read-only commands run on approval, configuration changes need a second confirmation, a backup is taken first, and every change is recorded.

## Install

Every push builds installers for all six platforms. Open the latest run under [Actions](https://github.com/Realishbird/OmniConsole/actions), scroll to **Artifacts** and download `OmniConsole-<platform>`:

| Platform | Artifact | Installer |
| --- | --- | --- |
| Windows x64 | `OmniConsole-windows-x64` | `.msi` or `.exe` |
| Windows ARM64 | `OmniConsole-windows-arm64` | `.exe` |
| macOS Apple Silicon | `OmniConsole-macos-arm64` | `.dmg` |
| macOS Intel | `OmniConsole-macos-x64` | `.dmg` |
| Linux x64 | `OmniConsole-linux-x64` | `.deb`, `.rpm` or `.AppImage` |
| Linux ARM64 | `OmniConsole-linux-arm64` | `.deb`, `.rpm` or `.AppImage` |

The builds are unsigned. Windows SmartScreen and macOS Gatekeeper will ask once; on macOS use right-click → Open the first time.

## Getting started

1. **Add a device** (sidebar `+`, or the palette) with its protocol, address, login and where it lives: customer, site, building, rack, role, environment, tags.
2. **Connect**: double-click it, or type in quick connect: `ssh admin@10.0.0.1`, `telnet 10.0.0.5 2001`, `COM3 9600`, `rdp 10.0.0.9`.
3. **Open the tools panel** (`Ctrl/Cmd+Shift+I`) next to the terminal. Each console tab keeps its own checks, chat and progress.
4. **Run health checks**, ask the assistant, back up the config, or deploy a template. Anything that changes the device shows you the exact lines first.

Passwords and key passphrases go in the operating system's credential store (Windows Credential Manager, macOS Keychain, Linux Secret Service). They are never written to files, logs, exports or AI requests.

## What it does

### Connections
- **SSH** with password, key file (with passphrase) or agent (ssh-agent, OpenSSH agent for Windows, Pageant). Legacy algorithms for old Cisco gear: diffie-hellman-group1/14-sha1, ssh-rsa, AES-CBC, 3DES. Jump hosts through another saved device. Host keys are trusted on first use, with a loud warning if one changes.
- **Telnet** with option negotiation, window size and BREAK. Warns before connecting to a production device.
- **Serial** console with port picker, baud, data bits, parity, stop bits, flow control, and a break signal for ROMMON.
- **RDP and VNC** devices in the same list; they open in the system client (Remote Desktop, FreeRDP, Remmina, Screen Sharing, TigerVNC).
- Automatic login for Telnet and serial at the Username/Password prompts, keepalives, automatic reconnect per device, a reconnect banner when a session drops.
- Tabs in one window, split right or down, more windows when you want them (`Ctrl/Cmd+Shift+N`, right-click a tab, or "Connect in a new window"), and named **layouts** that reopen a set of devices and the split.

### Terminal
- Command palette (`Ctrl/Cmd+Shift+P`): actions, devices, snippets, bookmarks, command history.
- **Ctrl+Space completion**: asks the device with its own `?`, shows the options with their help text, and restores what you typed.
- Highlighting of IP and MAC addresses, interface names, syslog severities, errors, up/down states.
- Session logs under `Documents/OmniConsole/Logs/<date>/`, optional timestamps, with passwords, secrets, SNMP communities and keys masked.
- Copy on select, right-click paste, confirmation before multi-line pastes, find in scrollback, bookmarks, save output or a selection to a file, searchable history, snippets.
- Broadcast a command to selected connected sessions (`Ctrl/Cmd+Shift+L`) behind a red bar, explicit targets and a confirmation that names production devices.
- Six themes (Midnight, Graphite, Nord, Dracula, Solarized Dark and Light), adjustable font, size, spacing, cursor, contrast and ANSI colors.

### Saved devices
- Grouped by customer, site, building, rack, role, environment or tag; search any field, or filter with `role:core`, `tag:stack`, `env:production`.
- Favorites, recents, import (OmniConsole JSON or CSV), export (never includes passwords).
- Production, staging, lab and customer environments color the tab and the status bar, which also shows the protocol, the host name from the prompt and the privilege level.

### Health checks (Tools → Checks)
- Detects the platform from `show version` (IOS, IOS-XE, NX-OS, IOS-XR, ASA, Small Business SG/CBS, CatOS) and runs the right read-only commands with paging off.
- 20 checks graded pass / warning / fail / unknown, each with the output lines behind the verdict and the follow-up commands: system and uptime, CPU, memory, temperature, power, fans, stack, redundancy, interface errors, speed/duplex/err-disabled, PoE, VLANs and trunks, port channels, spanning tree, routing neighbors, recent logs, CDP/LLDP neighbors, IP interfaces, MAC and ARP tables.
- Parsed tables with a filter, a choice of which checks run, repeat every 5/15/60 minutes, cancel at any time.
- Save a run as a baseline and compare later runs; save a diagnostic bundle (zip of summary, facts, results and raw output) with secrets masked.

### Config backups (Tools → Configs)
- Running and startup config to `Documents/OmniConsole/Configs/<site>/<device>/<timestamp>_running.cfg`.
- Each new backup is diffed against the previous one; diff any two, or a backup against the live config, ignoring timestamps and certificate blocks.
- Search the newest backup of every device (or every version), plain text or regex.

### Assistant (Tools → Assistant, `Ctrl/Cmd+Shift+A`)
- Providers: Ollama or LM Studio on this computer (Qwen3 8B suggested), OpenAI, Anthropic, or any OpenAI-compatible endpoint. API keys live in the keychain.
- Explain selected output, suggest next steps from the latest health check, interpret diagnostics, generate configuration, all tailored to the detected OS and version. Answers separate what the output shows from what the model infers.
- Proposed commands appear as cards classified **read-only**, **configuration change** or **disruptive**. Read-only cards run on approval; changes need a review checkbox and a second confirmation, take a backup first and write an audit entry; disruptive lines need the device name typed; production devices stay locked until unlocked for that tab.
- After a card runs, the assistant reads the result on its own: a rejected command comes back corrected, good output is interpreted, up to three rounds.
- Secrets are replaced with `<removed>` before anything leaves the app. Sharing of output, configuration text and device names can each be turned off.

### Guided troubleshooting (Tools → Workflows)
Step-by-step read-only workflows: trace a device by IP or MAC to its port and the neighbor behind it, DHCP, DNS, VLAN, trunking, spanning tree, routing, ACL, wireless, authentication. Each step says what to look for and can be handed to the assistant.

### Deploy (Tools → Deploy)
- **Templates** with typed, validated variables, rendered for IOS or NX-OS: access, access point, camera, voice, trunk, uplink and port-channel ports, VLAN with SVI, and initial setup of a new switch (hostname, management SVI and VRF, gateway, DNS, NTP, SSH, local admin, logging, SNMPv3, banner).
- **Reviewed deployment**: the exact lines, a running-config backup, a rollback point on the device (`copy running-config flash:` with `configure replace`, or an NX-OS checkpoint), an optional `reload in N` safety net, line-by-line sending with progress that stops at the first rejected line, verify commands, then save, roll back or cancel the reload.
- **Firmware assistant**: reads version, image, boot variable, free flash and stack members, checks the new image's family, space and checksum, and proposes transfer, MD5 verification, activation (`boot system`, `install add … activate commit`, or NX-OS `install all`) and post-upgrade validation as separate reviewed steps. The reload is always explicit.
- **Checklists** for installation, cutover and upgrade, with a result and note per item, saved per device and exportable.
- **Audit** of every configuration change sent: lines, result, backup path.

### Files (Tools → Files)
An SFTP browser over a second SSH connection with the same login: list, download, upload, rename, delete, new folder, with progress. Devices without SFTP (classic IOS with `ip scp server enable`) fall back to SCP by path, so `flash:` images and configs still move both ways.

### Workstation (Tools → Workstation)
Ping, traceroute, DNS lookup (any record type, optional server), TCP port check, path MTU discovery and a subnet calculator, run from this computer rather than the device.

## Shortcuts

| Keys | Action |
| --- | --- |
| Ctrl/Cmd+Shift+P or Ctrl/Cmd+K | Command palette |
| Ctrl/Cmd+Shift+T | Quick connect |
| Ctrl/Cmd+Shift+W | Close tab |
| Ctrl+Tab, Alt+1…9 | Switch tab |
| Ctrl/Cmd+Shift+D / E | Split right / down (again to unsplit); F6 switches panes |
| Ctrl/Cmd+Shift+N | New window |
| Ctrl/Cmd+Shift+B | Toggle sidebar |
| Ctrl/Cmd+Shift+I | Tools panel |
| Ctrl/Cmd+Shift+A | Assistant |
| Ctrl+Space | Complete the command with the device's `?` help |
| Ctrl/Cmd+Shift+L | Broadcast to sessions |
| Ctrl/Cmd+Shift+F | Find in output |
| Ctrl/Cmd+Shift+H | Command history |
| Ctrl/Cmd+Shift+O | Snippets |
| Ctrl/Cmd+Shift+M | Bookmark the current line |
| Ctrl/Cmd+Shift+R | Reconnect |
| Ctrl/Cmd+Shift+C / V | Copy / paste |
| Ctrl/Cmd+Shift+S | Save output |
| Ctrl/Cmd+, | Settings |
| Ctrl/Cmd + / - / 0 | Font size |

## Where things are stored

| What | Where |
| --- | --- |
| Devices, snippets, history, settings, layouts, checklists, audit | the app's config folder (`%APPDATA%\OmniConsole`, `~/Library/Application Support/OmniConsole`, `~/.config/OmniConsole`) |
| Passwords, passphrases, API keys | the OS credential store |
| Session logs | `Documents/OmniConsole/Logs/<date>/` |
| Config backups, reports, bundles | `Documents/OmniConsole/Configs/` (changeable in Settings) |

## Build from source

Node 22+, Rust stable and the [Tauri prerequisites](https://v2.tauri.app/start/prerequisites/). On Linux also `libudev-dev` and `libdbus-1-dev`.

```sh
npm ci
npm run tauri dev     # run in development
npm run tauri build   # build the installer for this machine
cd src-tauri && cargo test   # backend tests (parsers, checks, templates, SCP/SFTP helpers)
```

## How it is built

- **Backend**: Rust with Tauri 2. `session/` holds the SSH (russh), Telnet and serial (serialport) sessions plus the capture-based command runner; `cisco/` the OS detection, parsers, 20 checks, config diffing, command classification, templates and firmware planning; `ai.rs` the providers; `deploy.rs` the reviewed deployment; `files.rs` SFTP and SCP; `nettools.rs` the workstation tools; `store.rs` devices and the keyring.
- **Frontend**: React, TypeScript and xterm.js. `App.tsx` owns tabs, panes and shortcuts; `components/ToolsPanel.tsx` the tools sections, each keeping state per console tab.
- **CI**: `.github/workflows/build.yml` builds all six platforms and runs the backend tests.

## Roadmap

Everything above is in. Ideas for later: SNMP and NetBox lookups, an inventory view across devices, session recording playback, scheduled backups.

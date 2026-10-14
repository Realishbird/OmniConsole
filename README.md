# OmniConsole

A cross-platform terminal for network engineers: SSH, Telnet and serial console in tabs in one window, with saved devices.

Runs on Windows (x64, ARM64), macOS (Apple Silicon, Intel) and Linux (x64, ARM64).

## Features

**Connections**
- SSH with password, key file (with passphrase) or SSH agent login (ssh-agent, OpenSSH agent for Windows, Pageant), plus legacy algorithms for older Cisco gear (diffie-hellman-group1/14-sha1, ssh-rsa, AES-CBC, 3DES)
- Jump hosts: connect through another saved SSH device
- Telnet with option negotiation, window size and BREAK
- Serial console with port picker, baud rate, data bits, parity, stop bits, flow control and a break signal for ROMMON
- Every session opens as a tab in one window; split the window right or down to watch two sessions
- Keepalives, automatic reconnect per device, a reconnect banner when a session drops

**Saved devices**
- Organized by customer, site, building, rack, device role, environment and tags; group the sidebar by any of them
- Search every field, or filter with `role:core`, `tag:stack`, `env:production`
- Favorites, recent sessions, import (OmniConsole JSON or CSV) and export (never includes passwords)
- Production, staging, lab and customer environments color the tab and the status bar; the status bar also shows the protocol, the host name from the prompt, and the privilege level (user EXEC, privileged, config mode)

**Terminal**
- Command palette (Ctrl/Cmd+Shift+P): actions, devices, snippets, bookmarks and command history
- Searchable command history, reusable snippets, output bookmarks
- Optional highlighting of IP and MAC addresses, interface names, syslog severities, errors and up/down states
- Session logging to `Documents/OmniConsole/Logs/<date>/`, with optional timestamps; passwords, secrets, SNMP communities and keys are masked
- Copy on select, right-click paste, confirmation before multi-line pastes, find in scrollback, save output or a selection to a file

**Health checks** (Tools panel, Ctrl/Cmd+Shift+I)
- Detects the platform from `show version` (IOS, IOS-XE, NX-OS, IOS-XR, ASA, Small Business SG/CBS, CatOS) and runs the right read-only `show` commands, with paging turned off for the session
- 20 checks graded pass / warning / fail / unknown with the output lines behind each verdict and the follow-up commands to run next: system and uptime, CPU, memory, temperature, power supplies, fans, stack, redundancy, interface errors and discards, speed/duplex/err-disabled, PoE budget, VLANs and trunks, port channels, spanning tree, OSPF/BGP/EIGRP neighbors, recent log messages, CDP/LLDP neighbors, IP interfaces, MAC and ARP tables
- Parsed tables with a filter box, a selection of which checks run, repeat every 5/15/60 minutes while the tab is open, cancel at any time
- Save a run as a baseline and compare later runs with it; save a diagnostic bundle (zip of the summary, facts, results and every raw output) with secrets masked

**Config backups**
- Back up the running and startup config to `Documents/OmniConsole/Configs/<site>/<device>/<timestamp>_running.cfg`
- Every new running-config backup is diffed against the previous one; diff any two backups, or a backup against the live running config, with timestamps and certificate blocks ignored
- Search across the newest backup of every device (or every version), plain text or regular expression
- Run any command and capture its output while it still shows in the terminal; the pager is answered for you

**Assistant** (Tools panel → Assistant, Ctrl/Cmd+Shift+A)
- Providers: Ollama and LM Studio on this computer (suggested model Qwen3 8B), OpenAI, Anthropic, or any OpenAI-compatible endpoint; API keys live in the OS keychain
- Explain selected output, suggest next steps from the latest health check, interpret diagnostics, generate configuration, all tailored to the detected OS and version
- Answers separate what the output verifies from what the model infers, and ask for the missing show command instead of guessing
- Every proposed command appears as a card classified read-only, configuration change or disruptive. Read-only commands run on approval; configuration changes need a review checkbox and a second confirmation, take a running-config backup first and write an audit entry; disruptive commands also need the device name typed; production devices stay locked until unlocked for the tab
- Passwords, secrets, SNMP communities, keys and certificate bodies are replaced with `<removed>` before anything leaves the app; sharing of output, configuration text and device names can each be turned off

**Workstation tools** (run from this computer, not the device)
- Ping, traceroute, DNS lookup (any record type, optional server), TCP port check, path MTU discovery, subnet calculator with splitting

**Guided troubleshooting**
- Step-by-step read-only workflows for tracing a device by IP/MAC to its port (and the neighbor behind it), DHCP, DNS, VLAN, trunking, spanning tree, routing, ACL, wireless and authentication; each step says what to look for and can be handed to the assistant

**Security**
- Passwords and key passphrases live in the OS credential store (Windows Credential Manager, macOS Keychain, Linux Secret Service), never in files or exports
- Host keys are trusted on first use, with a loud warning if one changes
- A warning before Telnet to a production device, and before sending snippets to one

**Themes**: Midnight, Graphite, Nord, Dracula, Solarized Dark and Light. Fonts, size, spacing, cursor, contrast and the 16 ANSI colors are adjustable; compact and comfortable layouts.

Planned next: templates and reviewed deployment with rollback, firmware upgrades, broadcast to several sessions, an audit trail viewer, SFTP/SCP.

## Shortcuts

| Keys | Action |
| --- | --- |
| Ctrl/Cmd+Shift+P or Ctrl/Cmd+K | Command palette |
| Ctrl/Cmd+Shift+T | Quick connect |
| Ctrl/Cmd+Shift+W | Close tab |
| Ctrl+Tab, Alt+1…9 | Switch tab |
| Ctrl/Cmd+Shift+D / E | Split right / down (again to unsplit); F6 switches panes |
| Ctrl/Cmd+Shift+B | Toggle sidebar |
| Ctrl/Cmd+Shift+I | Tools panel: checks, workflows, configs, workstation tools |
| Ctrl/Cmd+Shift+A | Assistant |
| Ctrl/Cmd+Shift+F | Find in output |
| Ctrl/Cmd+Shift+H | Command history |
| Ctrl/Cmd+Shift+O | Snippets |
| Ctrl/Cmd+Shift+M | Bookmark the current line |
| Ctrl/Cmd+Shift+R | Reconnect |
| Ctrl/Cmd+Shift+C / V | Copy / paste |
| Ctrl/Cmd+Shift+S | Save output |
| Ctrl/Cmd+, | Settings |
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

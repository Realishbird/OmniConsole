# OmniConsole

A terminal for network engineers. SSH, Telnet and serial consoles in tabs, saved devices, health checks, config backups, an AI assistant that proposes commands you review before they run, and reviewed deployments with backup and rollback. One app on Windows (x64, ARM64), macOS (Apple Silicon, Intel) and Linux (x64, ARM64).

Nothing changes a device without you: read-only commands run on approval, configuration changes need a second confirmation, a backup is taken first, and every change is recorded.

## Install

Every push builds installers for all six platforms. Open the latest run under [Actions](https://github.com/Realishbird/OmniConsole/actions), scroll to **Artifacts** and download `OmniConsole-<version>-<platform>`. The file inside is named the same way, so `OmniConsole-0.9.0-windows-x64.msi` is version 0.9.0 for 64-bit Windows. The version goes up with every batch (0.9.0 = batch 9).

| Platform | Artifact | Installer |
| --- | --- | --- |
| Windows x64 | `OmniConsole-<version>-windows-x64` | `.msi` or `.exe` |
| Windows ARM64 | `OmniConsole-<version>-windows-arm64` | `.exe` |
| macOS Apple Silicon | `OmniConsole-<version>-macos-arm64` | `.dmg` |
| macOS Intel | `OmniConsole-<version>-macos-x64` | `.dmg` |
| Linux x64 | `OmniConsole-<version>-linux-x64` | `.deb`, `.rpm` or `.AppImage` |
| Linux ARM64 | `OmniConsole-<version>-linux-arm64` | `.deb`, `.rpm` or `.AppImage` |

The builds are not notarized with Apple or signed with a Windows certificate, so each operating system asks once before the first launch.

**macOS.** The app is ad-hoc signed, so it is not "damaged", but Gatekeeper still blocks apps that Apple has not notarized. Drag OmniConsole to Applications, open it once (it will be refused), then go to System Settings → Privacy & Security, scroll down and click **Open Anyway**. Or clear the download quarantine flag from a terminal:

```sh
xattr -dr com.apple.quarantine /Applications/OmniConsole.app
```

If you have an Apple Developer account, the build signs and notarizes the app itself when the repository has these secrets: `APPLE_CERTIFICATE` (base64 of the Developer ID Application .p12), `APPLE_CERTIFICATE_PASSWORD`, `APPLE_SIGNING_IDENTITY`, `APPLE_ID`, `APPLE_PASSWORD` (an app-specific password) and `APPLE_TEAM_ID`. Notarized builds open without any prompt.

**Windows.** SmartScreen shows "Windows protected your PC"; choose More info → Run anyway.

## Linux switches and sudo

Health checks and collections on Cumulus Linux and SONiC run their privileged commands as `sudo -n`. If your login has no passwordless sudo, the command is retried without sudo, and only when that also fails does the check say it needs root. Nothing ever answers a password prompt on your behalf: a command that asks for one is aborted (Ctrl-C) and reported, so the next command is never typed into a password field. To get the full set of checks, give the account a `NOPASSWD` sudoers rule or use the built-in `cumulus` user.

## Cisco ISE

ISE's ADE-OS CLI has no `terminal length`, so every ISE probe carries `| nomore`; logs come from `show logging system messages tail count N`. A probe that ISE answers with `syntax error` is reported as rejected rather than parsed as output.

## Getting started

1. **Add a device** (sidebar `+`, or the palette) with its protocol, address, login and where it lives: customer, site, building, rack, role, environment, tags.
2. **Connect**: double-click it, or type in quick connect: `ssh admin@10.0.0.1`, `telnet 10.0.0.5 2001`, `COM3 9600`, `rdp 10.0.0.9`.
3. **Open the tools panel** (`Ctrl/Cmd+Shift+I`) next to the terminal. Each console tab keeps its own checks, chat and progress.
4. **Run health checks**, ask the assistant, back up the config, or deploy a template. Anything that changes the device shows you the exact lines first.

Passwords and key passphrases go in the operating system's credential store (Windows Credential Manager, macOS Keychain, Linux Secret Service). They are never written to files, logs, exports or AI requests.

### Credential locker (Ctrl/Cmd+Shift+K, or Devices menu → Credentials)
- **Profiles**: a named username + password (or key file + passphrase, or SSH agent), optional domain, enable secret, tags, notes and the time of the last change. Create, search, duplicate (without secrets), delete; a device picks a profile in its Login tab, so changing the password once updates every device that uses it. Profiles can be **temporary**: the secrets are deleted when they expire.
- **Defaults per site and customer**: a device without a profile of its own uses its site's default, then its customer's; "This device only" keeps a device on its own saved password. The Login tab says which applies, and each profile shows the devices that use it. **Test login** tries exactly that profile against a saved SSH device and disconnects.
- **Protection**: a vault passphrase (argon2id verifier in the OS store) locks the app out of every stored secret until unlocked; auto-lock after idle time; growing delays after wrong passphrases; masked reveal (15 s) and copy with a timed clipboard clear; a password-prompt limit per connection, and a rejected stored password is never retried with another profile. Jump hosts use their own credentials. Keyboard-interactive/MFA and host-key prompts stay interactive.
- **Encrypted export/import** of profiles with their secrets (AES-256-GCM, key from a separate export passphrase); the device export still contains no secrets.

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

### Platforms
- Detects the platform from `show version` (and a shell probe for Linux-based switches): Cisco IOS, IOS-XE, NX-OS, IOS-XR, ASA, Small Business SG/CBS, CatOS, **Aruba AOS-S**, **Aruba AOS-CX**, **Juniper Junos**, **Arista EOS**, **SONiC (Cisco S1)** and **NVIDIA Cumulus Linux** (NVUE, with NCLU and Linux/FRR fallbacks). Each has its own command definitions, parsers, pagination, privilege and configuration workflow; AOS-S and AOS-CX are kept apart.
- **Cisco ISE** (ADE-OS CLI) and the wireless controllers **Catalyst 9800** (IOS-XE with the controller role) and **AireOS** (kept as its own profile, detected through `show sysinfo`) have their own profiles, collections, troubleshooting sets and checks.
- When detection is inconclusive, pick the platform by hand in Checks or Collect; the choice sticks for that tab.

### Health checks (Tools → Checks)
- **Quick** (system, CPU, memory, temperature, power, fans, stack, redundancy, interface errors, logs, time sync, storage, uplinks), **Detailed** (everything the platform supports, 25 checks: plus speed/duplex, PoE, VLANs, port channels, spanning tree, routing neighbors, CDP/LLDP, IP interfaces, MAC and ARP tables, optics, VXLAN/EVPN) and **Pre-change / Post-change**: keep a run as the snapshot, run again after the change and see every status that moved.
- Every result has a status, the output lines behind it, the collection time, a next step and the follow-up commands. Statuses: **Healthy, Warning, Critical, Not applicable, Unsupported, Not collected**. A command that failed or output that could not be read is Not collected, never healthy.
- Finds cabling loops: a port in the spanning-tree Backup role means the switch hears its own BPDUs, so the check goes critical and names both ports; many topology changes with a recent one, or two ports changing state at the same timestamps in the log, are called out too.
- **ISE**: application services per persona (core services down is critical), version/uptime, CPU, memory, disks, interfaces, routing, DNS/NTP, backup history, system log. The **ISE API checks** form (Checks panel, ISE only) adds what the CLI cannot see through the OpenAPI and ERS: deployment nodes and sync state, system certificates (warning inside 30 days, critical when expired), Active Directory join points, smart licensing and the live session count. The API password is saved in the OS keychain under the host name and never leaves the machine except to that ISE node.
- **Wireless controllers** (9800 and AireOS): access points joined against an **Expected APs** count you enter, AP join failures, AP image versions after an upgrade, WLANs and tags (open WLANs flagged), clients and authentication-related deletes, RF channel utilization and RRM state, plus HA/SSO, resources and logs. Pre-change / Post-change shows the deltas after maintenance.
- Parsed tables with a filter, a choice of which checks run, repeat every 5/15/60 minutes, cancel at any time; save a run as a baseline and compare later runs; save a diagnostic bundle (zip of summary, facts, results and raw output) with secrets masked.

### Collect (Tools → Collect)
Collect gathers evidence; it never changes anything. Pick a **show-command set** (or a **troubleshooting set** for one problem), run it, and the raw output of every command is kept on that tab: save it as a text file for a ticket, keep it as a timestamped **capture** next to the device's config backups, or send it to the assistant. Checks grade the device; Collect only collects. Per-platform sets: system and inventory, configuration (running, startup, committed, checkpoints), interfaces/counters/errors/optics/PoE, VLANs/trunks/STP/LACP, LLDP/CDP, routing/VRF/OSPF/BGP, stack/VC/VSF/VSX/MLAG, VXLAN/EVPN, logs and environment. Troubleshooting sets for the same areas. Each runs read-only, keeps its output per tab, saves to a text file or goes to the assistant. The **Change workflow** tab shows how this platform enters configuration, commits (Junos `commit confirmed`, EOS sessions, NVUE apply), saves, checkpoints and rolls back.

### Configuration lifecycle (Tools → Configs)
- **Backups vs captures**: a backup is configuration text the device can take back (marked *restorable* when it is the platform's restorable form, such as `show running-config`); a capture is timestamped show output kept for the record (the folder button on a Collect result). Both live under the device's folder; the list switches between them.
- **Restore…** on a restorable backup opens the reviewed deploy flow with the backup's lines: classification, production unlock, a fresh backup and rollback point first, then the platform's verify command.
- **Lifecycle strip** per platform, straight from its profile: *Save config* (`write memory`, `copy running-config startup-config`, `nv config save`, `save config`), *Checkpoint…* (NX-OS, IOS flash copy, AOS-CX, EOS), *Roll back…* (checkpoint rollback, `rollback 1` + commit on Junos, `configure replace`, `nv config apply startup`, IOS-XR `rollback configuration last 1`) and *History* (checkpoint/commit lists). Every action is confirmed first, needs the device name on production, and is written to the audit trail.
- **Health snapshot before and after** in the deploy flow: the Quick checks run before a change and again when you finish it, and every status that moved is listed next to the result.

### Config backups (Tools → Configs)
- Running and startup config to `Documents/OmniConsole/Configs/<site>/<device>/<timestamp>_running.cfg`.
- Each new backup is diffed against the previous one; diff any two, or a backup against the live config, ignoring timestamps and certificate blocks.
- Search the newest backup of every device (or every version), plain text or regex.

### Assistant (Tools → Assistant, `Ctrl/Cmd+Shift+A`)
- Providers: Ollama or LM Studio on this computer (Qwen3 8B suggested), OpenAI, Anthropic, or any OpenAI-compatible endpoint. API keys live in the keychain.
- Explain selected output, suggest next steps from the latest health check, interpret diagnostics, generate configuration. The detected vendor, model, OS and version are pinned into every request with platform notes (no quoted `| include` patterns on IOS, `show env all` on classic IOS, no interface ranges in show commands), and the latest health check results are included, so the model does not re-collect what the app already knows. Answers separate what the output shows from what the model infers.
- Proposed commands appear as cards classified **read-only**, **configuration change** or **disruptive**. Read-only cards run on approval; changes need a review checkbox and a second confirmation, take a backup first and write an audit entry; disruptive lines need the device name typed; production devices stay locked until unlocked for that tab.
- After a card runs, the assistant reads the result on its own: all the output that came back is interpreted, a rejected command gets one corrected replacement, and nothing that already ran is proposed again; up to three rounds. Commands the model gets wrong on the platform are corrected before they reach the card (quoted filters, bare `show memory`, interface ranges). Older proposals are marked superseded when a newer one arrives.
- Secrets are replaced with `<removed>` before anything leaves the app. Sharing of output, configuration text and device names can each be turned off.
- **Evidence rules**: every *Verified* bullet must end with the exact output line it rests on; *Likely* bullets are marked as hypotheses with the command that confirms them. After each reply the app checks the quoted lines against the output the model actually saw and flags quotes it cannot find, so a confident-sounding claim without evidence is called out.
- **Preview and target**: every proposed block shows the device and environment it will run on; nothing runs without approval, and configuration changes need a second confirmation and a backup.
- **Untrusted output**: device output goes to the model inside `<device-output>` tags marked as data, and the model is told never to follow instructions found inside it.

### Guided troubleshooting (Tools → Workflows)
Step-by-step read-only workflows: trace a device by IP or MAC to its port and the neighbor behind it, DHCP, DNS, VLAN, trunking, spanning tree, routing, ACL, wireless, authentication. Each step says what to look for and can be handed to the assistant.

### Deploy (Tools → Deploy)
- **Templates** with typed, validated variables, rendered for IOS or NX-OS: access, access point, camera, voice, trunk, uplink and port-channel ports, VLAN with SVI, and initial setup of a new switch (hostname, management SVI and VRF, gateway, DNS, NTP, SSH, local admin, logging, SNMPv3, banner).
- **Reviewed deployment**: the exact lines, a config backup, a rollback point on the device (IOS `copy running-config flash:` with `configure replace`, NX-OS and AOS-CX checkpoints, EOS saved checkpoints; Junos, IOS-XR and NVUE use their own history), an optional `reload in N` safety net on IOS or a timed `commit confirmed` / session `commit timer` on candidate platforms, line-by-line sending wrapped in the platform's configuration mode that stops at the first rejected line, the commit where one is needed, verify commands, then save or confirm, roll back, or let it revert.
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

- **Backend**: Rust with Tauri 2. `session/` holds the SSH (russh), Telnet and serial (serialport) sessions plus the capture-based command runner; `cisco/` the OS detection, parsers, 25 checks (`checks.rs` for Cisco, `generic.rs` for the other vendors), config diffing, command classification, templates and firmware planning; `ai.rs` the providers; `platforms.rs` the per-platform profiles (collections, troubleshooting sets, change workflows); `deploy.rs` the reviewed deployment; `files.rs` SFTP and SCP; `nettools.rs` the workstation tools; `store.rs` devices and the keyring.
- **Frontend**: React, TypeScript and xterm.js. `App.tsx` owns tabs, panes and shortcuts; `components/ToolsPanel.tsx` the tools sections, each keeping state per console tab.
- **CI**: `.github/workflows/build.yml` builds all six platforms and runs the backend tests.

## Roadmap

Everything above is in. Ideas for later: SNMP and NetBox lookups, an inventory view across devices, session recording playback, scheduled backups.

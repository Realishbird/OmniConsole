// One tab: an xterm.js terminal wired to a backend session.
import { Terminal, type ITheme } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
import { SearchAddon } from "@xterm/addon-search";
import { WebglAddon } from "@xterm/addon-webgl";
import { readText, writeText } from "@tauri-apps/plugin-clipboard-manager";
import { answerSession, closeSession, openSession, resizeSession, writeSession } from "./backend";
import type { ConnectSpec, PromptKind, SessionEvent } from "./types";

export type SessionState = "connecting" | "connected" | "closed" | "error";

export interface PendingPrompt {
  kind: PromptKind;
  message: string;
  fingerprint?: string;
  echo: boolean;
}

export const DARK_THEME: ITheme = {
  background: "#16181d",
  foreground: "#d8dee9",
  cursor: "#7aa2f7",
  cursorAccent: "#16181d",
  selectionBackground: "#3b4261",
  black: "#1d2026",
  red: "#f7768e",
  green: "#9ece6a",
  yellow: "#e0af68",
  blue: "#7aa2f7",
  magenta: "#bb9af7",
  cyan: "#7dcfff",
  white: "#c0caf5",
  brightBlack: "#565f89",
  brightRed: "#ff8fa3",
  brightGreen: "#b5e890",
  brightYellow: "#f3c786",
  brightBlue: "#9ab8ff",
  brightMagenta: "#cdb4ff",
  brightCyan: "#a4ddff",
  brightWhite: "#e6ebff",
};

export const LIGHT_THEME: ITheme = {
  background: "#fbfbfc",
  foreground: "#24292f",
  cursor: "#0969da",
  cursorAccent: "#fbfbfc",
  selectionBackground: "#b6d6fb",
  black: "#24292f",
  red: "#cf222e",
  green: "#116329",
  yellow: "#7d4e00",
  blue: "#0550ae",
  magenta: "#8250df",
  cyan: "#1b7c83",
  white: "#6e7781",
  brightBlack: "#57606a",
  brightRed: "#a40e26",
  brightGreen: "#1a7f37",
  brightYellow: "#633c01",
  brightBlue: "#218bff",
  brightMagenta: "#a475f9",
  brightCyan: "#3192aa",
  brightWhite: "#8c959f",
};

export interface SessionCallbacks {
  onState: (state: SessionState, message: string) => void;
  onPrompt: (prompt: PendingPrompt | null) => void;
  onSize: (cols: number, rows: number) => void;
  /** multi-line paste: resolve true to paste */
  confirmPaste: (lines: number) => Promise<boolean>;
}

export class TerminalSession {
  readonly element: HTMLDivElement;
  readonly term: Terminal;
  readonly search: SearchAddon;
  private fit: FitAddon;
  private backendId: string | null = null;
  private opened = false;
  private disposed = false;
  private generation = 0; // output of an older connection (before reconnect) is dropped

  constructor(
    readonly spec: Omit<ConnectSpec, "cols" | "rows">,
    private callbacks: SessionCallbacks,
    dark: boolean,
  ) {
    this.element = document.createElement("div");
    this.element.className = "terminal-host";
    this.term = new Terminal({
      fontFamily: '"Cascadia Mono", "JetBrains Mono", "SF Mono", Menlo, Consolas, "DejaVu Sans Mono", monospace',
      fontSize: 14,
      lineHeight: 1.15,
      cursorBlink: true,
      cursorStyle: "block",
      scrollback: 50000,
      allowProposedApi: true,
      macOptionIsMeta: true,
      rightClickSelectsWord: false,
      theme: dark ? DARK_THEME : LIGHT_THEME,
    });
    this.fit = new FitAddon();
    this.search = new SearchAddon();
    this.term.loadAddon(this.fit);
    this.term.loadAddon(this.search);

    this.term.onData((data) => {
      if (this.backendId) void writeSession(this.backendId, data);
    });
    this.term.onResize(({ cols, rows }) => {
      this.callbacks.onSize(cols, rows);
      if (this.backendId) void resizeSession(this.backendId, cols, rows);
    });
    // copy on select and right-click paste, like PuTTY and SecureCRT
    this.term.onSelectionChange(() => {
      const text = this.term.getSelection();
      if (text) void writeText(text).catch(() => {});
    });
    this.element.addEventListener("contextmenu", (e) => {
      e.preventDefault();
      void this.pasteClipboard();
    });
    // every paste goes through pasteText, so a multi-line paste is confirmed
    this.element.addEventListener(
      "paste",
      (e) => {
        e.preventDefault();
        e.stopPropagation();
        void this.pasteText(e.clipboardData?.getData("text/plain") ?? "");
      },
      true,
    );
  }

  async pasteClipboard() {
    try {
      await this.pasteText(await readText());
    } catch {
      // empty or non-text clipboard
    }
  }

  async pasteText(text: string) {
    if (!text || !this.backendId) return;
    const lines = text.replace(/\r?\n$/, "").split(/\r?\n/).length;
    if (lines > 1 && !(await this.callbacks.confirmPaste(lines))) {
      this.term.focus();
      return;
    }
    this.term.paste(text);
    this.term.focus();
  }

  copySelection() {
    const text = this.term.getSelection();
    if (text) void writeText(text).catch(() => {});
  }

  /** First time the tab is shown: attach to the DOM, size it, connect. */
  async attach(container: HTMLElement) {
    if (this.element.parentElement !== container) container.appendChild(this.element);
    if (this.opened) {
      this.fitNow();
      return;
    }
    this.opened = true;
    this.term.open(this.element);
    try {
      this.term.loadAddon(new WebglAddon());
    } catch {
      // software rendering still works
    }
    this.fitNow();
    await this.connect();
  }

  async connect() {
    if (this.disposed) return;
    const gen = ++this.generation;
    this.callbacks.onState("connecting", "Connecting");
    this.backendId = await openSession(
      { ...this.spec, cols: this.term.cols, rows: this.term.rows },
      (bytes) => {
        if (gen === this.generation && !this.disposed) this.term.write(bytes);
      },
      (event) => {
        if (gen === this.generation && !this.disposed) this.handleEvent(event);
      },
    );
  }

  /** Reconnect a closed session in the same tab. */
  async reconnect() {
    this.generation++;
    if (this.backendId) void closeSession(this.backendId);
    this.backendId = null;
    this.term.write("\r\n");
    await this.connect();
  }

  private handleEvent(event: SessionEvent) {
    if (event.type === "status") {
      if (event.state !== "connecting") this.callbacks.onPrompt(null);
      this.callbacks.onState(event.state, event.message);
    } else {
      this.callbacks.onPrompt({
        kind: event.kind,
        message: event.message,
        fingerprint: event.fingerprint,
        echo: event.echo,
      });
    }
  }

  answer(value: string | null) {
    this.callbacks.onPrompt(null);
    if (this.backendId) void answerSession(this.backendId, value);
    this.term.focus();
  }

  fitNow() {
    if (!this.opened || this.element.offsetParent === null) return;
    try {
      this.fit.fit();
    } catch {
      // not laid out yet
    }
  }

  focus() {
    this.term.focus();
  }

  setDark(dark: boolean) {
    this.term.options.theme = dark ? DARK_THEME : LIGHT_THEME;
  }

  setFontSize(size: number) {
    this.term.options.fontSize = size;
    this.fitNow();
  }

  /** Whole scrollback as plain text, for "Save output". */
  text(): string {
    const buf = this.term.buffer.active;
    const lines: string[] = [];
    for (let i = 0; i < buf.length; i++) {
      lines.push(buf.getLine(i)?.translateToString(true) ?? "");
    }
    while (lines.length && lines[lines.length - 1] === "") lines.pop();
    return lines.join("\n") + "\n";
  }

  dispose() {
    this.disposed = true;
    if (this.backendId) void closeSession(this.backendId);
    this.backendId = null;
    this.term.dispose();
    this.element.remove();
  }
}

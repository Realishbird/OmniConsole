// One terminal: an xterm.js instance wired to a backend session.
import { Terminal, type IDecoration, type IMarker, type ITheme } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
import { SearchAddon } from "@xterm/addon-search";
import { WebglAddon } from "@xterm/addon-webgl";
import { readText, writeText } from "@tauri-apps/plugin-clipboard-manager";
import { answerSession, breakSession, closeSession, openSession, resizeSession, writeSession } from "./backend";
import { Highlighter } from "./highlight";
import { parsePrompt, type PromptInfo } from "./prompt";
import { fontStack, type Settings } from "./settings";
import { themeById } from "./themes";
import type { ConnectSpec, PromptKind, SessionEvent } from "./types";

export type SessionState = "connecting" | "connected" | "closed" | "error";

export interface PendingPrompt {
  kind: PromptKind;
  message: string;
  fingerprint?: string;
  echo: boolean;
}

export interface Bookmark {
  id: number;
  label: string;
  at: number;
  marker: IMarker;
  decoration?: IDecoration;
}

export interface SessionCallbacks {
  onState: (state: SessionState, message: string) => void;
  onPrompt: (prompt: PendingPrompt | null) => void;
  onSize: (cols: number, rows: number) => void;
  onLog: (path: string) => void;
  /** the device prompt changed (host name, privilege level) */
  onPromptInfo: (info: PromptInfo | null) => void;
  /** a command line was entered at a device prompt */
  onCommand: (command: string) => void;
  onFocus: () => void;
  /** multi-line paste: resolve true to paste */
  confirmPaste: (lines: number) => Promise<boolean>;
}

export type SessionSpec = Omit<ConnectSpec, "cols" | "rows">;

function termTheme(settings: Settings): ITheme {
  return { ...themeById(settings.theme).term, ...settings.ansiOverrides };
}

export class TerminalSession {
  readonly element: HTMLDivElement;
  readonly term: Terminal;
  readonly search: SearchAddon;
  bookmarks: Bookmark[] = [];
  private fit: FitAddon;
  private backendId: string | null = null;
  private opened = false;
  private disposed = false;
  private generation = 0; // output of an older connection (before reconnect) is dropped
  private decoder = new TextDecoder();
  private highlighter = new Highlighter();
  private highlight: boolean;
  private promptTimer: number | undefined;
  private lastPrompt = "";
  private nextBookmark = 1;

  constructor(
    public spec: SessionSpec,
    private callbacks: SessionCallbacks,
    settings: Settings,
  ) {
    this.highlight = settings.highlight;
    this.element = document.createElement("div");
    this.element.className = "terminal-host";
    this.term = new Terminal({
      fontFamily: fontStack(settings.fontFamily),
      fontSize: settings.fontSize,
      lineHeight: settings.lineHeight,
      letterSpacing: settings.letterSpacing,
      cursorBlink: settings.cursorBlink,
      cursorStyle: settings.cursorStyle,
      minimumContrastRatio: settings.minimumContrast,
      scrollback: settings.scrollback,
      allowProposedApi: true,
      macOptionIsMeta: true,
      rightClickSelectsWord: false,
      theme: termTheme(settings),
    });
    this.fit = new FitAddon();
    this.search = new SearchAddon();
    this.term.loadAddon(this.fit);
    this.term.loadAddon(this.search);

    this.term.onData((data) => {
      if (!this.backendId) return;
      if (data.includes("\r")) this.captureCommand();
      void writeSession(this.backendId, data);
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
    this.element.addEventListener("mousedown", () => this.callbacks.onFocus());
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

  get connectedId(): string | null {
    return this.backendId;
  }

  applySettings(settings: Settings) {
    const o = this.term.options;
    o.theme = termTheme(settings);
    o.fontFamily = fontStack(settings.fontFamily);
    o.fontSize = settings.fontSize;
    o.lineHeight = settings.lineHeight;
    o.letterSpacing = settings.letterSpacing;
    o.cursorStyle = settings.cursorStyle;
    o.cursorBlink = settings.cursorBlink;
    o.minimumContrastRatio = settings.minimumContrast;
    o.scrollback = settings.scrollback;
    this.highlight = settings.highlight;
    this.fitNow();
  }

  /* ---------- input ---------- */

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

  /** Type text into the session as if typed (snippets, history). */
  send(text: string) {
    if (!this.backendId) return;
    if (text.includes("\r")) this.captureCommand();
    void writeSession(this.backendId, text);
    this.term.focus();
  }

  sendBreak() {
    if (this.backendId) void breakSession(this.backendId);
  }

  copySelection() {
    const text = this.term.getSelection();
    if (text) void writeText(text).catch(() => {});
  }

  selection(): string {
    return this.term.getSelection();
  }

  /** The line the cursor is on, as plain text. */
  private cursorLine(): string {
    const buf = this.term.buffer.active;
    return buf.getLine(buf.baseY + buf.cursorY)?.translateToString(true) ?? "";
  }

  /** Enter was pressed: record what was typed after the device prompt. */
  private captureCommand() {
    const p = parsePrompt(this.cursorLine());
    if (p?.command) this.callbacks.onCommand(p.command);
  }

  private checkPrompt() {
    const line = this.cursorLine();
    const p = parsePrompt(line);
    const key = p ? `${p.hostname}|${p.privilege}|${p.mode}` : "";
    if (p && key !== this.lastPrompt) {
      this.lastPrompt = key;
      this.callbacks.onPromptInfo(p);
    }
  }

  /* ---------- lifecycle ---------- */

  /** First time the terminal is shown: attach to the DOM, size it, connect. */
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

  private output(bytes: Uint8Array) {
    if (this.highlight) this.term.write(this.highlighter.feed(this.decoder.decode(bytes, { stream: true })));
    else this.term.write(bytes);
    window.clearTimeout(this.promptTimer);
    this.promptTimer = window.setTimeout(() => this.checkPrompt(), 120);
  }

  async connect() {
    if (this.disposed) return;
    const gen = ++this.generation;
    this.decoder = new TextDecoder();
    this.highlighter = new Highlighter();
    this.lastPrompt = "";
    this.callbacks.onPromptInfo(null);
    this.callbacks.onState("connecting", "Connecting");
    try {
      this.backendId = await openSession(
        { ...this.spec, cols: this.term.cols, rows: this.term.rows },
        (bytes) => {
          if (gen === this.generation && !this.disposed) this.output(bytes);
        },
        (event) => {
          if (gen === this.generation && !this.disposed) this.handleEvent(event);
        },
      );
    } catch (e) {
      this.callbacks.onState("error", String(e));
    }
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
    } else if (event.type === "log") {
      this.callbacks.onLog(event.path);
    } else {
      this.callbacks.onPrompt({ kind: event.kind, message: event.message, fingerprint: event.fingerprint, echo: event.echo });
    }
  }

  answer(value: string | null) {
    this.callbacks.onPrompt(null);
    if (this.backendId) void answerSession(this.backendId, value);
    this.term.focus();
  }

  /** A grey line from OmniConsole itself. */
  notice(text: string) {
    this.term.write(`\r\n\x1b[90m${text}\x1b[0m\r\n`);
  }

  /* ---------- bookmarks ---------- */

  addBookmark(label?: string): Bookmark | null {
    const marker = this.term.registerMarker(0);
    if (!marker) return null;
    const id = this.nextBookmark++;
    const decoration = this.term.registerDecoration({
      marker,
      width: 1,
      overviewRulerOptions: { color: "#e0af68", position: "full" },
    });
    decoration?.onRender((el) => {
      el.classList.add("bookmark-mark");
      el.title = `Bookmark ${id}`;
    });
    const text = this.cursorLine().trim();
    const b: Bookmark = { id, label: label || text || `Bookmark ${id}`, at: Date.now(), marker, decoration };
    marker.onDispose(() => {
      this.bookmarks = this.bookmarks.filter((x) => x !== b);
    });
    this.bookmarks.push(b);
    return b;
  }

  goToBookmark(id: number) {
    const b = this.bookmarks.find((x) => x.id === id);
    if (b && b.marker.line >= 0) this.term.scrollToLine(Math.max(0, b.marker.line - 3));
  }

  /* ---------- layout and output ---------- */

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
    window.clearTimeout(this.promptTimer);
    if (this.backendId) void closeSession(this.backendId);
    this.backendId = null;
    this.term.dispose();
    this.element.remove();
  }
}

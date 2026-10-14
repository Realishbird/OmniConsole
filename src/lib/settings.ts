// App settings, kept in settings.json next to devices.json (the backend
// reads logDir from it).
export type AnsiKey =
  | "black" | "red" | "green" | "yellow" | "blue" | "magenta" | "cyan" | "white"
  | "brightBlack" | "brightRed" | "brightGreen" | "brightYellow" | "brightBlue" | "brightMagenta" | "brightCyan" | "brightWhite";

export type CursorStyle = "block" | "bar" | "underline";
export type Density = "compact" | "comfortable";
export type GroupBy = "site" | "customer" | "building" | "role" | "environment" | "tag" | "none";

export interface Settings {
  theme: string;
  fontFamily: string;
  fontSize: number;
  lineHeight: number;
  letterSpacing: number;
  cursorStyle: CursorStyle;
  cursorBlink: boolean;
  minimumContrast: number;
  /** "#rrggbb" overrides for the 16 ANSI colors, keyed like ITheme */
  ansiOverrides: Partial<Record<AnsiKey, string>>;
  density: Density;
  scrollback: number;
  highlight: boolean;
  logAll: boolean;
  logTimestamps: boolean;
  logDir: string;
  /** config backups folder; empty = Documents/OmniConsole/Configs */
  configDir: string;
  maskExports: boolean;
  warnInsecure: boolean;
  groupBy: GroupBy;
}

export const FONT_CHOICES = [
  "Cascadia Mono",
  "JetBrains Mono",
  "Fira Code",
  "SF Mono",
  "Menlo",
  "Consolas",
  "DejaVu Sans Mono",
  "Ubuntu Mono",
  "Source Code Pro",
];

export const DEFAULT_SETTINGS: Settings = {
  theme: "midnight",
  fontFamily: "Cascadia Mono",
  fontSize: 14,
  lineHeight: 1.15,
  letterSpacing: 0,
  cursorStyle: "block",
  cursorBlink: true,
  minimumContrast: 1,
  ansiOverrides: {},
  density: "comfortable",
  scrollback: 50000,
  highlight: true,
  logAll: false,
  logTimestamps: true,
  logDir: "",
  configDir: "",
  maskExports: true,
  warnInsecure: true,
  groupBy: "site",
};

/** The chosen font first, then fallbacks every OS has. */
export function fontStack(family: string): string {
  const fallbacks = ['"Cascadia Mono"', '"JetBrains Mono"', '"SF Mono"', "Menlo", "Consolas", '"DejaVu Sans Mono"', "monospace"];
  return [`"${family.replace(/"/g, "")}"`, ...fallbacks].join(", ");
}

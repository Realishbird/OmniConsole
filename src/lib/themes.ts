// Interface and terminal color themes. Each theme sets the CSS tokens in
// styles.css and the xterm.js palette.
import type { ITheme } from "@xterm/xterm";

export interface Theme {
  id: string;
  name: string;
  dark: boolean;
  ui: Record<string, string>;
  term: ITheme;
}

function ui(t: {
  bg: string; elev: string; sidebar: string; chrome: string; border: string; text: string; quiet: string;
  accent: string; accentText: string; good: string; warn: string; bad: string; dark: boolean;
}): Record<string, string> {
  return {
    "--bg": t.bg,
    "--bg-elev": t.elev,
    "--sidebar": t.sidebar,
    "--chrome": t.chrome,
    "--border": t.border,
    "--text": t.text,
    "--text-quiet": t.quiet,
    "--hover": t.dark ? "rgba(255, 255, 255, 0.06)" : "rgba(0, 0, 0, 0.06)",
    "--active": `color-mix(in srgb, ${t.accent} 18%, transparent)`,
    "--accent": t.accent,
    "--accent-text": t.accentText,
    "--good": t.good,
    "--warn": t.warn,
    "--bad": t.bad,
    "--shadow": t.dark ? "0 12px 40px rgba(0, 0, 0, 0.5)" : "0 12px 40px rgba(0, 0, 0, 0.18)",
  };
}

export const THEMES: Theme[] = [
  {
    id: "midnight",
    name: "Midnight",
    dark: true,
    ui: ui({ bg: "#16181d", elev: "#1f2229", sidebar: "#1a1d23", chrome: "#1d2027", border: "#2c313a", text: "#d8dee9", quiet: "#8b93a7", accent: "#7aa2f7", accentText: "#10131a", good: "#9ece6a", warn: "#e0af68", bad: "#f7768e", dark: true }),
    term: {
      background: "#16181d", foreground: "#d8dee9", cursor: "#7aa2f7", cursorAccent: "#16181d", selectionBackground: "#3b4261",
      black: "#1d2026", red: "#f7768e", green: "#9ece6a", yellow: "#e0af68", blue: "#7aa2f7", magenta: "#bb9af7", cyan: "#7dcfff", white: "#c0caf5",
      brightBlack: "#565f89", brightRed: "#ff8fa3", brightGreen: "#b5e890", brightYellow: "#f3c786", brightBlue: "#9ab8ff", brightMagenta: "#cdb4ff", brightCyan: "#a4ddff", brightWhite: "#e6ebff",
    },
  },
  {
    id: "graphite",
    name: "Graphite",
    dark: true,
    ui: ui({ bg: "#1e1f22", elev: "#2b2d31", sidebar: "#232428", chrome: "#26272b", border: "#3a3c42", text: "#dcdde0", quiet: "#949ba4", accent: "#5b9bf8", accentText: "#0d1117", good: "#73c991", warn: "#e2c08d", bad: "#f48771", dark: true }),
    term: {
      background: "#1e1f22", foreground: "#d4d4d4", cursor: "#aeafad", cursorAccent: "#1e1f22", selectionBackground: "#3e4451",
      black: "#1e1f22", red: "#f14c4c", green: "#23d18b", yellow: "#f5f543", blue: "#3b8eea", magenta: "#d670d6", cyan: "#29b8db", white: "#cccccc",
      brightBlack: "#6e7681", brightRed: "#ff6b6b", brightGreen: "#3fe3a0", brightYellow: "#ffff66", brightBlue: "#5ca8ff", brightMagenta: "#e88fe8", brightCyan: "#4fd0f0", brightWhite: "#f2f2f2",
    },
  },
  {
    id: "nord",
    name: "Nord",
    dark: true,
    ui: ui({ bg: "#2e3440", elev: "#3b4252", sidebar: "#2b303b", chrome: "#323845", border: "#434c5e", text: "#eceff4", quiet: "#a3abbd", accent: "#88c0d0", accentText: "#2e3440", good: "#a3be8c", warn: "#ebcb8b", bad: "#bf616a", dark: true }),
    term: {
      background: "#2e3440", foreground: "#d8dee9", cursor: "#d8dee9", cursorAccent: "#2e3440", selectionBackground: "#4c566a",
      black: "#3b4252", red: "#bf616a", green: "#a3be8c", yellow: "#ebcb8b", blue: "#81a1c1", magenta: "#b48ead", cyan: "#88c0d0", white: "#e5e9f0",
      brightBlack: "#4c566a", brightRed: "#d27a82", brightGreen: "#b5cfa0", brightYellow: "#f0d6a2", brightBlue: "#94b3d1", brightMagenta: "#c4a3bf", brightCyan: "#8fbcbb", brightWhite: "#eceff4",
    },
  },
  {
    id: "dracula",
    name: "Dracula",
    dark: true,
    ui: ui({ bg: "#282a36", elev: "#343746", sidebar: "#21222c", chrome: "#252631", border: "#44475a", text: "#f8f8f2", quiet: "#a4a8c8", accent: "#bd93f9", accentText: "#282a36", good: "#50fa7b", warn: "#f1fa8c", bad: "#ff5555", dark: true }),
    term: {
      background: "#282a36", foreground: "#f8f8f2", cursor: "#f8f8f2", cursorAccent: "#282a36", selectionBackground: "#44475a",
      black: "#21222c", red: "#ff5555", green: "#50fa7b", yellow: "#f1fa8c", blue: "#bd93f9", magenta: "#ff79c6", cyan: "#8be9fd", white: "#f8f8f2",
      brightBlack: "#6272a4", brightRed: "#ff6e6e", brightGreen: "#69ff94", brightYellow: "#ffffa5", brightBlue: "#d6acff", brightMagenta: "#ff92df", brightCyan: "#a4ffff", brightWhite: "#ffffff",
    },
  },
  {
    id: "solarized-dark",
    name: "Solarized Dark",
    dark: true,
    ui: ui({ bg: "#002b36", elev: "#073642", sidebar: "#00252f", chrome: "#00303b", border: "#0d4a58", text: "#c4d0d0", quiet: "#839496", accent: "#268bd2", accentText: "#fdf6e3", good: "#859900", warn: "#b58900", bad: "#dc322f", dark: true }),
    term: {
      background: "#002b36", foreground: "#93a1a1", cursor: "#93a1a1", cursorAccent: "#002b36", selectionBackground: "#0d4a58",
      black: "#073642", red: "#dc322f", green: "#859900", yellow: "#b58900", blue: "#268bd2", magenta: "#d33682", cyan: "#2aa198", white: "#eee8d5",
      brightBlack: "#586e75", brightRed: "#cb4b16", brightGreen: "#9cb200", brightYellow: "#d0a300", brightBlue: "#4aa3e8", brightMagenta: "#6c71c4", brightCyan: "#35c4b8", brightWhite: "#fdf6e3",
    },
  },
  {
    id: "light",
    name: "Light",
    dark: false,
    ui: ui({ bg: "#fbfbfc", elev: "#ffffff", sidebar: "#f2f3f5", chrome: "#eceef1", border: "#d9dce1", text: "#1f2328", quiet: "#656d76", accent: "#0969da", accentText: "#ffffff", good: "#1a7f37", warn: "#9a6700", bad: "#cf222e", dark: false }),
    term: {
      background: "#fbfbfc", foreground: "#24292f", cursor: "#0969da", cursorAccent: "#fbfbfc", selectionBackground: "#b6d6fb",
      black: "#24292f", red: "#cf222e", green: "#116329", yellow: "#7d4e00", blue: "#0550ae", magenta: "#8250df", cyan: "#1b7c83", white: "#6e7781",
      brightBlack: "#57606a", brightRed: "#a40e26", brightGreen: "#1a7f37", brightYellow: "#633c01", brightBlue: "#218bff", brightMagenta: "#a475f9", brightCyan: "#3192aa", brightWhite: "#8c959f",
    },
  },
];

export function themeById(id: string): Theme {
  return THEMES.find((t) => t.id === id) ?? THEMES[0];
}

export function applyTheme(theme: Theme) {
  const root = document.documentElement;
  for (const [k, v] of Object.entries(theme.ui)) root.style.setProperty(k, v);
  root.dataset.theme = theme.dark ? "dark" : "light";
}

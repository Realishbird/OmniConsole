// Optional output highlighting: colors IP and MAC addresses, interface names,
// errors, warnings and good states in plain device output. It only touches
// text outside escape sequences, uses the theme's ANSI colors, and steps
// aside while the device sets its own colors or runs a full-screen program.

const FG = { red: 31, green: 32, yellow: 33, blue: 34, magenta: 35, cyan: 36 } as const;

const RULES: { color: number; re: string }[] = [
  // syslog severity 0-3 / 4 in %FACILITY-SEV-MNEMONIC
  { color: FG.red, re: String.raw`%[A-Z0-9_]+(?:-[A-Z0-9_]+)*-[0-3]-[A-Z0-9_]+` },
  { color: FG.yellow, re: String.raw`%[A-Z0-9_]+(?:-[A-Z0-9_]+)*-4-[A-Z0-9_]+` },
  { color: FG.cyan, re: String.raw`\b(?:25[0-5]|2[0-4]\d|1?\d?\d)(?:\.(?:25[0-5]|2[0-4]\d|1?\d?\d)){3}(?:\/\d{1,2})?\b` },
  { color: FG.cyan, re: String.raw`\b(?:[0-9a-fA-F]{1,4}:){2,7}[0-9a-fA-F]{1,4}(?:\/\d{1,3})?\b|\b(?:[0-9a-fA-F]{1,4}:){1,6}:(?:[0-9a-fA-F]{1,4})?(?:\/\d{1,3})?` },
  { color: FG.magenta, re: String.raw`\b[0-9a-fA-F]{4}\.[0-9a-fA-F]{4}\.[0-9a-fA-F]{4}\b|\b(?:[0-9a-fA-F]{2}[:-]){5}[0-9a-fA-F]{2}\b` },
  {
    color: FG.blue,
    re: String.raw`\b(?:(?:Hundred|Forty|TwentyFive|Ten|Two|Five)?Gig(?:abit)?E(?:thernet)?|FastEthernet|Ethernet|AppGigabitEthernet|Port-channel|Vlan|Loopback|Tunnel|Serial|mgmt|Hu|Fo|Twe|Te|Tw|Gi|Fa|Et|Eth|Po|Vl|Lo|Tu|Se|Ap)\d+(?:\/\d+){0,3}(?:\.\d+)?\b`,
  },
  { color: FG.red, re: String.raw`\b(?:err-disabled|errdisable|err-disable|notconnect|down|failed|failure|denied|invalid|blocked|BKN|incomplete|unreachable|critical|Error|ERROR|error)\b` },
  { color: FG.yellow, re: String.raw`\b(?:administratively|warning|Warning|WARNING|half-duplex|a-half|half|flapping|suspended|Idle|Active|Connect|OpenSent)\b` },
  { color: FG.green, re: String.raw`\b(?:up|connected|forwarding|FWD|established|Established|FULL|Full|full-duplex|a-full|enabled|OK|ok)\b` },
];

const COMBINED = new RegExp(RULES.map((r, i) => `(?<r${i}>${r.re})`).join("|"), "g");

function colorize(text: string): string {
  if (!/[\w%]/.test(text)) return text;
  return text.replace(COMBINED, (match, ...args) => {
    const groups = args[args.length - 1] as Record<string, string | undefined>;
    const i = RULES.findIndex((_, n) => groups[`r${n}`] !== undefined);
    return i < 0 ? match : `\x1b[${RULES[i].color}m${match}\x1b[39m`;
  });
}

type Esc = "none" | "esc" | "csi" | "osc" | "oscEsc" | "two";

export class Highlighter {
  private esc: Esc = "none";
  private csi = "";
  private colored = false; // the device set its own colors
  private altScreen = false;

  /** Transform one chunk of decoded output. */
  feed(chunk: string): string {
    let out = "";
    let run = "";
    const flush = () => {
      if (run) {
        out += this.colored || this.altScreen ? run : colorize(run);
        run = "";
      }
    };
    for (const ch of chunk) {
      switch (this.esc) {
        case "none":
          if (ch === "\x1b") {
            flush();
            out += ch;
            this.esc = "esc";
          } else {
            run += ch;
          }
          continue;
        case "esc":
          out += ch;
          if (ch === "[") {
            this.csi = "";
            this.esc = "csi";
          } else if (ch === "]") this.esc = "osc";
          else if ("()#%".includes(ch)) this.esc = "two";
          else this.esc = "none";
          continue;
        case "two":
          out += ch;
          this.esc = "none";
          continue;
        case "csi":
          out += ch;
          if (ch >= "\x40" && ch <= "\x7e") {
            this.esc = "none";
            this.csiDone(this.csi, ch);
          } else this.csi += ch;
          continue;
        case "osc":
          out += ch;
          if (ch === "\x07") this.esc = "none";
          else if (ch === "\x1b") this.esc = "oscEsc";
          continue;
        case "oscEsc":
          out += ch;
          this.esc = ch === "\\" ? "none" : "osc";
          continue;
      }
    }
    flush();
    return out;
  }

  private csiDone(params: string, final: string) {
    if (final === "m") {
      const codes = params.split(";").filter((p) => p !== "");
      this.colored = !(codes.length === 0 || codes.every((c) => c === "0" || c === "39" || c === "49"));
    } else if ((final === "h" || final === "l") && /^\?(1049|1047|47)$/.test(params)) {
      this.altScreen = final === "h";
    }
  }
}

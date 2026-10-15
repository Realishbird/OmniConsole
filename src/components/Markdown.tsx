import type { ReactNode } from "react";

/** Enough Markdown for assistant replies: headings, bullets, code, bold. */
/** `hideCommands` drops fenced command blocks (they are shown as review cards instead). */
export default function Markdown({ text, hideCommands = false }: { text: string; hideCommands?: boolean }) {
  const out: ReactNode[] = [];
  const lines = text.split("\n");
  let i = 0;
  let key = 0;
  // a heading is a node with a `heading` mark; empty headings are dropped at the end
  const heading = (text: string) => {
    const t = text.trim().toLowerCase();
    const cls = t === "verified" ? " verified" : t === "likely" ? " likely" : "";
    const label = t === "verified" ? "Verified from output" : t === "likely" ? "Likely (hypothesis, not confirmed)" : text;
    out.push(<div key={key++} className={`md-h${cls}`} data-h="1">{inline(label)}</div>);
  };
  const code = (tag: string, body: string[]) => {
    const commandish = ["cli", "cisco", "ios", "junos", "eos"].includes(tag) || (["", "text", "bash", "shell", "sh", "console", "plaintext"].includes(tag) && body.some((l) => l.trim()) && body.every((l) => !l.trim() || /^[a-z!#]/i.test(l.trim())));
    if (hideCommands && commandish) {
      // the block is shown as a review card; a heading that only introduced it goes too
      const last = out[out.length - 1] as { props?: { "data-h"?: string } } | undefined;
      if (last?.props?.["data-h"]) out.pop();
      return;
    }
    out.push(<pre key={key++} className={`md-code ${tag}`}>{body.join("\n")}</pre>);
  };
  while (i < lines.length) {
    const line = lines[i];
    // ```bash show version``` on one line
    const oneLine = line.match(/^\s*```(\w*)\s+(.+?)\s*```\s*$/);
    if (oneLine) {
      code(oneLine[1].toLowerCase(), [oneLine[2]]);
      i++;
      continue;
    }
    const fence = line.match(/^\s*```(\w*)\s*(.*)$/);
    if (fence) {
      const body: string[] = [];
      if (fence[2].trim()) body.push(fence[2].trim());
      i++;
      while (i < lines.length && !lines[i].trim().startsWith("```")) body.push(lines[i++]);
      i++;
      code(fence[1].toLowerCase(), body);
      continue;
    }
    if (/^\s*([-*_]\s*){3,}$/.test(line)) {
      out.push(<hr key={key++} />);
      i++;
      continue;
    }
    const h = line.match(/^(#{1,4})\s+(.*)/) ?? line.match(/^\*\*([^*]+)\*\*:?\s*$/) ?? line.match(/^(?:__)?(Verified|Likely|Next steps|Corrected command|Purpose|Summary|Interpretation)(?:__)?:?\s*$/i);
    if (h) {
      heading((h[2] ?? h[1]).replace(/^#+\s*/, "").replace(/\*\*/g, "").replace(/:$/, ""));
      i++;
      continue;
    }
    if (/^\s*[-*•]\s+/.test(line)) {
      const items: string[] = [];
      while (i < lines.length && /^\s*[-*•]\s+/.test(lines[i])) items.push(lines[i++].replace(/^\s*[-*•]\s+/, ""));
      out.push(<ul key={key++}>{items.map((it, j) => <li key={j}>{inline(it)}</li>)}</ul>);
      continue;
    }
    if (/^\s*\d+[.)]\s+/.test(line)) {
      const items: string[] = [];
      while (i < lines.length && /^\s*\d+[.)]\s+/.test(lines[i])) items.push(lines[i++].replace(/^\s*\d+[.)]\s+/, ""));
      out.push(<ol key={key++}>{items.map((it, j) => <li key={j}>{inline(it)}</li>)}</ol>);
      continue;
    }
    if (line.trim() === "") {
      i++;
      continue;
    }
    const para: string[] = [];
    while (i < lines.length && lines[i].trim() !== "" && !/^(\s*```|#{1,4}\s|\s*[-*•]\s+|\s*\d+[.)]\s+|\s*([-*_]\s*){3,}$)/.test(lines[i])) para.push(lines[i++]);
    out.push(<p key={key++}>{inline(para.join(" "))}</p>);
  }
  // a heading with nothing under it (the model had nothing verified yet) is noise
  const isH = (n: ReactNode) => !!(n as { props?: { "data-h"?: string } })?.props?.["data-h"];
  const cleaned = out.filter((n, idx) => !(isH(n) && (idx === out.length - 1 || isH(out[idx + 1]))));
  return <div className="md">{cleaned}</div>;
}

function inline(text: string): ReactNode[] {
  const parts: ReactNode[] = [];
  const re = /(```[^`]+```|`[^`]+`|\*\*[^*]+\*\*)/g;
  let last = 0;
  let m: RegExpExecArray | null;
  let k = 0;
  while ((m = re.exec(text))) {
    if (m.index > last) parts.push(text.slice(last, m.index));
    const t = m[0];
    if (t.startsWith("```")) parts.push(<code key={k++}>{t.slice(3, -3).replace(/^\w+\s+(?=\S)/, "").trim()}</code>);
    else if (t.startsWith("`")) parts.push(<code key={k++}>{t.slice(1, -1)}</code>);
    else parts.push(<strong key={k++}>{t.slice(2, -2)}</strong>);
    last = m.index + t.length;
  }
  if (last < text.length) parts.push(text.slice(last));
  return parts;
}

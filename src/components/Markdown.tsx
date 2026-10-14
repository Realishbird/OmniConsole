import type { ReactNode } from "react";

/** Enough Markdown for assistant replies: headings, bullets, code, bold. */
export default function Markdown({ text }: { text: string }) {
  const out: ReactNode[] = [];
  const lines = text.split("\n");
  let i = 0;
  let key = 0;
  while (i < lines.length) {
    const line = lines[i];
    const fence = line.match(/^```(\w*)/);
    if (fence) {
      const body: string[] = [];
      i++;
      while (i < lines.length && !lines[i].startsWith("```")) body.push(lines[i++]);
      i++;
      out.push(<pre key={key++} className={`md-code ${fence[1]}`}>{body.join("\n")}</pre>);
      continue;
    }
    const h = line.match(/^(#{1,4})\s+(.*)/);
    if (h) {
      out.push(<div key={key++} className="md-h">{inline(h[2])}</div>);
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
    while (i < lines.length && lines[i].trim() !== "" && !/^(```|#{1,4}\s|\s*[-*•]\s+|\s*\d+[.)]\s+)/.test(lines[i])) para.push(lines[i++]);
    out.push(<p key={key++}>{inline(para.join(" "))}</p>);
  }
  return <div className="md">{out}</div>;
}

function inline(text: string): ReactNode[] {
  const parts: ReactNode[] = [];
  const re = /(`[^`]+`|\*\*[^*]+\*\*)/g;
  let last = 0;
  let m: RegExpExecArray | null;
  let k = 0;
  while ((m = re.exec(text))) {
    if (m.index > last) parts.push(text.slice(last, m.index));
    const t = m[0];
    if (t.startsWith("`")) parts.push(<code key={k++}>{t.slice(1, -1)}</code>);
    else parts.push(<strong key={k++}>{t.slice(2, -2)}</strong>);
    last = m.index + t.length;
  }
  if (last < text.length) parts.push(text.slice(last));
  return parts;
}

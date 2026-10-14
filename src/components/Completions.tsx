import { useEffect, useState } from "react";
import type { Completion } from "../lib/files";

interface Props {
  items: Completion[];
  partial: string;
  loading: boolean;
  onPick: (word: string) => void;
  onClose: () => void;
}

/** The device's own `?` help as a pick list over the terminal (Ctrl+Space). */
export default function Completions({ items, partial, loading, onPick, onClose }: Props) {
  const [sel, setSel] = useState(0);
  useEffect(() => setSel(0), [items]);
  useEffect(() => {
    function onKey(e: KeyboardEvent) {
      if (e.key === "Escape") onClose();
      else if (e.key === "ArrowDown") setSel((s) => Math.min(items.length - 1, s + 1));
      else if (e.key === "ArrowUp") setSel((s) => Math.max(0, s - 1));
      else if (e.key === "Enter" || e.key === "Tab") {
        if (items[sel]) onPick(items[sel].word);
        else onClose();
      } else return;
      e.preventDefault();
      e.stopPropagation();
    }
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  }, [items, sel, onPick, onClose]);

  return (
    <div className="completions" onMouseDown={(e) => e.preventDefault()}>
      <div className="completions-head"><code>{partial || "(start of line)"}</code> ?<span className="grow" /><span className="quiet small">{loading ? "asking the device…" : `${items.length} · ↑↓ Enter · Esc`}</span></div>
      {!loading && items.length === 0 && <div className="quiet small pad">Nothing to complete here.</div>}
      <div className="completions-list">
        {items.map((c, i) => (
          <div key={i} className={`completion${i === sel ? " on" : ""}`} onMouseEnter={() => setSel(i)} onClick={() => onPick(c.word)}>
            <code>{c.word}</code><span className="quiet">{c.help}</span>
          </div>
        ))}
      </div>
    </div>
  );
}

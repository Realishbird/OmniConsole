import { useMemo, useRef, useState } from "react";

export interface PaletteItem {
  id: string;
  title: string;
  group: string;
  subtitle?: string;
  keys?: string;
  run: () => void;
}

interface Props {
  items: PaletteItem[];
  placeholder?: string;
  initial?: string;
  onClose: () => void;
}

/** Subsequence match; earlier, contiguous and word-start hits score higher. */
function score(query: string, text: string): number {
  if (!query) return 1;
  const q = query.toLowerCase();
  const t = text.toLowerCase();
  const direct = t.indexOf(q);
  if (direct >= 0) return 1000 - direct + (direct === 0 || /\W/.test(t[direct - 1]) ? 200 : 0);
  let ti = 0;
  let s = 0;
  let run = 0;
  for (const ch of q) {
    const found = t.indexOf(ch, ti);
    if (found < 0) return 0;
    run = found === ti ? run + 1 : 0;
    s += 10 + run * 5 - Math.min(found - ti, 10);
    ti = found + 1;
  }
  return s;
}

export default function CommandPalette({ items, placeholder, initial = "", onClose }: Props) {
  const [query, setQuery] = useState(initial);
  const [index, setIndex] = useState(0);
  const listRef = useRef<HTMLDivElement>(null);

  const results = useMemo(() => {
    return items
      .map((item) => ({ item, s: Math.max(score(query, item.title), score(query, `${item.group} ${item.title} ${item.subtitle ?? ""}`) * 0.8) }))
      .filter((r) => r.s > 0)
      .sort((a, b) => b.s - a.s)
      .slice(0, 60)
      .map((r) => r.item);
  }, [items, query]);

  function run(item: PaletteItem | undefined) {
    if (!item) return;
    onClose();
    // let the palette close (and give focus back) before the action runs
    setTimeout(() => item.run(), 0);
  }

  function onKey(e: React.KeyboardEvent) {
    if (e.key === "Escape") {
      e.preventDefault();
      onClose();
    } else if (e.key === "ArrowDown") {
      e.preventDefault();
      setIndex((i) => Math.min(i + 1, results.length - 1));
      listRef.current?.children[Math.min(index + 1, results.length - 1)]?.scrollIntoView({ block: "nearest" });
    } else if (e.key === "ArrowUp") {
      e.preventDefault();
      setIndex((i) => Math.max(i - 1, 0));
      listRef.current?.children[Math.max(index - 1, 0)]?.scrollIntoView({ block: "nearest" });
    } else if (e.key === "Enter") {
      e.preventDefault();
      run(results[index]);
    }
  }

  return (
    <div className="overlay top" onMouseDown={(e) => e.target === e.currentTarget && onClose()}>
      <div className="palette" onKeyDown={onKey}>
        <input
          autoFocus
          value={query}
          placeholder={placeholder ?? "Type a command, device, snippet or past command"}
          onChange={(e) => {
            setQuery(e.target.value);
            setIndex(0);
          }}
          spellCheck={false}
        />
        <div className="palette-list" ref={listRef}>
          {results.length === 0 && <div className="palette-empty">Nothing matches</div>}
          {results.map((item, i) => (
            <div
              key={item.id}
              className={`palette-item${i === index ? " on" : ""}`}
              onMouseMove={() => setIndex(i)}
              onMouseDown={(e) => {
                e.preventDefault();
                run(item);
              }}
            >
              <span className="group">{item.group}</span>
              <span className="title">{item.title}</span>
              {item.subtitle && <span className="subtitle">{item.subtitle}</span>}
              {item.keys && <kbd>{item.keys}</kbd>}
            </div>
          ))}
        </div>
      </div>
    </div>
  );
}

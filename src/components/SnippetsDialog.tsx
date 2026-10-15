import { useState } from "react";
import type { Snippet } from "../lib/types";

interface Props {
  snippets: Snippet[];
  canSend: boolean;
  onChange: (snippets: Snippet[]) => void;
  onSend: (snippet: Snippet) => void;
  onClose: () => void;
}

/** Saved commands. A snippet is typed into the active session as is;
 *  every line ends with Enter, and multi-line snippets ask first. */
export default function SnippetsDialog({ snippets, canSend, onChange, onSend, onClose }: Props) {
  const [selected, setSelected] = useState<string | null>(snippets[0]?.id ?? null);
  const [filter, setFilter] = useState("");
  const current = snippets.find((s) => s.id === selected) ?? null;
  const shown = snippets
    .filter((s) => !filter || `${s.name} ${s.body} ${s.description}`.toLowerCase().includes(filter.toLowerCase()))
    .sort((a, b) => a.name.localeCompare(b.name));

  function update(patch: Partial<Snippet>) {
    if (!current) return;
    onChange(snippets.map((s) => (s.id === current.id ? { ...s, ...patch } : s)));
  }

  function add() {
    const s: Snippet = { id: crypto.randomUUID(), name: "New snippet", body: "", description: "" };
    onChange([...snippets, s]);
    setSelected(s.id);
  }

  function remove() {
    if (!current) return;
    const rest = snippets.filter((s) => s.id !== current.id);
    onChange(rest);
    setSelected(rest[0]?.id ?? null);
  }

  return (
    <div className="overlay" onMouseDown={(e) => e.target === e.currentTarget && onClose()}>
      <div className="dialog xwide" onKeyDown={(e) => e.key === "Escape" && onClose()}>
        <h3>Snippets</h3>
        <div className="split-editor">
          <div className="split-list">
            <input className="search" placeholder="Search snippets" value={filter} onChange={(e) => setFilter(e.target.value)} spellCheck={false} />
            <div className="list">
              {shown.map((s) => (
                <div key={s.id} className={`list-row${s.id === selected ? " selected" : ""}`} onClick={() => setSelected(s.id)} onDoubleClick={() => canSend && onSend(s)}>
                  <span className="name">{s.name}</span>
                  <span className="target">{s.body.split("\n")[0]}</span>
                </div>
              ))}
              {snippets.length === 0 && <div className="sidebar-empty">No snippets yet.</div>}
            </div>
            <button className="btn" onClick={add}>New snippet</button>
          </div>
          <div className="split-body">
            {current ? (
              <div className="form" style={{ gridTemplateColumns: "90px 1fr" }}>
                <label>Name</label>
                <input value={current.name} onChange={(e) => update({ name: e.target.value })} />
                <label>Description</label>
                <input value={current.description} onChange={(e) => update({ description: e.target.value })} placeholder="Optional" />
                <label>Commands</label>
                <textarea className="code" rows={10} value={current.body} onChange={(e) => update({ body: e.target.value })} placeholder={"show interfaces status\nshow ip interface brief"} spellCheck={false} />
                <div className="hint">One command per line. Each line is sent followed by Enter.</div>
              </div>
            ) : (
              <div className="sidebar-empty">Pick or create a snippet.</div>
            )}
          </div>
        </div>
        <div className="dialog-actions">
          {current && <button className="btn danger left" onClick={remove}>Delete</button>}
          <button className="btn" onClick={onClose}>Close</button>
          <button className="btn primary" disabled={!current || !canSend || !current.body.trim()} onClick={() => current && onSend(current)}>
            Send to session
          </button>
        </div>
      </div>
    </div>
  );
}

import { useState } from "react";

interface Props {
  title: string;
  placeholder?: string;
  initial?: string;
  confirm: string;
  onResult: (name: string | null) => void;
}

/** One text field: layout names and the like. */
export default function NameDialog({ title, placeholder, initial = "", confirm, onResult }: Props) {
  const [value, setValue] = useState(initial);
  return (
    <div className="overlay" onMouseDown={(e) => e.target === e.currentTarget && onResult(null)}>
      <form className="dialog" onSubmit={(e) => { e.preventDefault(); if (value.trim()) onResult(value.trim()); }} onKeyDown={(e) => e.key === "Escape" && onResult(null)}>
        <h3>{title}</h3>
        <div className="form" style={{ gridTemplateColumns: "1fr" }}>
          <input autoFocus value={value} onChange={(e) => setValue(e.target.value)} placeholder={placeholder} spellCheck={false} />
        </div>
        <div className="dialog-actions">
          <button type="button" className="btn" onClick={() => onResult(null)}>Cancel</button>
          <button type="submit" className="btn primary" disabled={!value.trim()}>{confirm}</button>
        </div>
      </form>
    </div>
  );
}

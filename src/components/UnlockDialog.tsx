import { useState } from "react";
import { vaultUnlock, type VaultStatus } from "../lib/credentials";

interface Props {
  /** why the vault is needed right now */
  reason?: string;
  /** label of the alternative to unlocking, when there is one */
  skipLabel?: string;
  onUnlocked: (status: VaultStatus) => void;
  onCancel: () => void;
}

/** Vault passphrase prompt. Wrong attempts are counted by the backend. */
export default function UnlockDialog({ reason, skipLabel, onUnlocked, onCancel }: Props) {
  const [value, setValue] = useState("");
  const [error, setError] = useState("");
  const [busy, setBusy] = useState(false);

  async function submit(e: React.FormEvent) {
    e.preventDefault();
    if (!value) return;
    setBusy(true);
    try {
      onUnlocked(await vaultUnlock(value));
    } catch (err) {
      setError(String(err));
      setValue("");
    } finally {
      setBusy(false);
    }
  }

  return (
    <div className="overlay" onMouseDown={(e) => e.target === e.currentTarget && onCancel()}>
      <form className="dialog" onSubmit={submit} onKeyDown={(e) => e.key === "Escape" && onCancel()}>
        <h3>Unlock the credential vault</h3>
        {reason && <p>{reason}</p>}
        <div className="form" style={{ gridTemplateColumns: "1fr" }}>
          <input autoFocus type="password" value={value} onChange={(e) => { setValue(e.target.value); setError(""); }} placeholder="Vault passphrase" autoComplete="current-password" />
          {error && <div className="hint warn-text" style={{ gridColumn: 1 }}>{error}</div>}
        </div>
        <div className="dialog-actions">
          <button type="button" className="btn" onClick={onCancel}>{skipLabel ?? "Cancel"}</button>
          <button type="submit" className="btn primary" disabled={!value || busy}>Unlock</button>
        </div>
      </form>
    </div>
  );
}

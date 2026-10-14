import { useState } from "react";
import type { PendingPrompt } from "../lib/terminal";

interface Props {
  prompt: PendingPrompt;
  onAnswer: (value: string | null) => void;
}

/** Questions a connecting session asks: password, username, host key. */
export default function PromptDialog({ prompt, onAnswer }: Props) {
  const [value, setValue] = useState("");
  const hostKey = prompt.kind === "hostKeyNew" || prompt.kind === "hostKeyChanged";
  const changed = prompt.kind === "hostKeyChanged";

  const title =
    prompt.kind === "hostKeyNew" ? "Trust this device?"
    : changed ? "Host key changed"
    : prompt.kind === "password" ? "Password"
    : prompt.kind === "username" ? "Username"
    : "Login";

  function submit(e: React.FormEvent) {
    e.preventDefault();
    onAnswer(hostKey ? "yes" : value);
  }

  return (
    <div className="overlay">
      <form className={`dialog${changed ? " warning" : ""}`} onSubmit={submit} onKeyDown={(e) => e.key === "Escape" && onAnswer(null)}>
        <h3>{title}</h3>
        <p>{prompt.message}</p>
        {prompt.fingerprint && <div className="fingerprint">{prompt.fingerprint}</div>}
        {!hostKey && (
          <div className="form" style={{ gridTemplateColumns: "1fr" }}>
            <input
              autoFocus
              type={prompt.echo ? "text" : "password"}
              value={value}
              onChange={(e) => setValue(e.target.value)}
              spellCheck={false}
            />
          </div>
        )}
        <div className="dialog-actions">
          <button type="button" className="btn" onClick={() => onAnswer(null)} autoFocus={changed}>Cancel</button>
          <button type="submit" className={`btn ${changed ? "danger" : "primary"}`} autoFocus={hostKey && !changed}>
            {prompt.kind === "hostKeyNew" ? "Trust and connect" : changed ? "Trust the new key" : "Log in"}
          </button>
        </div>
      </form>
    </div>
  );
}

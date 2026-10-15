import { useEffect, useState } from "react";
import { deleteSecret, setSecret } from "../lib/backend";
import { aiModels, newProvider, PROVIDER_DEFAULT_URL, PROVIDER_LABEL, type AiSettings, type Provider, type ProviderKind } from "../lib/ai";
import { TrashIcon } from "./Icons";

interface Props {
  value: AiSettings;
  onChange: (v: AiSettings) => void;
}

/** Settings → Assistant: providers, models, keys (keychain) and what is shared. */
export default function AiSettingsSection({ value, onChange }: Props) {
  const [models, setModels] = useState<Record<string, string[] | string>>({});
  const [key, setKey] = useState<Record<string, string>>({});
  const active = value.providers.find((p) => p.id === value.activeId) ?? value.providers[0];

  useEffect(() => {
    if (active && value.activeId !== active.id) onChange({ ...value, activeId: active.id });
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [active?.id]);

  const patch = (id: string, p: Partial<Provider>) => onChange({ ...value, providers: value.providers.map((x) => (x.id === id ? { ...x, ...p } : x)) });

  function add(kind: ProviderKind) {
    const p = newProvider(kind);
    onChange({ ...value, providers: [...value.providers, p], activeId: p.id });
  }

  async function remove(p: Provider) {
    if (p.hasKey) await deleteSecret("apikey", p.id).catch(() => {});
    const providers = value.providers.filter((x) => x.id !== p.id);
    onChange({ ...value, providers, activeId: providers[0]?.id ?? "" });
  }

  async function test(p: Provider) {
    setModels((m) => ({ ...m, [p.id]: "…" }));
    try {
      const list = await aiModels(p);
      setModels((m) => ({ ...m, [p.id]: list }));
      if (!p.model && list.length) patch(p.id, { model: list[0] });
    } catch (e) {
      setModels((m) => ({ ...m, [p.id]: `✗ ${String(e)}` }));
    }
  }

  async function saveKey(p: Provider) {
    const k = key[p.id]?.trim();
    if (!k) return;
    await setSecret("apikey", p.id, k);
    setKey((x) => ({ ...x, [p.id]: "" }));
    patch(p.id, { hasKey: true });
  }

  const needsKey = (k: ProviderKind) => k === "openai" || k === "anthropic";

  return (
    <div className="form ai-form">
      <label>Provider</label>
      <div className="row">
        <select value={active?.id ?? ""} onChange={(e) => onChange({ ...value, activeId: e.target.value })}>
          {value.providers.length === 0 && <option value="">none yet</option>}
          {value.providers.map((p) => <option key={p.id} value={p.id}>{p.name}</option>)}
        </select>
        <select value="" onChange={(e) => e.target.value && add(e.target.value as ProviderKind)} title="Add a provider">
          <option value="">Add…</option>
          {(Object.keys(PROVIDER_LABEL) as ProviderKind[]).map((k) => <option key={k} value={k}>{PROVIDER_LABEL[k]}</option>)}
        </select>
      </div>
      {active && (
        <>
          <label>Name</label>
          <div className="row">
            <input value={active.name} onChange={(e) => patch(active.id, { name: e.target.value })} spellCheck={false} />
            <button type="button" className="icon-btn" title="Remove this provider" onClick={() => void remove(active)}><TrashIcon /></button>
          </div>
          <label>Type</label>
          <div className="quiet">{PROVIDER_LABEL[active.kind]}{active.kind === "ollama" || active.kind === "lmstudio" ? " · runs on this computer, nothing leaves it" : " · output you share is sent to this service"}</div>
          <label>Base URL</label>
          <input value={active.baseUrl} placeholder={PROVIDER_DEFAULT_URL[active.kind] || "https://host/v1 compatible endpoint"} onChange={(e) => patch(active.id, { baseUrl: e.target.value })} spellCheck={false} />
          {(needsKey(active.kind) || active.kind === "compatible") && (
            <>
              <label>API key</label>
              <div className="row">
                <input
                  type="password"
                  value={key[active.id] ?? ""}
                  placeholder={active.hasKey ? "saved in the OS keychain (enter to replace)" : needsKey(active.kind) ? "required" : "optional"}
                  onChange={(e) => setKey((x) => ({ ...x, [active.id]: e.target.value }))}
                  onBlur={() => void saveKey(active)}
                  onKeyDown={(e) => e.key === "Enter" && void saveKey(active)}
                  autoComplete="off"
                />
                {active.hasKey && (
                  <button type="button" className="btn" onClick={() => void deleteSecret("apikey", active.id).then(() => patch(active.id, { hasKey: false }))}>Forget</button>
                )}
              </div>
            </>
          )}
          <label>Model</label>
          <div className="row">
            {Array.isArray(models[active.id]) && (models[active.id] as string[]).length > 0 ? (
              <select value={active.model} onChange={(e) => patch(active.id, { model: e.target.value })}>
                {!(models[active.id] as string[]).includes(active.model) && active.model && <option value={active.model}>{active.model}</option>}
                {(models[active.id] as string[]).map((m) => <option key={m} value={m}>{m}</option>)}
              </select>
            ) : (
              <input value={active.model} placeholder={active.kind === "ollama" ? "qwen3:8b" : active.kind === "anthropic" ? "claude-sonnet-4-5" : "model name"} onChange={(e) => patch(active.id, { model: e.target.value })} spellCheck={false} />
            )}
            <button type="button" className="btn" onClick={() => void test(active)}>Test and list models</button>
          </div>
          {typeof models[active.id] === "string" && <div className={`hint${(models[active.id] as string).startsWith("✗") ? " warn-text" : ""}`}>{models[active.id] as string}</div>}
          {Array.isArray(models[active.id]) && <div className="hint">Connected. {(models[active.id] as string[]).length} model{(models[active.id] as string[]).length === 1 ? "" : "s"} available.</div>}
        </>
      )}
      <label>Sharing</label>
      <label className="check"><input type="checkbox" checked={value.shareOutput} onChange={(e) => onChange({ ...value, shareOutput: e.target.checked })} />Send terminal output and selections to the assistant</label>
      <label className="check"><input type="checkbox" checked={value.shareConfig} onChange={(e) => onChange({ ...value, shareConfig: e.target.checked })} />Allow configuration text in requests</label>
      <label className="check"><input type="checkbox" checked={value.shareDeviceNames} onChange={(e) => onChange({ ...value, shareDeviceNames: e.target.checked })} />Include device host names</label>
      <div className="hint">Passwords, secrets, SNMP communities, keys and certificate bodies are always replaced with &lt;removed&gt; before a request leaves the app, whatever the settings above. Local providers (Ollama, LM Studio) keep everything on this computer. Suggested local model: Qwen3 8B.</div>
    </div>
  );
}

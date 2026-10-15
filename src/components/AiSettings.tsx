import { useEffect, useRef, useState } from "react";
import { deleteSecret, setSecret } from "../lib/backend";
import { aiModels, builtinProvider, newProvider, PROVIDER_DEFAULT_URL, PROVIDER_LABEL, type AiSettings, type Provider, type ProviderKind } from "../lib/ai";
import { downloads, engineStop, formatBytes, localStatus, modelAddCustom, modelCancel, modelDelete, modelDownload, onDownload, type LocalStatus } from "../lib/local";
import { TrashIcon } from "./Icons";

interface Props {
  value: AiSettings;
  onChange: (v: AiSettings) => void;
}

/** Settings → Assistant: providers, models, keys (keychain) and what is shared. */
export default function AiSettingsSection({ value, onChange }: Props) {
  const [models, setModels] = useState<Record<string, string[] | string>>({});
  const [key, setKey] = useState<Record<string, string>>({});
  const [local, setLocal] = useState<LocalStatus | null>(null);
  const [, setTick] = useState(0);
  const [localError, setLocalError] = useState("");
  const active = value.providers.find((p) => p.id === value.activeId) ?? value.providers[0];
  const valueRef = useRef(value);
  valueRef.current = value;

  const reloadLocal = () => localStatus().then(setLocal).catch((e) => setLocalError(String(e)));
  useEffect(() => {
    void reloadLocal();
    let off: (() => void) | undefined;
    void onDownload((e) => {
      if (e.type === "progress") downloads.set(e.id, { received: e.received, total: e.total, bytesPerSec: e.bytesPerSec });
      else downloads.delete(e.id);
      if (e.type === "done") {
        void reloadLocal();
        selectBuiltin(e.id);
      }
      if (e.type === "error") setLocalError(e.message);
      setTick((t) => t + 1);
    }).then((fn) => { off = fn; });
    return () => off?.();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  /** Use the built-in runtime with this model (the provider is created on first use). */
  function selectBuiltin(modelId: string) {
    const cur = valueRef.current;
    const existing = builtinProvider(cur);
    const p: Provider = existing ? { ...existing, model: modelId } : { ...newProvider("builtin"), name: "Built-in", model: modelId };
    const providers = existing ? cur.providers.map((x) => (x.id === p.id ? p : x)) : [...cur.providers, p];
    onChange({ ...cur, providers, activeId: p.id });
  }

  async function download(id: string) {
    setLocalError("");
    try {
      await modelDownload(id);
    } catch (e) {
      if (!/cancelled/.test(String(e))) setLocalError(String(e));
    }
  }

  async function removeModel(id: string) {
    setLocalError("");
    try {
      await modelDelete(id);
      await reloadLocal();
      const b = builtinProvider(value);
      if (b && b.model === id) onChange({ ...value, providers: value.providers.map((p) => (p.id === b.id ? { ...p, model: "" } : p)) });
    } catch (e) {
      setLocalError(String(e));
    }
  }

  async function addCustom() {
    setLocalError("");
    try {
      const m = await modelAddCustom();
      if (m) {
        await reloadLocal();
        selectBuiltin(m.id);
      }
    } catch (e) {
      setLocalError(String(e));
    }
  }

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
  const builtin = builtinProvider(value);
  const activeModel = builtin && value.activeId === builtin.id ? builtin.model : "";

  return (
    <div className="form ai-form">
      <label>Built-in</label>
      <div className="models">
        {local ? (
          <>
            <div className="quiet small">{local.runtime.available ? `Runs on this computer with llama.cpp ${local.runtime.version}; nothing leaves it. Detected: ${local.hardware.summary}.` : local.runtime.note}</div>
            {local.runtime.available && local.models.map((m) => {
              const p = downloads.get(m.id);
              const selected = activeModel === m.id;
              return (
                <div key={m.id} className={`model-row${selected ? " selected" : ""}`}>
                  <div className="model-main">
                    <span className="strong">{m.name}</span>
                    {m.id === local.recommended && <span className="chip">recommended for this computer</span>}
                    {selected && <span className="chip on">in use</span>}
                    <span className="quiet small">{m.custom ? m.path : `${formatBytes(m.sizeBytes)} · needs about ${m.needsGb} GB of GPU memory`}</span>
                    <div className="quiet small">{m.blurb}</div>
                    {p && (
                      <div className="download">
                        <progress value={p.received} max={p.total || 1} />
                        <span className="quiet small">{formatBytes(p.received)} of {formatBytes(p.total)} · {formatBytes(p.bytesPerSec)}/s</span>
                      </div>
                    )}
                  </div>
                  <div className="model-actions">
                    {p ? (
                      <button type="button" className="btn small" onClick={() => void modelCancel(m.id)}>Cancel</button>
                    ) : m.downloaded ? (
                      <>
                        {!selected && <button type="button" className="btn small primary" onClick={() => selectBuiltin(m.id)}>Use</button>}
                        <button type="button" className="icon-btn" title={m.custom ? "Forget this file (it stays on disk)" : "Delete the downloaded file"} onClick={() => void removeModel(m.id)}><TrashIcon /></button>
                      </>
                    ) : (
                      <button type="button" className="btn small" onClick={() => void download(m.id)}>Download</button>
                    )}
                  </div>
                </div>
              );
            })}
            {local.runtime.available && (
              <div className="row">
                <button type="button" className="btn small" onClick={() => void addCustom()}>Add a .gguf file…</button>
                {local.engine.running && <button type="button" className="btn small" title="Free the GPU memory until the next question" onClick={() => void engineStop().then(reloadLocal)}>Unload the running model</button>}
                <span className="quiet small">Files live in {local.modelsDir}</span>
              </div>
            )}
            {localError && <div className="hint warn-text">{localError}</div>}
          </>
        ) : (
          <div className="quiet small">{localError || "Checking this computer…"}</div>
        )}
      </div>
      <label>Provider</label>
      <div className="row">
        <select value={active?.id ?? ""} onChange={(e) => onChange({ ...value, activeId: e.target.value })}>
          {value.providers.length === 0 && <option value="">none yet</option>}
          {value.providers.map((p) => <option key={p.id} value={p.id}>{p.name}</option>)}
        </select>
        <select value="" onChange={(e) => e.target.value && add(e.target.value as ProviderKind)} title="Add a provider">
          <option value="">Add…</option>
          {(Object.keys(PROVIDER_LABEL) as ProviderKind[]).filter((k) => k !== "builtin").map((k) => <option key={k} value={k}>{PROVIDER_LABEL[k]}</option>)}
        </select>
      </div>
      {active && active.kind === "builtin" && (
        <>
          <label>Type</label>
          <div className="quiet">Built-in runtime · model {local?.models.find((m) => m.id === active.model)?.name ?? active.model ?? "not chosen"} · pick another in the list above</div>
        </>
      )}
      {active && active.kind !== "builtin" && (
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
      <label>Autonomy</label>
      <label className="check"><input type="checkbox" checked={value.autoRun ?? true} onChange={(e) => onChange({ ...value, autoRun: e.target.checked })} />Let the assistant run read-only show commands on its own while answering (switch it off per device from the assistant's header)</label>
      <div className="hint">Passwords, secrets, SNMP communities, keys and certificate bodies are always replaced with &lt;removed&gt; before a request leaves the app, whatever the settings above. The built-in runtime, Ollama and LM Studio keep everything on this computer. Configuration changes always wait for your review, whatever the assistant runs on its own.</div>
    </div>
  );
}

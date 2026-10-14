import { useEffect, useRef, useState } from "react";
import { answerSession } from "../lib/backend";
import {
  fileSize, filesClose, filesDelete, filesDownload, filesList, filesMkdir, filesOpen, filesRename, filesUpload, parentPath,
  type Entry, type Opened, type Transfer,
} from "../lib/files";
import type { SessionSpec } from "../lib/terminal";
import type { ConnectSpec } from "../lib/types";
import type { PendingPrompt } from "../lib/terminal";
import PromptDialog from "./PromptDialog";
import { FolderIcon, RefreshIcon, TrashIcon, UpIcon } from "./Icons";

interface Props {
  tabKey: string;
  /** the active tab's connection, reused for the file connection */
  spec: SessionSpec | null;
  deviceName: string;
  onToast: (t: string) => void;
}

interface FilesState {
  conn: Opened | null;
  connecting: boolean;
  error: string | null;
  path: string;
  entries: Entry[];
  loading: boolean;
  transfer: { label: string; done: number; total: number } | null;
  /** SCP mode: the remote path typed for a download or upload */
  scpPath: string;
}

const cache = new Map<string, FilesState>();

function fresh(): FilesState {
  return { conn: null, connecting: false, error: null, path: "", entries: [], loading: false, transfer: null, scpPath: "flash:/" };
}

function when(ts: number | null): string {
  if (!ts) return "";
  const d = new Date(ts * 1000);
  return `${d.toLocaleDateString()} ${d.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" })}`;
}

/** SFTP browser, or SCP get/put for devices without the subsystem. */
export default function FilesPanel({ tabKey, spec, deviceName, onToast }: Props) {
  const [, setTick] = useState(0);
  const [prompt, setPrompt] = useState<{ id: string; prompt: PendingPrompt } | null>(null);
  const [renaming, setRenaming] = useState<{ entry: Entry; name: string } | null>(null);
  const [newDir, setNewDir] = useState<string | null>(null);
  const pathInput = useRef<HTMLInputElement>(null);
  if (!cache.has(tabKey)) cache.set(tabKey, fresh());
  const st = cache.get(tabKey)!;
  const refresh = () => setTick((t) => t + 1);
  const patch = (p: Partial<FilesState>) => {
    Object.assign(st, p);
    refresh();
  };

  useEffect(() => () => setPrompt(null), [tabKey]);

  async function connect() {
    if (!spec || spec.protocol !== "ssh") {
      onToast("File transfer needs an SSH device");
      return;
    }
    const id = crypto.randomUUID();
    patch({ connecting: true, error: null });
    try {
      const full: ConnectSpec = { ...spec, cols: 80, rows: 24, log: false };
      const conn = await filesOpen(id, full, (e) => {
        if (e.type === "prompt") setPrompt({ id, prompt: { kind: e.kind, message: e.message, fingerprint: e.fingerprint, echo: e.echo } });
        else if (e.type === "status" && (e.state === "closed" || e.state === "error")) {
          setPrompt(null);
          if (st.conn?.id === id) patch({ conn: null, entries: [], error: e.state === "error" ? e.message : null });
        }
      });
      setPrompt(null);
      patch({ conn, connecting: false, path: conn.home });
      if (conn.sftp) await list(conn.id, conn.home);
    } catch (e) {
      setPrompt(null);
      patch({ connecting: false, error: String(e) });
    }
  }

  async function disconnect() {
    if (st.conn) void filesClose(st.conn.id);
    patch({ conn: null, entries: [], transfer: null });
  }

  async function list(id: string, path: string) {
    patch({ loading: true, error: null });
    try {
      const entries = await filesList(id, path);
      patch({ entries, path: path || st.path, loading: false });
    } catch (e) {
      patch({ loading: false, error: String(e) });
    }
  }

  const onProgress = (label: string) => (t: Transfer) => {
    if (t.type === "progress") patch({ transfer: { label, done: t.done, total: t.total } });
  };

  async function download(remote: string) {
    if (!st.conn) return;
    const name = remote.split(/[/:]/).pop() || remote;
    try {
      const local = await filesDownload(st.conn.id, remote, onProgress(`Downloading ${name}`));
      patch({ transfer: null });
      if (local) onToast(`Saved ${local}`);
    } catch (e) {
      patch({ transfer: null });
      onToast(`Download failed: ${e}`);
    }
  }

  async function upload(remoteDir: string) {
    if (!st.conn) return;
    try {
      const remote = await filesUpload(st.conn.id, remoteDir, onProgress("Uploading"));
      patch({ transfer: null });
      if (remote) {
        onToast(`Uploaded to ${remote}`);
        if (st.conn.sftp) await list(st.conn.id, st.path);
      }
    } catch (e) {
      patch({ transfer: null });
      onToast(`Upload failed: ${e}`);
    }
  }

  async function remove(entry: Entry) {
    if (!st.conn) return;
    if (!window.confirm(`Delete ${entry.name} on ${deviceName}? This cannot be undone.`)) return;
    try {
      await filesDelete(st.conn.id, entry.path, entry.isDir);
      await list(st.conn.id, st.path);
    } catch (e) {
      onToast(`Delete failed: ${e}`);
    }
  }

  async function rename() {
    if (!st.conn || !renaming) return;
    const to = `${parentPath(renaming.entry.path) === "/" ? "" : parentPath(renaming.entry.path)}/${renaming.name}`.replace(/^\/+/, "/");
    try {
      await filesRename(st.conn.id, renaming.entry.path, to);
      setRenaming(null);
      await list(st.conn.id, st.path);
    } catch (e) {
      onToast(`Rename failed: ${e}`);
    }
  }

  async function mkdir() {
    if (!st.conn || !newDir?.trim()) return;
    try {
      await filesMkdir(st.conn.id, `${st.path.replace(/\/+$/, "")}/${newDir.trim()}`);
      setNewDir(null);
      await list(st.conn.id, st.path);
    } catch (e) {
      onToast(`Could not create the folder: ${e}`);
    }
  }

  const conn = st.conn;
  const pct = st.transfer && st.transfer.total > 0 ? Math.min(100, Math.round((st.transfer.done / st.transfer.total) * 100)) : null;

  return (
    <div className="tools-body files">
      {prompt && <PromptDialog key={prompt.prompt.kind + prompt.prompt.message} prompt={prompt.prompt} onAnswer={(v) => { setPrompt(null); void answerSession(prompt.id, v); }} />}
      {!conn ? (
        <div className="tools-empty">
          <FolderIcon />
          <p>Browse and transfer files on <b>{deviceName || "the device"}</b> over a second SSH connection with the same login. SFTP is used when the device has it (IOS-XE, NX-OS, Linux hosts); classic IOS falls back to SCP, which needs <code>ip scp server enable</code> on the switch.</p>
          <button className="btn primary" disabled={!spec || spec.protocol !== "ssh" || st.connecting} onClick={() => void connect()}>{st.connecting ? "Connecting…" : "Connect for files"}</button>
          {spec && spec.protocol !== "ssh" && <div className="quiet small">This tab is not SSH.</div>}
          {st.error && <div className="tools-error">{st.error}</div>}
        </div>
      ) : conn.sftp ? (
        <>
          <div className="tools-actions">
            <button className="icon-btn" title="Up one folder" disabled={st.loading} onClick={() => void list(conn.id, parentPath(st.path))}><UpIcon /></button>
            <form className="grow" onSubmit={(e) => { e.preventDefault(); void list(conn.id, pathInput.current?.value ?? st.path); }}>
              <input ref={pathInput} key={st.path} defaultValue={st.path} spellCheck={false} className="files-path" title="Remote folder; Enter to open" />
            </form>
            <button className="icon-btn" title="Refresh" disabled={st.loading} onClick={() => void list(conn.id, st.path)}><RefreshIcon /></button>
            <button className="btn small" disabled={!!st.transfer} onClick={() => void upload(st.path)}>Upload here…</button>
            <button className="btn small" onClick={() => setNewDir("")}>New folder</button>
            <button className="btn small" title="Close the file connection" onClick={() => void disconnect()}>Disconnect</button>
          </div>
          <div className="quiet small">SFTP as {conn.user} · {st.entries.length} item{st.entries.length === 1 ? "" : "s"}{st.loading ? " · loading…" : ""}</div>
          {newDir !== null && (
            <form className="row" onSubmit={(e) => { e.preventDefault(); void mkdir(); }}>
              <input autoFocus value={newDir} onChange={(e) => setNewDir(e.target.value)} placeholder="Folder name" spellCheck={false} />
              <button className="btn small primary" type="submit" disabled={!newDir.trim()}>Create</button>
              <button className="btn small" type="button" onClick={() => setNewDir(null)}>Cancel</button>
            </form>
          )}
          {st.error && <div className="tools-error">{st.error}</div>}
          <div className="data-table files-table">
            <div className="data-table-scroll">
              <table>
                <thead><tr><th>Name</th><th>Size</th><th>Modified</th><th></th></tr></thead>
                <tbody>
                  {st.entries.map((e) => (
                    <tr key={e.path} className={e.isDir ? "dir" : ""} onDoubleClick={() => (e.isDir ? void list(conn.id, e.path) : void download(e.path))}>
                      <td className="name">
                        {renaming?.entry.path === e.path ? (
                          <form onSubmit={(ev) => { ev.preventDefault(); void rename(); }}>
                            <input autoFocus value={renaming.name} onChange={(ev) => setRenaming({ ...renaming, name: ev.target.value })} onBlur={() => setRenaming(null)} onKeyDown={(ev) => ev.key === "Escape" && setRenaming(null)} spellCheck={false} />
                          </form>
                        ) : (
                          <>{e.isDir ? <FolderIcon /> : null}<span>{e.name}</span></>
                        )}
                      </td>
                      <td>{e.isDir ? "" : fileSize(e.size)}</td>
                      <td>{when(e.modified)}</td>
                      <td className="acts">
                        {!e.isDir && <button className="link" onClick={() => void download(e.path)} disabled={!!st.transfer}>Download</button>}
                        <button className="link" onClick={() => setRenaming({ entry: e, name: e.name })}>Rename</button>
                        <button className="icon-btn" title="Delete" onClick={() => void remove(e)}><TrashIcon /></button>
                      </td>
                    </tr>
                  ))}
                  {st.entries.length === 0 && !st.loading && <tr><td colSpan={4} className="quiet">Empty folder</td></tr>}
                </tbody>
              </table>
            </div>
          </div>
        </>
      ) : (
        <>
          <div className="tools-note inline">No SFTP on this device, so transfers use SCP by path. Type the remote file to fetch, or the folder (ending in <code>/</code>) or full path to put a file at.</div>
          <div className="row">
            <input className="grow" value={st.scpPath} onChange={(e) => patch({ scpPath: e.target.value })} placeholder="flash:/c2960x-universalk9-mz.bin" spellCheck={false} />
          </div>
          <div className="row">
            <button className="btn small primary" disabled={!st.scpPath.trim() || !!st.transfer || st.scpPath.trim().endsWith("/") || st.scpPath.trim().endsWith(":")} onClick={() => void download(st.scpPath.trim())}>Download</button>
            <button className="btn small" disabled={!st.scpPath.trim() || !!st.transfer} onClick={() => void upload(st.scpPath.trim())}>Upload a file to this path…</button>
            <span className="grow" />
            <button className="btn small" onClick={() => void disconnect()}>Disconnect</button>
          </div>
          <div className="quiet small">SCP as {conn.user}. Common paths: <code>flash:/</code>, <code>bootflash:/</code>, <code>nvram:/startup-config</code>, <code>system:running-config</code>.</div>
        </>
      )}
      {st.transfer && (
        <div className="transfer">
          <div className="row"><span>{st.transfer.label}</span><span className="grow" /><span className="quiet small">{fileSize(st.transfer.done)}{st.transfer.total ? ` of ${fileSize(st.transfer.total)}` : ""}</span></div>
          <div className="bar"><div className="fill" style={{ width: `${pct ?? 0}%` }} /></div>
        </div>
      )}
    </div>
  );
}

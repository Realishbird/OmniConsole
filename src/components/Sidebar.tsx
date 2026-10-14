import { useMemo, useState } from "react";
import type { Device } from "../lib/types";
import { deviceTarget } from "../lib/types";
import { ChevronIcon, PlusIcon } from "./Icons";

interface Props {
  devices: Device[];
  width: number;
  onResize: (width: number) => void;
  onConnect: (device: Device) => void;
  onAdd: (site?: string) => void;
  onEdit: (device: Device) => void;
  onDuplicate: (device: Device) => void;
  onDelete: (device: Device) => void;
}

interface Menu {
  x: number;
  y: number;
  device?: Device;
  site?: string;
}

const NO_SITE = "";

export default function Sidebar(props: Props) {
  const { devices } = props;
  const [filter, setFilter] = useState("");
  const [collapsed, setCollapsed] = useState<Set<string>>(new Set());
  const [selected, setSelected] = useState<string | null>(null);
  const [menu, setMenu] = useState<Menu | null>(null);

  // sites A-Z, devices without a site last
  const groups = useMemo(() => {
    const f = filter.trim().toLowerCase();
    const match = (d: Device) =>
      !f || [d.name, d.host, d.site, d.username, d.serialPort].some((v) => v.toLowerCase().includes(f));
    const map = new Map<string, Device[]>();
    for (const d of devices.filter(match)) {
      const site = d.site.trim();
      map.set(site, [...(map.get(site) ?? []), d]);
    }
    return [...map.entries()]
      .sort(([a], [b]) => (a === NO_SITE ? 1 : b === NO_SITE ? -1 : a.localeCompare(b)))
      .map(([site, list]) => ({ site, list: list.sort((a, b) => a.name.localeCompare(b.name)) }));
  }, [devices, filter]);

  const flat = groups.flatMap((g) => (collapsed.has(g.site) && !filter ? [] : g.list));

  function toggle(site: string) {
    const next = new Set(collapsed);
    if (next.has(site)) next.delete(site);
    else next.add(site);
    setCollapsed(next);
  }

  function onKey(e: React.KeyboardEvent) {
    const i = flat.findIndex((d) => d.id === selected);
    if (e.key === "ArrowDown") {
      e.preventDefault();
      setSelected(flat[Math.min(i + 1, flat.length - 1)]?.id ?? null);
    } else if (e.key === "ArrowUp") {
      e.preventDefault();
      setSelected(flat[Math.max(i - 1, 0)]?.id ?? null);
    } else if (e.key === "Enter") {
      const d = flat[i] ?? flat[0];
      if (d) props.onConnect(d);
    } else if (e.key === "Escape") {
      setFilter("");
    }
  }

  function startResize(e: React.MouseEvent) {
    e.preventDefault();
    const startX = e.clientX;
    const startW = props.width;
    const move = (ev: MouseEvent) => props.onResize(Math.min(520, Math.max(180, startW + ev.clientX - startX)));
    const up = () => {
      window.removeEventListener("mousemove", move);
      window.removeEventListener("mouseup", up);
    };
    window.addEventListener("mousemove", move);
    window.addEventListener("mouseup", up);
  }

  return (
    <aside className="sidebar" style={{ ["--sidebar-width" as string]: `${props.width}px` }} onKeyDown={onKey}>
      <div className="sidebar-head">
        <h2>Devices</h2>
        <button className="icon-btn" title="Add device" onClick={() => props.onAdd()}>
          <PlusIcon />
        </button>
      </div>
      <input
        className="search"
        placeholder="Search devices"
        value={filter}
        onChange={(e) => setFilter(e.target.value)}
        spellCheck={false}
      />
      <div className="device-list" tabIndex={0} onContextMenu={(e) => { e.preventDefault(); setMenu({ x: e.clientX, y: e.clientY }); }}>
        {devices.length === 0 && (
          <div className="sidebar-empty">
            No saved devices yet.
            <br />
            <button className="btn" onClick={() => props.onAdd()}>Add a device</button>
          </div>
        )}
        {devices.length > 0 && groups.length === 0 && <div className="sidebar-empty">No matching devices</div>}
        {groups.map(({ site, list }) => {
          const closed = collapsed.has(site) && !filter;
          return (
            <div key={site || "_none"}>
              {site !== NO_SITE && (
                <div
                  className="site-row"
                  onClick={() => toggle(site)}
                  onContextMenu={(e) => { e.preventDefault(); e.stopPropagation(); setMenu({ x: e.clientX, y: e.clientY, site }); }}
                >
                  <ChevronIcon closed={closed} />
                  {site}
                  <span className="count">{list.length}</span>
                </div>
              )}
              {!closed &&
                list.map((d) => (
                  <div
                    key={d.id}
                    className={`device-row${selected === d.id ? " selected" : ""}`}
                    onClick={() => setSelected(d.id)}
                    onDoubleClick={() => props.onConnect(d)}
                    onContextMenu={(e) => {
                      e.preventDefault();
                      e.stopPropagation();
                      setSelected(d.id);
                      setMenu({ x: e.clientX, y: e.clientY, device: d, site: d.site });
                    }}
                    title={d.notes || undefined}
                  >
                    <span className={`proto ${d.protocol}`}>{d.protocol === "serial" ? "COM" : d.protocol.toUpperCase()}</span>
                    <span className="names">
                      <span className="name">{d.name}</span>
                      <span className="target">{deviceTarget(d)}</span>
                    </span>
                  </div>
                ))}
            </div>
          );
        })}
      </div>
      <div className="resizer" onMouseDown={startResize} />
      {menu && (
        <>
          <div className="overlay" style={{ background: "transparent" }} onMouseDown={() => setMenu(null)} onContextMenu={(e) => { e.preventDefault(); setMenu(null); }} />
          <div className="menu" style={{ left: menu.x, top: menu.y }} onClick={() => setMenu(null)}>
            {menu.device && (
              <>
                <button onClick={() => props.onConnect(menu.device!)}><b>Connect</b></button>
                <button onClick={() => props.onEdit(menu.device!)}>Edit…</button>
                <button onClick={() => props.onDuplicate(menu.device!)}>Duplicate…</button>
                <button className="danger" onClick={() => props.onDelete(menu.device!)}>Delete</button>
                <hr />
              </>
            )}
            <button onClick={() => props.onAdd(menu.site || undefined)}>
              {menu.site ? `Add device to ${menu.site}…` : "Add device…"}
            </button>
          </div>
        </>
      )}
    </aside>
  );
}

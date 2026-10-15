import { useMemo, useState } from "react";
import type { GroupBy } from "../lib/settings";
import { deviceTarget, envInfo, type Device } from "../lib/types";
import { ChevronIcon, MoreIcon, PlusIcon, StarIcon } from "./Icons";

interface Props {
  devices: Device[];
  width: number;
  groupBy: GroupBy;
  onGroupBy: (g: GroupBy) => void;
  onResize: (width: number) => void;
  onConnect: (device: Device) => void;
  onAdd: (preset?: Partial<Device>) => void;
  onEdit: (device: Device) => void;
  onDuplicate: (device: Device) => void;
  onDelete: (device: Device) => void;
  onToggleFavorite: (device: Device) => void;
  onImport: () => void;
  onExport: () => void;
}

interface Menu {
  x: number;
  y: number;
  device?: Device;
  group?: string;
  list?: boolean;
}

const GROUPS: { value: GroupBy; label: string }[] = [
  { value: "site", label: "Site" },
  { value: "customer", label: "Customer" },
  { value: "building", label: "Building" },
  { value: "role", label: "Role" },
  { value: "environment", label: "Environment" },
  { value: "tag", label: "Tag" },
  { value: "none", label: "No grouping" },
];

const UNGROUPED = "";

function groupKeys(d: Device, by: GroupBy): string[] {
  switch (by) {
    case "none":
      return [UNGROUPED];
    case "tag":
      return d.tags.length ? d.tags : [UNGROUPED];
    case "environment":
      return [envInfo(d.environment).value ? envInfo(d.environment).label : UNGROUPED];
    default:
      return [d[by].trim()];
  }
}

/** Search matches every field, and "key:value" filters such as role:core or tag:stack. */
function matcher(filter: string): (d: Device) => boolean {
  const words = filter.trim().toLowerCase().split(/\s+/).filter(Boolean);
  return (d) =>
    words.every((w) => {
      const m = w.match(/^(\w+):(.*)$/);
      if (m) {
        const [, key, val] = m;
        const field =
          key === "tag" ? d.tags.join(" ") :
          key === "env" ? d.environment :
          key in d ? String((d as unknown as Record<string, unknown>)[key] ?? "") : "";
        return field.toLowerCase().includes(val);
      }
      return [d.name, d.host, d.customer, d.site, d.building, d.rack, d.role, d.username, d.serialPort, d.environment, d.notes, ...d.tags]
        .some((v) => v.toLowerCase().includes(w));
    });
}

export default function Sidebar(props: Props) {
  const { devices, groupBy } = props;
  const [filter, setFilter] = useState("");
  const [collapsed, setCollapsed] = useState<Set<string>>(new Set());
  const [selected, setSelected] = useState<string | null>(null);
  const [menu, setMenu] = useState<Menu | null>(null);

  const filtered = useMemo(() => devices.filter(matcher(filter)), [devices, filter]);
  const favorites = useMemo(() => filtered.filter((d) => d.favorite).sort((a, b) => a.name.localeCompare(b.name)), [filtered]);
  const recent = useMemo(
    () => (filter ? [] : devices.filter((d) => d.lastConnected).sort((a, b) => (b.lastConnected ?? 0) - (a.lastConnected ?? 0)).slice(0, 5)),
    [devices, filter],
  );

  // groups A-Z, ungrouped devices last
  const groups = useMemo(() => {
    const map = new Map<string, Device[]>();
    for (const d of filtered) for (const k of groupKeys(d, groupBy)) map.set(k, [...(map.get(k) ?? []), d]);
    return [...map.entries()]
      .sort(([a], [b]) => (a === UNGROUPED ? 1 : b === UNGROUPED ? -1 : a.localeCompare(b)))
      .map(([group, list]) => ({ group, list: list.sort((a, b) => a.name.localeCompare(b.name)) }));
  }, [filtered, groupBy]);

  const sections = [
    ...(favorites.length ? [{ key: "★fav", title: "Favorites", list: favorites }] : []),
    ...(recent.length ? [{ key: "★recent", title: "Recent", list: recent }] : []),
    ...groups.map((g) => ({ key: g.group, title: g.group || (groupBy === "none" ? "" : "Other"), list: g.list })),
  ];
  const flat = sections.flatMap((s) => (collapsed.has(s.key) && !filter ? [] : s.list));

  function toggle(key: string) {
    const next = new Set(collapsed);
    if (next.has(key)) next.delete(key);
    else next.add(key);
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
    const move = (ev: MouseEvent) => props.onResize(Math.min(520, Math.max(200, startW + ev.clientX - startX)));
    const up = () => {
      window.removeEventListener("mousemove", move);
      window.removeEventListener("mouseup", up);
    };
    window.addEventListener("mousemove", move);
    window.addEventListener("mouseup", up);
  }

  function presetFor(group?: string): Partial<Device> | undefined {
    if (!group || group.startsWith("★")) return undefined;
    switch (groupBy) {
      case "site": return { site: group };
      case "customer": return { customer: group };
      case "building": return { building: group };
      case "role": return { role: group };
      case "tag": return { tags: [group] };
      default: return undefined;
    }
  }

  return (
    <aside className="sidebar" style={{ ["--sidebar-width" as string]: `${props.width}px` }} onKeyDown={onKey}>
      <div className="sidebar-head">
        <h2>Devices</h2>
        <button className="icon-btn" title="Add device" onClick={() => props.onAdd()}>
          <PlusIcon />
        </button>
        <button className="icon-btn" title="Group, import, export" onClick={(e) => setMenu({ x: e.clientX, y: e.clientY, list: true })}>
          <MoreIcon />
        </button>
      </div>
      <input
        className="search"
        placeholder="Search  (try role:core or tag:stack)"
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
            <button className="btn" onClick={props.onImport}>Import…</button>
          </div>
        )}
        {devices.length > 0 && filtered.length === 0 && <div className="sidebar-empty">No matching devices</div>}
        {sections.map(({ key, title, list }) => {
          const closed = collapsed.has(key) && !filter;
          return (
            <div key={key || "_none"}>
              {title && (
                <div
                  className={`site-row${key.startsWith("★") ? " special" : ""}`}
                  onClick={() => toggle(key)}
                  onContextMenu={(e) => { e.preventDefault(); e.stopPropagation(); setMenu({ x: e.clientX, y: e.clientY, group: key }); }}
                >
                  <ChevronIcon closed={closed} />
                  {title}
                  <span className="count">{list.length}</span>
                </div>
              )}
              {!closed &&
                list.map((d) => {
                  const env = envInfo(d.environment);
                  return (
                    <div
                      key={key + d.id}
                      className={`device-row${selected === d.id ? " selected" : ""}`}
                      onClick={() => setSelected(d.id)}
                      onDoubleClick={() => props.onConnect(d)}
                      onContextMenu={(e) => {
                        e.preventDefault();
                        e.stopPropagation();
                        setSelected(d.id);
                        setMenu({ x: e.clientX, y: e.clientY, device: d, group: key });
                      }}
                      title={[deviceTarget(d), d.role && `Role: ${d.role}`, d.rack && `Rack: ${d.rack}`, d.tags.length ? `Tags: ${d.tags.join(", ")}` : "", d.notes].filter(Boolean).join("\n")}
                    >
                      <span className={`proto ${d.protocol}`}>{d.protocol === "serial" ? "COM" : d.protocol.toUpperCase()}</span>
                      <span className="names">
                        <span className="name">{d.name}</span>
                        <span className="target">{deviceTarget(d)}{d.role ? ` · ${d.role}` : ""}</span>
                      </span>
                      {env.short && <span className="env-badge" style={{ ["--env" as string]: env.color }}>{env.short}</span>}
                      <button
                        className={`star${d.favorite ? " on" : ""}`}
                        title={d.favorite ? "Remove from favorites" : "Add to favorites"}
                        onClick={(e) => { e.stopPropagation(); props.onToggleFavorite(d); }}
                        onDoubleClick={(e) => e.stopPropagation()}
                      >
                        <StarIcon filled={d.favorite} />
                      </button>
                    </div>
                  );
                })}
            </div>
          );
        })}
      </div>
      <div className="resizer" onMouseDown={startResize} />
      {menu && (
        <>
          <div className="overlay" style={{ background: "transparent" }} onMouseDown={() => setMenu(null)} onContextMenu={(e) => { e.preventDefault(); setMenu(null); }} />
          <div className="menu" style={{ left: Math.min(menu.x, window.innerWidth - 220), top: Math.min(menu.y, window.innerHeight - 300) }} onClick={() => setMenu(null)}>
            {menu.device && (
              <>
                <button onClick={() => props.onConnect(menu.device!)}><b>Connect</b></button>
                <button onClick={() => props.onToggleFavorite(menu.device!)}>{menu.device.favorite ? "Remove from favorites" : "Add to favorites"}</button>
                <button onClick={() => props.onEdit(menu.device!)}>Edit…</button>
                <button onClick={() => props.onDuplicate(menu.device!)}>Duplicate…</button>
                <button className="danger" onClick={() => props.onDelete(menu.device!)}>Delete</button>
                <hr />
              </>
            )}
            {menu.list ? (
              <>
                <div className="menu-label">Group by</div>
                {GROUPS.map((g) => (
                  <button key={g.value} className={groupBy === g.value ? "checked" : ""} onClick={() => props.onGroupBy(g.value)}>
                    {g.label}
                  </button>
                ))}
                <hr />
                <button onClick={props.onImport}>Import devices (JSON or CSV)…</button>
                <button onClick={props.onExport}>Export devices…</button>
              </>
            ) : (
              <button onClick={() => props.onAdd(presetFor(menu.group))}>
                {presetFor(menu.group) ? `Add device to ${menu.group}…` : "Add device…"}
              </button>
            )}
          </div>
        </>
      )}
    </aside>
  );
}

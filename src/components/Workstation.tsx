import { useState } from "react";
import { COMMON_PORTS, netCancel, netDns, netMtu, netPing, netPorts, netTraceroute, parsePorts, split, subnet, type DnsResult, type Line, type PortResult } from "../lib/net";
import { StopIcon } from "./Icons";

type Tool = "ping" | "trace" | "dns" | "ports" | "mtu" | "subnet";

const TOOLS: { id: Tool; title: string }[] = [
  { id: "ping", title: "Ping" }, { id: "trace", title: "Traceroute" }, { id: "dns", title: "DNS" },
  { id: "ports", title: "Port check" }, { id: "mtu", title: "MTU" }, { id: "subnet", title: "Subnet" },
];

/** Tools that run from this computer, not from the device. */
export default function Workstation({ defaultTarget, onToast }: { defaultTarget: string; onToast: (t: string) => void }) {
  const [tool, setTool] = useState<Tool>("ping");
  const [target, setTarget] = useState(defaultTarget);
  const [count, setCount] = useState(4);
  const [size, setSize] = useState("");
  const [hops, setHops] = useState(30);
  const [record, setRecord] = useState("A");
  const [server, setServer] = useState("");
  const [ports, setPorts] = useState(COMMON_PORTS);
  const [lines, setLines] = useState<string[]>([]);
  const [summary, setSummary] = useState<string | null>(null);
  const [job, setJob] = useState<string | null>(null);
  const [dns, setDns] = useState<DnsResult | null>(null);
  const [portResults, setPortResults] = useState<PortResult[] | null>(null);
  const [cidr, setCidr] = useState("192.168.1.10/24");
  const [splitTo, setSplitTo] = useState(26);

  const onLine = (l: Line) => {
    if (l.type === "line") setLines((x) => [...x, l.text]);
    else setSummary(l.summary);
  };

  async function start() {
    if (job) return;
    const id = crypto.randomUUID();
    setJob(id);
    setLines([]);
    setSummary(null);
    setDns(null);
    setPortResults(null);
    try {
      if (tool === "ping") await netPing(id, target, count, size ? Number(size) : null, onLine);
      else if (tool === "trace") await netTraceroute(id, target, hops, onLine);
      else if (tool === "mtu") await netMtu(id, target, onLine);
      else if (tool === "dns") setDns(await netDns(target, record, server));
      else if (tool === "ports") {
        const list = parsePorts(ports);
        if (!list.length) throw new Error("enter ports like 22, 80-443");
        setPortResults(await netPorts(target, list));
      }
    } catch (e) {
      setSummary(String(e));
      onToast(String(e));
    } finally {
      setJob(null);
    }
  }

  const net = tool === "subnet" ? subnet(cidr) : null;
  const needsTarget = tool !== "subnet";

  return (
    <div className="tools-body">
      <div className="ws-banner">Runs from this computer, not from the device.</div>
      <div className="dialog-tabs small">
        {TOOLS.map((t) => <button key={t.id} className={tool === t.id ? "on" : ""} onClick={() => { setTool(t.id); setLines([]); setSummary(null); }}>{t.title}</button>)}
      </div>

      {needsTarget && (
        <form className="ws-form" onSubmit={(e) => { e.preventDefault(); void start(); }}>
          <input value={target} onChange={(e) => setTarget(e.target.value)} placeholder="host name or IP address" spellCheck={false} autoFocus />
          {tool === "ping" && (
            <>
              <label>count<input type="number" min={1} max={100} value={count} onChange={(e) => setCount(Number(e.target.value))} /></label>
              <label>size<input type="number" min={0} max={65000} value={size} placeholder="56" onChange={(e) => setSize(e.target.value)} /></label>
            </>
          )}
          {tool === "trace" && <label>max hops<input type="number" min={1} max={64} value={hops} onChange={(e) => setHops(Number(e.target.value))} /></label>}
          {tool === "dns" && (
            <>
              <select value={record} onChange={(e) => setRecord(e.target.value)}>
                {["A", "AAAA", "PTR", "MX", "TXT", "NS", "CNAME", "SOA", "SRV"].map((r) => <option key={r}>{r}</option>)}
              </select>
              <input value={server} onChange={(e) => setServer(e.target.value)} placeholder="DNS server (optional)" spellCheck={false} />
            </>
          )}
          {tool === "ports" && <input className="ports" value={ports} onChange={(e) => setPorts(e.target.value)} placeholder="22, 80-443" spellCheck={false} />}
          {job ? (
            <button type="button" className="btn" onClick={() => void netCancel(job)}><StopIcon /> Stop</button>
          ) : (
            <button type="submit" className="btn primary" disabled={!target.trim()}>{tool === "mtu" ? "Find MTU" : tool === "ports" ? "Check" : tool === "dns" ? "Look up" : "Run"}</button>
          )}
        </form>
      )}
      {tool === "mtu" && <div className="quiet small">Sends do-not-fragment pings of growing size and reports the largest that gets through plus the 28 byte header: the path MTU to the target.</div>}

      {(lines.length > 0 || summary) && (
        <div className="ws-output">
          <pre>{lines.join("\n")}</pre>
          {summary && <div className="ws-summary">{summary}</div>}
        </div>
      )}

      {dns && (
        <div className="ws-output">
          <div className="ws-summary">{dns.query} · {dns.elapsedMs} ms</div>
          {dns.addresses.length > 0 && <div><span className="quiet">Addresses: </span>{dns.addresses.join(", ")}</div>}
          {dns.names.length > 0 && <div><span className="quiet">Names: </span>{dns.names.join(", ")}</div>}
          {dns.addresses.length === 0 && dns.names.length === 0 && !dns.detail && <div className="warn-text">No records.</div>}
          {dns.detail && <details className="raw"><summary>Raw answer</summary><pre>{dns.detail}</pre></details>}
        </div>
      )}

      {portResults && (
        <div className="data-table">
          <div className="data-table-head"><span>{target} · {portResults.filter((p) => p.open).length} of {portResults.length} open</span></div>
          <div className="data-table-scroll">
            <table>
              <thead><tr><th>Port</th><th>State</th><th>Time</th></tr></thead>
              <tbody>
                {portResults.map((p) => <tr key={p.port}><td>{p.port}</td><td className={p.open ? "diff-add" : "quiet"}>{p.detail}</td><td>{p.elapsedMs} ms</td></tr>)}
              </tbody>
            </table>
          </div>
        </div>
      )}

      {tool === "subnet" && (
        <div className="ws-subnet">
          <input value={cidr} onChange={(e) => setCidr(e.target.value)} placeholder="10.1.2.3/24  or  10.1.2.3 255.255.255.0  or  10.1.2.3 0.0.0.255" spellCheck={false} autoFocus />
          {net ? (
            <>
              <dl>
                <dt>Network</dt><dd>{net.cidr}</dd>
                <dt>Mask</dt><dd>{net.mask} <span className="quiet">wildcard {net.wildcard}</span></dd>
                <dt>Hosts</dt><dd>{net.firstHost} – {net.lastHost} <span className="quiet">({net.hosts.toLocaleString()} usable)</span></dd>
                <dt>Broadcast</dt><dd>{net.broadcast}</dd>
                <dt>Class</dt><dd>{net.class}{net.private ? " · private (RFC 1918 / link-local)" : ""}</dd>
              </dl>
              <div className="row">
                <label>Split into /<input type="number" min={net.prefix + 1} max={32} value={splitTo} onChange={(e) => setSplitTo(Number(e.target.value))} style={{ width: 60 }} /></label>
                <span className="quiet small">{split(net, splitTo).length ? `${2 ** (splitTo - net.prefix)} subnets of ${Math.max(0, 2 ** (32 - splitTo) - 2)} hosts` : ""}</span>
              </div>
              {split(net, splitTo).length > 0 && <pre className="ws-split">{split(net, splitTo).join("\n")}{2 ** (splitTo - net.prefix) > 256 ? "\n…" : ""}</pre>}
            </>
          ) : (
            <div className="warn-text">Enter an address with a prefix length or mask.</div>
          )}
        </div>
      )}
    </div>
  );
}

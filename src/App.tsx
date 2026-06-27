import { useState, useEffect, useRef, useCallback } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type { InterfaceInfo, AppConfig, LogEntry, ConnectionStatus } from "./types";

const DEFAULT_CONFIG: AppConfig = {
  interface_name: "",
  remote_ip: "",
  remote_port: 13400,
  session_code: "",
  local_ws_port: 8765,
  doip_source_address: 0x0e00,
  doip_target_address: 0x0001,
};

function hexInput(val: number): string {
  return "0x" + val.toString(16).toUpperCase().padStart(4, "0");
}

function parseHex(raw: string): number | null {
  const cleaned = raw.replace(/^0x/i, "");
  const n = parseInt(cleaned, 16);
  return isNaN(n) || n < 0 || n > 0xffff ? null : n;
}

export default function App() {
  const [interfaces, setInterfaces] = useState<InterfaceInfo[]>([]);
  const [config, setConfig] = useState<AppConfig>(DEFAULT_CONFIG);
  const [status, setStatus] = useState<ConnectionStatus>("Disconnected");
  const [logs, setLogs] = useState<LogEntry[]>([]);
  const [isBusy, setIsBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const logEndRef = useRef<HTMLDivElement>(null);

  /* ── Bootstrap ────────────────────────────────────────────────────────── */
  useEffect(() => {
    invoke<InterfaceInfo[]>("get_interfaces")
      .then((ifaces) => {
        setInterfaces(ifaces);
        if (ifaces.length > 0) {
          setConfig((p) => ({ ...p, interface_name: ifaces[0].name }));
        }
      })
      .catch(console.error);

    invoke<LogEntry[]>("get_logs").then(setLogs).catch(console.error);

    const unlistenLog = listen<LogEntry>("log_entry", (e) => {
      setLogs((prev) => [...prev.slice(-999), e.payload]);
    });

    const unlistenStatus = listen<string>("connection_status", (e) => {
      setStatus(e.payload as ConnectionStatus);
      setIsBusy(false);
    });

    return () => {
      unlistenLog.then((f) => f());
      unlistenStatus.then((f) => f());
    };
  }, []);

  /* ── Auto-scroll logs ─────────────────────────────────────────────────── */
  useEffect(() => {
    logEndRef.current?.scrollIntoView({ behavior: "smooth" });
  }, [logs]);

  /* ── Actions ──────────────────────────────────────────────────────────── */
  const handleConnect = async () => {
    setError(null);
    setIsBusy(true);
    try {
      await invoke("update_config", { config });
      await invoke("connect");
    } catch (err) {
      setError(String(err));
      setIsBusy(false);
    }
  };

  const handleDisconnect = async () => {
    setIsBusy(true);
    try {
      await invoke("disconnect");
    } catch (err) {
      setError(String(err));
      setIsBusy(false);
    }
  };

  const clearLogs = useCallback(() => setLogs([]), []);

  /* ── Derived state ────────────────────────────────────────────────────── */
  const isConnected = status === "Connected";
  const isConnecting = status === "Connecting";
  const locked = isConnected || isConnecting;
  const canConnect = !locked && config.remote_ip.trim().length > 0;

  /* ── Render ───────────────────────────────────────────────────────────── */
  return (
    <div className="app">
      {/* ── Header ── */}
      <header className="header">
        <div className="header-left">
          <span className="logo-icon">⟨/⟩</span>
          <span className="logo-text">
            AutoBridge <span className="accent">Mac</span>
          </span>
          <span className="badge">v0.1.0</span>
          <span className="badge badge-protocol">ENET/DoIP</span>
        </div>
        <div className={`status-pill status-${status.toLowerCase()}`}>
          <span className="status-dot" />
          {status}
        </div>
      </header>

      {/* ── Main layout ── */}
      <div className="layout">
        {/* ── Sidebar ── */}
        <aside className="sidebar">
          <div className="section-label">Connection</div>

          {/* Interface selector */}
          <div className="field">
            <label>Ethernet Interface</label>
            <select
              value={config.interface_name}
              onChange={(e) =>
                setConfig((p) => ({ ...p, interface_name: e.target.value }))
              }
              disabled={locked}
            >
              {interfaces.length === 0 && (
                <option value="">No interfaces found</option>
              )}
              {interfaces.map((iface) => (
                <option key={iface.name} value={iface.name}>
                  {iface.name}
                  {iface.ip_addresses[0] ? ` — ${iface.ip_addresses[0]}` : ""}
                  {!iface.is_up ? " (down)" : ""}
                </option>
              ))}
            </select>
          </div>

          {/* Remote IP */}
          <div className="field">
            <label>Remote Server IP</label>
            <input
              type="text"
              placeholder="192.168.1.100"
              value={config.remote_ip}
              onChange={(e) =>
                setConfig((p) => ({ ...p, remote_ip: e.target.value }))
              }
              disabled={locked}
            />
          </div>

          {/* Port row */}
          <div className="field-row">
            <div className="field">
              <label>DoIP Port</label>
              <input
                type="number"
                value={config.remote_port}
                onChange={(e) =>
                  setConfig((p) => ({
                    ...p,
                    remote_port: parseInt(e.target.value) || 13400,
                  }))
                }
                disabled={locked}
                min={1}
                max={65535}
              />
            </div>
            <div className="field">
              <label>WS Port</label>
              <input
                type="number"
                value={config.local_ws_port}
                onChange={(e) =>
                  setConfig((p) => ({
                    ...p,
                    local_ws_port: parseInt(e.target.value) || 8765,
                  }))
                }
                disabled={locked}
                min={1024}
                max={65535}
              />
            </div>
          </div>

          {/* Session code */}
          <div className="field">
            <label>Session Code</label>
            <input
              type="text"
              placeholder="optional identifier"
              value={config.session_code}
              onChange={(e) =>
                setConfig((p) => ({ ...p, session_code: e.target.value }))
              }
              disabled={locked}
            />
          </div>

          <div className="divider" />
          <div className="section-label">DoIP Addressing</div>

          {/* DoIP addresses */}
          <div className="field-row">
            <div className="field">
              <label>Source Addr</label>
              <input
                type="text"
                value={hexInput(config.doip_source_address)}
                onChange={(e) => {
                  const n = parseHex(e.target.value);
                  if (n !== null)
                    setConfig((p) => ({ ...p, doip_source_address: n }));
                }}
                disabled={locked}
                placeholder="0x0E00"
              />
            </div>
            <div className="field">
              <label>Target Addr</label>
              <input
                type="text"
                value={hexInput(config.doip_target_address)}
                onChange={(e) => {
                  const n = parseHex(e.target.value);
                  if (n !== null)
                    setConfig((p) => ({ ...p, doip_target_address: n }));
                }}
                disabled={locked}
                placeholder="0x0001"
              />
            </div>
          </div>

          {/* Error message */}
          {error && <div className="error-box">{error}</div>}

          {/* Tunnel badge */}
          {isConnected && (
            <div className="tunnel-badge">
              <span className="tunnel-dot" />
              ws://127.0.0.1:{config.local_ws_port}
            </div>
          )}

          <div className="spacer" />

          {/* Connect / Disconnect button */}
          <button
            className={`btn-connect ${isConnected ? "btn-disconnect" : ""}`}
            onClick={isConnected ? handleDisconnect : handleConnect}
            disabled={isBusy || isConnecting || (!locked && !canConnect)}
          >
            {isConnecting ? (
              <>
                <span className="spinner" /> Connecting…
              </>
            ) : isConnected ? (
              "Disconnect"
            ) : (
              "Connect"
            )}
          </button>
        </aside>

        {/* ── Log panel ── */}
        <main className="log-panel">
          <div className="log-toolbar">
            <span className="section-label" style={{ margin: 0 }}>
              Log
            </span>
            <div className="log-toolbar-right">
              <span className="log-count">{logs.length} entries</span>
              <button className="btn-clear" onClick={clearLogs}>
                Clear
              </button>
            </div>
          </div>

          <div className="log-body">
            {logs.length === 0 ? (
              <div className="log-empty">
                <span className="log-empty-icon">◎</span>
                <span>No log entries — connect to start</span>
              </div>
            ) : (
              logs.map((entry, i) => (
                <div
                  key={i}
                  className={`log-row log-${entry.level.toLowerCase()}`}
                >
                  <span className="log-ts">{entry.timestamp}</span>
                  <span className={`log-lvl lvl-${entry.level.toLowerCase()}`}>
                    {entry.level}
                  </span>
                  <span className="log-msg">{entry.message}</span>
                </div>
              ))
            )}
            <div ref={logEndRef} />
          </div>
        </main>
      </div>

      {/* ── Footer ── */}
      <footer className="footer">
        <span>ISO 13400-2 DoIP v2</span>
        <span className="footer-sep">·</span>
        <span>TCP:{config.remote_port}</span>
        {isConnected && (
          <>
            <span className="footer-sep">·</span>
            <span className="accent">WS:{config.local_ws_port} ▲</span>
          </>
        )}
        <span className="footer-sep">·</span>
        <span>AutoBridge Mac 0.1.0</span>
      </footer>
    </div>
  );
}

import { useState, useEffect, useRef } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

// ── Types ──────────────────────────────────────────────────────────────────────
interface QualityStats {
  latency_ms: number;
  packet_loss_pct: number;
  uptime_secs: number;
  quality: "Excellent" | "Good" | "Unstable" | "Critical";
}
type EnetState = "Disconnected" | "Searching" | "CarDetected";
type ConnStatus =
  | "Disconnected"
  | "ScanningEnet"
  | "ConnectingDoip"
  | "ReadingVin"
  | "ConnectingRelay"
  | "WaitingPeer"
  | "Connected";

interface LogEntry {
  timestamp: string;
  level: string;
  message: string;
}

interface TestResult {
  success: boolean;
  relay_url: string;
  pings_sent: number;
  pings_received: number;
  packet_loss_pct: number;
  latency_min_ms: number;
  latency_max_ms: number;
  latency_avg_ms: number;
  error: string | null;
}

interface AdvancedConfig {
  relay_url: string;
  ws_port: number;
  doip_port: number;
  src_addr: string;
  tgt_addr: string;
  manual_interface: string;
  debug_mode: boolean;
}

// ── Helpers ────────────────────────────────────────────────────────────────────
function enetStateColor(s: EnetState): string {
  if (s === "Disconnected") return "#ff4060";
  if (s === "Searching") return "#ffaa00";
  return "#00e87a";
}
function enetStateLabel(s: EnetState): string {
  if (s === "Disconnected") return "Auto non rilevata";
  if (s === "Searching") return "Ricerca ENET...";
  return "Auto rilevata";
}
function connStatusColor(s: ConnStatus): string {
  if (s === "Disconnected") return "#ff4060";
  if (s === "Connected") return "#00e87a";
  return "#ffaa00";
}
function connStatusLabel(s: ConnStatus): string {
  if (s === "Disconnected") return "Non connesso";
  if (s === "Connected") return "Tecnico connesso";
  return "Connessione in corso...";
}
function qualityLabel(q: QualityStats["quality"]): string {
  if (q === "Excellent") return "Ottima";
  if (q === "Good") return "Buona";
  if (q === "Unstable") return "Instabile";
  return "Critica";
}
function formatUptime(secs: number): string {
  const h = Math.floor(secs / 3600);
  const m = Math.floor((secs % 3600) / 60);
  const s = secs % 60;
  return [h, m, s].map((v) => String(v).padStart(2, "0")).join(":");
}
function logLevelColor(level: string): string {
  if (level === "ERROR") return "#ff4060";
  if (level === "WARN") return "#ffaa00";
  if (level === "INFO") return "#00e87a";
  return "#8899aa";
}

// ── Styles (inline) ────────────────────────────────────────────────────────────
const styles: Record<string, React.CSSProperties> = {
  root: {
    minHeight: "100vh",
    background: "#0d1117",
    color: "#e6edf3",
    fontFamily: "'Inter', 'Segoe UI', system-ui, sans-serif",
    display: "flex",
    flexDirection: "column",
    alignItems: "center",
    justifyContent: "center",
    padding: "24px 16px",
    boxSizing: "border-box",
  },
  card: {
    width: "100%",
    maxWidth: "420px",
    display: "flex",
    flexDirection: "column",
    gap: "20px",
  },
  header: {
    display: "flex",
    alignItems: "center",
    justifyContent: "space-between",
  },
  logoRow: {
    display: "flex",
    alignItems: "center",
    gap: "10px",
  },
  logoText: {
    fontSize: "22px",
    fontWeight: 700,
    letterSpacing: "-0.5px",
    color: "#e6edf3",
  },
  badge: {
    fontSize: "10px",
    fontWeight: 600,
    letterSpacing: "0.5px",
    color: "#00c8ff",
    border: "1px solid #00c8ff44",
    borderRadius: "6px",
    padding: "2px 7px",
    background: "#00c8ff0d",
  },
  gearBtn: {
    background: "none",
    border: "none",
    color: "#8899aa",
    fontSize: "20px",
    cursor: "pointer",
    padding: "4px 6px",
    borderRadius: "8px",
    transition: "color 0.2s, background 0.2s",
  },
  vinPanel: {
    background: "#161b22",
    borderRadius: "16px",
    padding: "20px 24px",
    display: "flex",
    flexDirection: "column",
    alignItems: "center",
    gap: "6px",
    border: "1px solid #21262d",
  },
  vinLabel: {
    fontSize: "10px",
    fontWeight: 600,
    letterSpacing: "1.5px",
    color: "#8899aa",
    textTransform: "uppercase" as const,
  },
  vinText: {
    fontSize: "17px",
    fontWeight: 600,
    letterSpacing: "2px",
    fontFamily: "'JetBrains Mono', 'Fira Code', 'Courier New', monospace",
    color: "#e6edf3",
  },
  vinShimmer: {
    fontSize: "17px",
    fontWeight: 600,
    letterSpacing: "2px",
    fontFamily: "'JetBrains Mono', 'Fira Code', 'Courier New', monospace",
    color: "#8899aa",
    animation: "shimmer 1.5s ease-in-out infinite",
  },
  vinBrand: {
    fontSize: "12px",
    color: "#8899aa",
    marginTop: "2px",
  },
  statusRow: {
    display: "flex",
    gap: "12px",
  },
  statusCard: {
    flex: 1,
    background: "#161b22",
    border: "1px solid #21262d",
    borderRadius: "12px",
    padding: "14px 16px",
    display: "flex",
    flexDirection: "column",
    gap: "6px",
  },
  statusCardLabel: {
    fontSize: "10px",
    fontWeight: 600,
    letterSpacing: "1.2px",
    color: "#8899aa",
    textTransform: "uppercase" as const,
  },
  statusIndicatorRow: {
    display: "flex",
    alignItems: "center",
    gap: "8px",
  },
  dot: {
    width: "8px",
    height: "8px",
    borderRadius: "50%",
    flexShrink: 0,
  },
  statusText: {
    fontSize: "13px",
    fontWeight: 500,
    color: "#e6edf3",
  },
  connectBtn: {
    width: "100%",
    minWidth: "200px",
    height: "56px",
    borderRadius: "28px",
    border: "none",
    fontSize: "16px",
    fontWeight: 700,
    letterSpacing: "0.5px",
    cursor: "pointer",
    display: "flex",
    alignItems: "center",
    justifyContent: "center",
    gap: "10px",
    transition: "background 0.25s, transform 0.1s",
  },
  qualityBar: {
    background: "#161b22",
    border: "1px solid #21262d",
    borderRadius: "12px",
    padding: "14px 20px",
    display: "grid",
    gridTemplateColumns: "1fr 1fr 1fr 1fr",
    gap: "8px",
  },
  qualityItem: {
    display: "flex",
    flexDirection: "column",
    alignItems: "center",
    gap: "4px",
  },
  qualityItemLabel: {
    fontSize: "9px",
    fontWeight: 600,
    letterSpacing: "0.8px",
    color: "#8899aa",
    textTransform: "uppercase" as const,
  },
  qualityItemValue: {
    fontSize: "14px",
    fontWeight: 600,
    color: "#e6edf3",
  },
  logBox: {
    background: "#0d1117",
    border: "1px solid #21262d",
    borderRadius: "12px",
    padding: "12px 16px",
    maxHeight: "160px",
    overflowY: "auto" as const,
    display: "flex",
    flexDirection: "column",
    gap: "4px",
  },
  logLine: {
    display: "flex",
    gap: "8px",
    fontSize: "11px",
    lineHeight: "1.5",
    fontFamily: "'JetBrains Mono', 'Fira Code', monospace",
  },
  logTs: {
    color: "#484f58",
    flexShrink: 0,
  },
  logMsg: {
    color: "#8899aa",
    wordBreak: "break-all" as const,
  },
  overlay: {
    position: "fixed" as const,
    inset: 0,
    background: "#0d1117ee",
    backdropFilter: "blur(8px)",
    display: "flex",
    alignItems: "center",
    justifyContent: "center",
    zIndex: 100,
    padding: "16px",
  },
  advPanel: {
    background: "#161b22",
    border: "1px solid #30363d",
    borderRadius: "20px",
    padding: "28px 28px 24px",
    width: "100%",
    maxWidth: "420px",
    maxHeight: "90vh",
    overflowY: "auto" as const,
    display: "flex",
    flexDirection: "column",
    gap: "18px",
  },
  advHeader: {
    display: "flex",
    alignItems: "center",
    justifyContent: "space-between",
    marginBottom: "4px",
  },
  advTitle: {
    fontSize: "16px",
    fontWeight: 700,
    color: "#e6edf3",
  },
  closeBtn: {
    background: "none",
    border: "none",
    color: "#8899aa",
    fontSize: "22px",
    cursor: "pointer",
    padding: "2px 6px",
    borderRadius: "8px",
    lineHeight: 1,
  },
  fieldGroup: {
    display: "flex",
    flexDirection: "column",
    gap: "6px",
  },
  fieldLabel: {
    fontSize: "11px",
    fontWeight: 600,
    letterSpacing: "0.8px",
    color: "#8899aa",
    textTransform: "uppercase" as const,
  },
  input: {
    background: "#0d1117",
    border: "1px solid #30363d",
    borderRadius: "10px",
    padding: "10px 14px",
    color: "#e6edf3",
    fontSize: "14px",
    fontFamily: "inherit",
    outline: "none",
    width: "100%",
    boxSizing: "border-box" as const,
    transition: "border-color 0.2s",
  },
  toggleRow: {
    display: "flex",
    alignItems: "center",
    justifyContent: "space-between",
  },
  toggleLabel: {
    fontSize: "14px",
    color: "#e6edf3",
    fontWeight: 500,
  },
  saveBtn: {
    width: "100%",
    height: "44px",
    borderRadius: "22px",
    border: "none",
    background: "#00c8ff",
    color: "#0d1117",
    fontSize: "14px",
    fontWeight: 700,
    cursor: "pointer",
    transition: "opacity 0.2s",
    marginTop: "4px",
  },
  exportBtn: {
    width: "100%",
    height: "40px",
    borderRadius: "10px",
    border: "1px solid #30363d",
    background: "none",
    color: "#8899aa",
    fontSize: "13px",
    fontWeight: 600,
    cursor: "pointer",
    transition: "border-color 0.2s, color 0.2s",
  },
};

// ── Spinner ────────────────────────────────────────────────────────────────────
function Spinner() {
  return (
    <span
      style={{
        display: "inline-block",
        width: "18px",
        height: "18px",
        border: "2px solid #0d111766",
        borderTop: "2px solid #0d1117",
        borderRadius: "50%",
        animation: "spin 0.7s linear infinite",
      }}
    />
  );
}

// ── Toggle ─────────────────────────────────────────────────────────────────────
function Toggle({ checked, onChange }: { checked: boolean; onChange: (v: boolean) => void }) {
  return (
    <div
      onClick={() => onChange(!checked)}
      style={{
        width: "44px",
        height: "24px",
        borderRadius: "12px",
        background: checked ? "#00c8ff" : "#30363d",
        position: "relative",
        cursor: "pointer",
        transition: "background 0.2s",
        flexShrink: 0,
      }}
    >
      <div
        style={{
          position: "absolute",
          top: "3px",
          left: checked ? "23px" : "3px",
          width: "18px",
          height: "18px",
          borderRadius: "50%",
          background: "#fff",
          transition: "left 0.2s",
        }}
      />
    </div>
  );
}

// ── App ────────────────────────────────────────────────────────────────────────
export default function App() {
  const [enetState, setEnetState] = useState<EnetState>("Disconnected");
  const [connStatus, setConnStatus] = useState<ConnStatus>("Disconnected");
  const [vin, setVin] = useState<string | null>(null);
  const [quality, setQuality] = useState<QualityStats | null>(null);
  const [logs, setLogs] = useState<LogEntry[]>([]);
  const [showAdvanced, setShowAdvanced] = useState(false);
  const [isConnecting, setIsConnecting] = useState(false);
  const [testResult, setTestResult] = useState<TestResult | null>(null);
  const [isTesting, setIsTesting] = useState(false);
  const logRef = useRef<HTMLDivElement>(null);

  const [config, setConfig] = useState<AdvancedConfig>({
    relay_url: "ws://localhost:3001",
    ws_port: 8765,
    doip_port: 13400,
    src_addr: "0x0E00",
    tgt_addr: "0x0001",
    manual_interface: "",
    debug_mode: false,
  });

  const isConnected = connStatus === "Connected";
  const isReadingVin = connStatus === "ReadingVin";

  useEffect(() => {
    const unlisten: Array<() => void> = [];

    listen<{ state: EnetState; interface: string; ip: string }>("enet_status", (e) => {
      setEnetState(e.payload.state);
    }).then((u) => unlisten.push(u));

    listen<string>("vin_detected", (e) => {
      setVin(e.payload);
    }).then((u) => unlisten.push(u));

    listen<ConnStatus>("connection_status", (e) => {
      setConnStatus(e.payload);
      if (e.payload === "Connected" || e.payload === "Disconnected") {
        setIsConnecting(false);
      }
    }).then((u) => unlisten.push(u));

    listen<QualityStats>("quality_stats", (e) => {
      setQuality(e.payload);
    }).then((u) => unlisten.push(u));

    listen<LogEntry>("log_entry", (e) => {
      setLogs((prev) => [...prev.slice(-99), e.payload]);
    }).then((u) => unlisten.push(u));

    return () => unlisten.forEach((u) => u());
  }, []);

  useEffect(() => {
    if (logRef.current) {
      logRef.current.scrollTop = logRef.current.scrollHeight;
    }
  }, [logs]);

  async function handleConnect() {
    if (isConnected) {
      await invoke("disconnect");
      setIsConnecting(false);
    } else {
      setIsConnecting(true);
      await invoke("connect");
    }
  }

  async function handleSaveConfig() {
    await invoke("update_advanced_config", {
      relay_url: config.relay_url,
      ws_port: config.ws_port,
      doip_port: config.doip_port,
      src_addr: config.src_addr,
      tgt_addr: config.tgt_addr,
      manual_interface: config.manual_interface,
      debug_mode: config.debug_mode,
    });
    setShowAdvanced(false);
  }

  async function handleExportLogs() {
    await invoke("export_logs");
  }

  async function handleTestConnection() {
    setIsTesting(true);
    setTestResult(null);
    try {
      const result = await invoke<TestResult>("test_connection");
      setTestResult(result);
    } catch (e) {
      setTestResult({
        success: false,
        relay_url: "",
        pings_sent: 0,
        pings_received: 0,
        packet_loss_pct: 100,
        latency_min_ms: 0,
        latency_max_ms: 0,
        latency_avg_ms: 0,
        error: String(e),
      });
    } finally {
      setIsTesting(false);
    }
  }

  const connectBtnBg = isConnected ? "#ff4060" : "#00c8ff";
  const connectBtnText = isConnected ? "DISCONNETTI" : isConnecting ? "CONNESSIONE..." : "CONNETTI";

  const visibleLogs = logs.slice(-8);

  return (
    <>
      <style>{`
        @keyframes spin { to { transform: rotate(360deg); } }
        @keyframes pulse { 0%,100% { box-shadow: 0 0 0 0 #00c8ff44; } 50% { box-shadow: 0 0 0 12px #00c8ff00; } }
        @keyframes shimmer { 0%,100% { opacity: 1; } 50% { opacity: 0.3; } }
        * { box-sizing: border-box; margin: 0; padding: 0; }
        body { background: #0d1117; }
        ::-webkit-scrollbar { width: 4px; }
        ::-webkit-scrollbar-track { background: transparent; }
        ::-webkit-scrollbar-thumb { background: #30363d; border-radius: 4px; }
        input:focus { border-color: #00c8ff !important; }
      `}</style>

      <div style={styles.root}>
        <div style={styles.card}>
          {/* Header */}
          <div style={styles.header}>
            <div style={styles.logoRow}>
              <span style={styles.logoText}>AutoBridge</span>
              <span style={styles.badge}>ENET · DoIP</span>
            </div>
            <button
              style={styles.gearBtn}
              onClick={() => setShowAdvanced(true)}
              title="Impostazioni avanzate"
            >
              ⚙
            </button>
          </div>

          {/* VIN Panel */}
          <div style={styles.vinPanel}>
            <span style={styles.vinLabel}>Veicolo</span>
            {isReadingVin && !vin ? (
              <span style={styles.vinShimmer}>Lettura VIN...</span>
            ) : (
              <span style={styles.vinText}>{vin ?? "——————————————————"}</span>
            )}
            <span style={styles.vinBrand}>BMW</span>
          </div>

          {/* Status Row */}
          <div style={styles.statusRow}>
            <div style={styles.statusCard}>
              <span style={styles.statusCardLabel}>ENET</span>
              <div style={styles.statusIndicatorRow}>
                <div
                  style={{
                    ...styles.dot,
                    background: enetStateColor(enetState),
                    boxShadow: `0 0 6px ${enetStateColor(enetState)}88`,
                  }}
                />
                <span style={styles.statusText}>{enetStateLabel(enetState)}</span>
              </div>
            </div>
            <div style={styles.statusCard}>
              <span style={styles.statusCardLabel}>Connessione</span>
              <div style={styles.statusIndicatorRow}>
                <div
                  style={{
                    ...styles.dot,
                    background: connStatusColor(connStatus),
                    boxShadow: `0 0 6px ${connStatusColor(connStatus)}88`,
                  }}
                />
                <span style={styles.statusText}>{connStatusLabel(connStatus)}</span>
              </div>
            </div>
          </div>

          {/* Connect Button */}
          <button
            style={{
              ...styles.connectBtn,
              background: connectBtnBg,
              color: "#0d1117",
              animation: isConnecting && !isConnected ? "pulse 1.2s ease-in-out infinite" : "none",
            }}
            onClick={handleConnect}
          >
            {isConnecting && !isConnected ? <Spinner /> : null}
            {connectBtnText}
          </button>

          {/* Quality Stats */}
          {isConnected && quality && (
            <div style={styles.qualityBar}>
              <div style={styles.qualityItem}>
                <span style={styles.qualityItemLabel}>Latenza</span>
                <span style={styles.qualityItemValue}>{quality.latency_ms} ms</span>
              </div>
              <div style={styles.qualityItem}>
                <span style={styles.qualityItemLabel}>Pacchetti</span>
                <span style={styles.qualityItemValue}>{quality.packet_loss_pct}%</span>
              </div>
              <div style={styles.qualityItem}>
                <span style={styles.qualityItemLabel}>Tempo</span>
                <span style={styles.qualityItemValue}>{formatUptime(quality.uptime_secs)}</span>
              </div>
              <div style={styles.qualityItem}>
                <span style={styles.qualityItemLabel}>Qualità</span>
                <span
                  style={{
                    ...styles.qualityItemValue,
                    color:
                      quality.quality === "Excellent"
                        ? "#00e87a"
                        : quality.quality === "Good"
                        ? "#00c8ff"
                        : quality.quality === "Unstable"
                        ? "#ffaa00"
                        : "#ff4060",
                  }}
                >
                  {qualityLabel(quality.quality)}
                </span>
              </div>
            </div>
          )}

          {/* Log */}
          <div style={styles.logBox} ref={logRef}>
            {visibleLogs.length === 0 ? (
              <span style={{ ...styles.logMsg, fontSize: "11px" }}>Nessun log disponibile.</span>
            ) : (
              visibleLogs.map((entry, i) => (
                <div key={i} style={styles.logLine}>
                  <span style={styles.logTs}>{entry.timestamp.slice(11, 19)}</span>
                  <span style={{ color: logLevelColor(entry.level), flexShrink: 0, fontWeight: 600 }}>
                    {entry.level.padEnd(5)}
                  </span>
                  <span style={styles.logMsg}>{entry.message}</span>
                </div>
              ))
            )}
          </div>

          {/* Test Connessione */}
          <button
            onClick={handleTestConnection}
            disabled={isTesting}
            style={{
              width: "100%",
              height: "42px",
              borderRadius: "21px",
              border: "1px solid #30363d",
              background: "none",
              color: isTesting ? "#8899aa" : "#00c8ff",
              fontSize: "13px",
              fontWeight: 600,
              cursor: isTesting ? "not-allowed" : "pointer",
              display: "flex",
              alignItems: "center",
              justifyContent: "center",
              gap: "8px",
              transition: "border-color 0.2s, color 0.2s",
              letterSpacing: "0.3px",
            }}
          >
            {isTesting ? (
              <>
                <span style={{
                  display: "inline-block",
                  width: "14px",
                  height: "14px",
                  border: "2px solid #30363d",
                  borderTop: "2px solid #8899aa",
                  borderRadius: "50%",
                  animation: "spin 0.7s linear infinite",
                }} />
                Test in corso (10 ping)...
              </>
            ) : (
              <>◎ Test Connessione Relay</>
            )}
          </button>

          {/* Risultato test */}
          {testResult && (
            <div style={{
              background: testResult.success ? "#0d2518" : "#1a0d10",
              border: `1px solid ${testResult.success ? "#1f4a31" : "#4a1f24"}`,
              borderRadius: "14px",
              padding: "16px 20px",
              display: "flex",
              flexDirection: "column",
              gap: "12px",
            }}>
              {/* Intestazione risultato */}
              <div style={{ display: "flex", alignItems: "center", gap: "8px" }}>
                <span style={{
                  fontSize: "15px",
                  fontWeight: 700,
                  color: testResult.success ? "#00e87a" : "#ff4060",
                }}>
                  {testResult.success ? "✓ Relay raggiungibile" : "✗ Relay non raggiungibile"}
                </span>
              </div>

              {testResult.error && (
                <span style={{ fontSize: "12px", color: "#ff4060", fontFamily: "monospace" }}>
                  {testResult.error}
                </span>
              )}

              {testResult.success && (
                <div style={{
                  display: "grid",
                  gridTemplateColumns: "1fr 1fr",
                  gap: "10px",
                }}>
                  {[
                    { label: "Pacchetti inviati",   value: `${testResult.pings_sent}` },
                    { label: "Pacchetti ricevuti",  value: `${testResult.pings_received}` },
                    { label: "Perdita pacchetti",   value: `${testResult.packet_loss_pct}%`,
                      color: testResult.packet_loss_pct === 0 ? "#00e87a"
                           : testResult.packet_loss_pct < 20  ? "#ffaa00" : "#ff4060" },
                    { label: "Latenza media",       value: `${testResult.latency_avg_ms} ms`,
                      color: testResult.latency_avg_ms < 100 ? "#00e87a"
                           : testResult.latency_avg_ms < 300 ? "#ffaa00" : "#ff4060" },
                    { label: "Latenza min",         value: `${testResult.latency_min_ms} ms` },
                    { label: "Latenza max",         value: `${testResult.latency_max_ms} ms` },
                  ].map(({ label, value, color }) => (
                    <div key={label} style={{ display: "flex", flexDirection: "column", gap: "2px" }}>
                      <span style={{ fontSize: "10px", color: "#8899aa", fontWeight: 600,
                                     letterSpacing: "0.5px", textTransform: "uppercase" }}>
                        {label}
                      </span>
                      <span style={{ fontSize: "15px", fontWeight: 700, fontFamily: "monospace",
                                     color: color ?? "#e6edf3" }}>
                        {value}
                      </span>
                    </div>
                  ))}
                </div>
              )}

              <span style={{ fontSize: "10px", color: "#484f58", wordBreak: "break-all" }}>
                {testResult.relay_url}
              </span>
            </div>
          )}

        </div>
      </div>

      {/* Advanced Overlay */}
      {showAdvanced && (
        <div
          style={styles.overlay}
          onClick={(e) => e.target === e.currentTarget && setShowAdvanced(false)}
        >
          <div style={styles.advPanel}>
            <div style={styles.advHeader}>
              <span style={styles.advTitle}>Impostazioni avanzate</span>
              <button style={styles.closeBtn} onClick={() => setShowAdvanced(false)}>
                ✕
              </button>
            </div>

            <div style={styles.fieldGroup}>
              <label style={styles.fieldLabel}>Server Relay URL</label>
              <input
                style={styles.input}
                type="text"
                value={config.relay_url}
                onChange={(e) => setConfig((c) => ({ ...c, relay_url: e.target.value }))}
                placeholder="ws://localhost:3001"
              />
            </div>

            <div style={styles.fieldGroup}>
              <label style={styles.fieldLabel}>WebSocket Port locale</label>
              <input
                style={styles.input}
                type="number"
                value={config.ws_port}
                onChange={(e) => setConfig((c) => ({ ...c, ws_port: Number(e.target.value) }))}
              />
            </div>

            <div style={styles.fieldGroup}>
              <label style={styles.fieldLabel}>DoIP Port</label>
              <input
                style={styles.input}
                type="number"
                value={config.doip_port}
                onChange={(e) => setConfig((c) => ({ ...c, doip_port: Number(e.target.value) }))}
              />
            </div>

            <div style={styles.fieldGroup}>
              <label style={styles.fieldLabel}>Source Address (hex)</label>
              <input
                style={styles.input}
                type="text"
                value={config.src_addr}
                onChange={(e) => setConfig((c) => ({ ...c, src_addr: e.target.value }))}
                placeholder="0x0E00"
              />
            </div>

            <div style={styles.fieldGroup}>
              <label style={styles.fieldLabel}>Target Address (hex)</label>
              <input
                style={styles.input}
                type="text"
                value={config.tgt_addr}
                onChange={(e) => setConfig((c) => ({ ...c, tgt_addr: e.target.value }))}
                placeholder="0x0001"
              />
            </div>

            <div style={styles.fieldGroup}>
              <label style={styles.fieldLabel}>Manual Interface Override</label>
              <input
                style={styles.input}
                type="text"
                value={config.manual_interface}
                onChange={(e) => setConfig((c) => ({ ...c, manual_interface: e.target.value }))}
                placeholder="lascia vuoto per auto"
              />
            </div>

            <div style={styles.toggleRow}>
              <span style={styles.toggleLabel}>Debug Mode</span>
              <Toggle
                checked={config.debug_mode}
                onChange={(v) => setConfig((c) => ({ ...c, debug_mode: v }))}
              />
            </div>

            <button style={styles.saveBtn} onClick={handleSaveConfig}>
              Salva impostazioni
            </button>

            <button style={styles.exportBtn} onClick={handleExportLogs}>
              Esporta log
            </button>
          </div>
        </div>
      )}
    </>
  );
}

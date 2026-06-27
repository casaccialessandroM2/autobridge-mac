export interface InterfaceInfo {
  name: string;
  description: string;
  ip_addresses: string[];
  is_up: boolean;
}

export interface AppConfig {
  interface_name: string;
  remote_ip: string;
  remote_port: number;
  session_code: string;
  local_ws_port: number;
  doip_source_address: number;
  doip_target_address: number;
}

export interface LogEntry {
  timestamp: string;
  level: "INFO" | "WARN" | "ERROR" | "DEBUG" | "DOIP";
  message: string;
}

export type ConnectionStatus =
  | "Disconnected"
  | "Connecting"
  | "Connected"
  | "Error";

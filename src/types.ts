export type ApiSurface = "anthropic" | "openAi";
export type ServiceStatusKind = "stopped" | "starting" | "running" | "stopping" | "error";
export type LogLevel = "error" | "info" | "debug";

export interface FeatureFlags {
  normalizeAdaptiveThinking: boolean;
  mapEffort: boolean;
  preserveSse: boolean;
  smoothStreamingText: boolean;
  reasoningReplay: boolean;
  oneMContextDefaults: boolean;
  redactSensitiveLogs: boolean;
}

export interface ModelMapping {
  main: string;
  opus: string;
  sonnet: string;
  haiku: string;
  subagent: string;
}

export interface GatewayProfile {
  id: string;
  name: string;
  port: number;
  upstreamBaseUrl: string;
  apiKey?: string | null;
  enabledSurfaces: ApiSurface[];
  modelMapping: ModelMapping;
  timeoutSeconds: number;
  logLevel: LogLevel;
  features: FeatureFlags;
}

export interface ProfileStatus {
  id: string;
  status: ServiceStatusKind;
  port: number;
  proxyOrigin: string;
  lastError?: string | null;
  startedAt?: string | null;
  requestCount: number;
}

export interface LogEntry {
  timestamp: string;
  level: string;
  profileId: string;
  requestId?: string | null;
  message: string;
}

export interface McpServiceConfig {
  id: string;
  name: string;
  command: string;
  args: string;
  env: string;
  description: string;
  enabled: boolean;
}

export interface SkillConfig {
  id: string;
  name: string;
  description: string;
  instructions: string;
  enabled: boolean;
}

export interface AppSettings {
  mcpServices: McpServiceConfig[];
  skills: SkillConfig[];
}

export const defaultProfile = (port = 17777): GatewayProfile => ({
  id: "",
  name: "DeepSeek Local",
  port,
  upstreamBaseUrl: "https://api.deepseek.com",
  apiKey: "",
  enabledSurfaces: ["anthropic", "openAi"],
  modelMapping: {
    main: "deepseek-v4-pro[1m]",
    opus: "deepseek-v4-pro[1m]",
    sonnet: "deepseek-v4-pro[1m]",
    haiku: "deepseek-v4-flash",
    subagent: "deepseek-v4-flash"
  },
  timeoutSeconds: 120,
  logLevel: "info",
  features: {
    normalizeAdaptiveThinking: true,
    mapEffort: true,
    preserveSse: true,
    smoothStreamingText: true,
    reasoningReplay: true,
    oneMContextDefaults: true,
    redactSensitiveLogs: true
  }
});

export const defaultSettings = (): AppSettings => ({
  mcpServices: [],
  skills: []
});

export interface ProviderMetric {
  id: string;
  healthy: boolean;
  success: number;
  error: number;
  total: number;
  latency_ms: number;
  circuit_open: boolean;
  last_error: string | null;
  last_success_ts: string | null;
}

export interface ModelEntry {
  id: string;
  name: string;
  provider_id: string;
  native: string;
  display: string;
  supports_tools: boolean;
  supports_images: boolean;
  supports_reasoning: boolean;
  only_reasoning: boolean;
  max_input: number | null;
  max_output: number | null;
}

export type ProviderKind = "opencode-free" | "openrouter" | "ollama" | "custom";

export interface ProviderConfig {
  id: string;
  kind: ProviderKind;
  enabled: boolean;
  base_url: string | null;
  api_key: string | null;
  priority: number;
  model_filter: string[] | null;
  extra_models: string[] | null;
}

export interface AppConfig {
  bind: string;
  port: number;
  concurrency: number;
  request_timeout_secs: number;
  max_retries: number;
  retry_base_delay_ms: number;
  remove_on_exit: boolean;
  use_system_proxy: boolean;
  proxy_url: string | null;
  workbuddy_models_path: string;
  providers: ProviderConfig[];
  log_level: string;
}

export interface StatusResponse {
  providers: ProviderMetric[];
  models: ModelEntry[];
  config: AppConfig;
}

export interface HealthResponse {
  status: string;
  version: string;
  providers: string[];
  models: number;
}

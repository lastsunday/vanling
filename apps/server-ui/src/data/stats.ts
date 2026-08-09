export interface DashboardSummary {
  total_devices: number
  activated_devices: number
  pending_devices: number
  disabled_devices: number
  online_devices: number
  total_sessions: number
  sessions_today: number
  total_rounds: number
  security_events_today: number
  security_events_7d: number
  rate_limited_today: number
  api_requests_today: number
  api_requests_24h: number
  api_p95_duration_24h_ms: number
  api_4xx_24h: number
  api_5xx_24h: number
  api_top_paths: ApiNameCount[]
  recent_security_events: SecuritySummaryEvent[]
  server_version: string
  server_time: string
  recent_sessions: RecentSession[]
}

export interface SecuritySummaryEvent {
  id: string
  event_type: string
  ip: string | null
  path: string | null
  account: string | null
  retry_after_ms: number | null
  limit: number | null
  remaining: number | null
  window_secs: number | null
  create_datetime: string | null
  update_datetime: string | null
}

export interface RecentSession {
  session_id: string
  device_id: string | null
  uid: string | null
  board_type: string | null
  board_name: string | null
  chip_model_name: string | null
  create_datetime: string | null
  turn_count: number
}

export interface ApiNameCount {
  name: string
  count: number
}

export interface TrendPoint {
  date: string
  sessions: number
  rounds: number
  requests: number
}

export interface DashboardTrends {
  daily: TrendPoint[]
}

export interface StepLatency {
  data_type: string
  avg_ms: number
  max_ms: number
  min_ms: number
  count: number
}

export interface DashboardLatency {
  steps: StepLatency[]
}

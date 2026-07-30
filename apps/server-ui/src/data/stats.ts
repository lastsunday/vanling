export interface DashboardSummary {
  total_devices: number
  activated_devices: number
  pending_devices: number
  disabled_devices: number
  online_devices: number
  total_sessions: number
  sessions_today: number
  total_rounds: number
  server_version: string
  server_time: string
  recent_sessions: RecentSession[]
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

export interface TrendPoint {
  date: string
  sessions: number
  rounds: number
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

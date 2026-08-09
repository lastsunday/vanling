import type { PageData } from "./round";

export type SecurityEventType =
  | 'rate_limited'
  | 'rate_limit_near'
  | 'auth_login_success'
  | 'auth_login_failure'

export interface SecurityEvent {
  id: string
  event_type: SecurityEventType
  ip: string | null
  path: string | null
  principal_id: string | null
  account: string | null
  retry_after_ms: number | null
  limit: number | null
  remaining: number | null
  window_secs: number | null
  create_datetime: string | null
  update_datetime: string | null
}

export type SecurityEventListResult = PageData<SecurityEvent>

export interface EventTypeCounts {
  rate_limited: number
  rate_limit_near: number
  auth_login_success: number
  auth_login_failure: number
}

export interface EventIpHit {
  ip: string
  count: number
}

export interface SecurityEventStats {
  today: EventTypeCounts
  last_7d: EventTypeCounts
  total: EventTypeCounts
  top_ips_last_24h: EventIpHit[]
}

export interface BucketUsageInfo {
  key: string
  used: number
  remaining: number
  reset_after_secs: number
}

export interface ResourceUsageInfo {
  name: string
  limit: number
  window_secs: number
  active_keys: number
  allowed: number
  limited: number
  top_keys: BucketUsageInfo[]
}

export interface UsageStatsResult {
  resources: ResourceUsageInfo[]
}

export interface ApiAccessLog {
  id: string
  request_id: string
  method: string
  path: string
  query: string | null
  ip: string | null
  principal_id: string | null
  name: string | null
  status: number
  duration_ms: number
  response_size: number | null
  user_agent: string | null
  create_datetime: string | null
  update_datetime: string | null
}

export type AccessLogListResult = PageData<ApiAccessLog>

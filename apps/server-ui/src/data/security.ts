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

export interface AccessLogHourlyPoint {
  hour: string
  total: number
  count_2xx: number
  count_3xx: number
  count_4xx: number
  count_5xx: number
  avg_ms: number
  p95_ms: number
}

export interface AccessLogNameCount {
  name: string
  count: number
}

export interface AccessLogPrincipalCount {
  id: string
  name: string | null
  count: number
}

export interface AccessLogStats {
  total: number
  today: number
  last_24h: number
  avg_duration_24h_ms: number
  p95_duration_24h_ms: number
  error_4xx_24h: number
  error_5xx_24h: number
  requests_by_hour: AccessLogHourlyPoint[]
  status_classes: AccessLogNameCount[]
  top_methods: AccessLogNameCount[]
  top_paths: AccessLogNameCount[]
  top_ips: AccessLogNameCount[]
  top_principals: AccessLogPrincipalCount[]
}

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
  account: string | null
  retry_after_ms: number | null
  limit: number | null
  remaining: number | null
  window_secs: number | null
  create_datetime: string | null
  update_datetime: string | null
}

export type SecurityEventListResult = PageData<SecurityEvent>

import type { AccessLogListResult, SecurityEventListResult, SecurityEventStats, UsageStatsResult } from '@/data/security'
import { getJson } from './http'

export async function listSecurityEvents(
  page?: number,
  pageSize?: number,
  eventType?: string,
  ip?: string,
  start?: string,
  end?: string,
  account?: string,
  path?: string,
): Promise<SecurityEventListResult> {
  return getJson('/api/security/events', {
    page,
    page_size: pageSize,
    event_type: eventType || undefined,
    ip: ip || undefined,
    start: start || undefined,
    end: end || undefined,
    account: account || undefined,
    path: path || undefined,
  } as Record<string, unknown>)
}

export async function getSecurityEventStats(): Promise<SecurityEventStats> {
  return getJson('/api/security/stats')
}

export async function getSecurityUsageStats(topN?: number): Promise<UsageStatsResult> {
  return getJson('/api/security/usage_stats', {
    top_n: topN || undefined,
  } as Record<string, unknown>)
}

export async function listAccessLogs(
  page?: number,
  pageSize?: number,
  method?: string,
  path?: string,
  ip?: string,
  name?: string,
  status?: number,
): Promise<AccessLogListResult> {
  return getJson('/api/security/access_logs', {
    page,
    page_size: pageSize,
    method: method || undefined,
    path: path || undefined,
    ip: ip || undefined,
    name: name || undefined,
    status,
  } as Record<string, unknown>)
}

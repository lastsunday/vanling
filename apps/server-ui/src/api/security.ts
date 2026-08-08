import type { SecurityEventListResult } from '@/data/security'
import { getJson } from './http'

export async function listSecurityEvents(
  page?: number,
  pageSize?: number,
  eventType?: string,
  ip?: string,
): Promise<SecurityEventListResult> {
  return getJson('/api/security/events', {
    page,
    page_size: pageSize,
    event_type: eventType || undefined,
    ip: ip || undefined,
  } as Record<string, unknown>)
}

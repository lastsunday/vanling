import type { DashboardLatency, DashboardSummary, DashboardTrends } from '@/data/stats'
import { getJson } from './http'

export async function getSummary(): Promise<DashboardSummary> {
  return getJson('/api/stats/summary')
}

export async function getTrends(): Promise<DashboardTrends> {
  return getJson('/api/stats/trends')
}

export async function getLatency(): Promise<DashboardLatency> {
  return getJson('/api/stats/latency')
}

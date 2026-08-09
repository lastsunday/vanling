import { getLatency, getSummary, getTrends } from '@/api/stats'
import type { ApiNameCount, RecentSession, SecuritySummaryEvent, StepLatency, TrendPoint } from '@/data/stats'
import {
  Area,
  AreaChart,
  Bar,
  BarChart,
  CartesianGrid,
  Legend,
  ResponsiveContainer,
  Tooltip,
  XAxis,
  YAxis,
} from 'recharts'
import {
  Badge,
  Box,
  Card,
  Group,
  SimpleGrid,
  Skeleton,
  Table,
  Text,
  Title,
} from '@mantine/core'
import { useQuery } from '@tanstack/react-query'
import { createFileRoute } from '@tanstack/react-router'
import dayjs from 'dayjs'
import { useTranslation } from 'react-i18next'

export const Route = createFileRoute('/_pathlessLayout/admin/')({
  component: RouteComponent,
})

function RouteComponent() {
  const { t } = useTranslation()

  const { data: summary, isLoading: summaryLoading } = useQuery({
    queryKey: ['stats', 'summary'],
    queryFn: getSummary,
    refetchInterval: 60_000,
  })

  const { data: trends } = useQuery({
    queryKey: ['stats', 'trends'],
    queryFn: getTrends,
    refetchInterval: 120_000,
  })

  const { data: latency } = useQuery({
    queryKey: ['stats', 'latency'],
    queryFn: getLatency,
    refetchInterval: 120_000,
  })

  return (
    <>
      <Title mb="lg">{t('dashboard.title')}</Title>

      <SimpleGrid cols={{ base: 2, sm: 3, lg: 6 }} mb="lg">
        <StatCard label={t('dashboard.stat.total_devices')} value={summary?.total_devices} loading={summaryLoading} icon="i-mdi:devices" />
        <StatCard label={t('dashboard.stat.activated')} value={summary?.activated_devices} loading={summaryLoading} color="green" icon="i-mdi:check-circle" />
        <StatCard label={t('dashboard.stat.pending')} value={summary?.pending_devices} loading={summaryLoading} color="gray" icon="i-mdi:clock-outline" />
        <StatCard label={t('dashboard.stat.disabled')} value={summary?.disabled_devices} loading={summaryLoading} color="red" icon="i-mdi:cancel" />
        <StatCard label={t('dashboard.stat.online')} value={summary?.online_devices} loading={summaryLoading} color="blue" icon="i-mdi:signal" />
        <StatCard label={t('dashboard.stat.sessions_today')} value={summary?.sessions_today} loading={summaryLoading} color="violet" icon="i-mdi:chat-processing" />
      </SimpleGrid>

      <SimpleGrid cols={{ base: 1, sm: 3, lg: 3 }} mb="lg">
        <StatCard label={t('dashboard.stat.security_events_today')} value={summary?.security_events_today} loading={summaryLoading} color="orange" icon="i-mdi:shield-alert" />
        <StatCard label={t('dashboard.stat.security_events_7d')} value={summary?.security_events_7d} loading={summaryLoading} color="violet" icon="i-mdi:shield-alert-outline" />
        <StatCard label={t('dashboard.stat.rate_limited_today')} value={summary?.rate_limited_today} loading={summaryLoading} color="red" icon="i-mdi:shield-off" />
      </SimpleGrid>

      <SimpleGrid cols={{ base: 2, sm: 3, lg: 5 }} mb="lg">
        <StatCard label={t('dashboard.stat.api_requests_today')} value={summary?.api_requests_today} loading={summaryLoading} color="blue" icon="i-mdi:api" />
        <StatCard label={t('dashboard.stat.api_requests_24h')} value={summary?.api_requests_24h} loading={summaryLoading} color="cyan" icon="i-mdi:server-network" />
        <StatCard label={t('dashboard.stat.api_p95_duration')} value={summary?.api_p95_duration_24h_ms} loading={summaryLoading} color="grape" icon="i-mdi:clock-fast" />
        <StatCard label={t('dashboard.stat.api_4xx')} value={summary?.api_4xx_24h} loading={summaryLoading} color="orange" icon="i-mdi:alert-circle-outline" />
        <StatCard label={t('dashboard.stat.api_5xx')} value={summary?.api_5xx_24h} loading={summaryLoading} color="red" icon="i-mdi:alert-octagon-outline" />
      </SimpleGrid>

      <Card withBorder padding="lg" radius="md" mb="lg">
        <Text fw={500} mb="md">{t('dashboard.recent_security_events')}</Text>
        {summaryLoading ? (
          <Skeleton height={180} />
        ) : (
          <RecentSecurityEventsTable events={summary?.recent_security_events ?? []} />
        )}
      </Card>

      <Card withBorder padding="lg" radius="md" mb="lg">
        <Text fw={500} mb="md">{t('dashboard.recent_sessions')}</Text>
        {summaryLoading ? (
          <Skeleton height={180} />
        ) : (
          <RecentSessionsTable sessions={summary?.recent_sessions ?? []} />
        )}
      </Card>

      <Card withBorder padding="lg" radius="md" mb="lg">
        <Text fw={500} mb="md">{t('dashboard.trends')}</Text>
        {trends ? (
          <TrendsChart data={trends.daily} />
        ) : (
          <Skeleton height={200} />
        )}
      </Card>

      <Card withBorder padding="lg" radius="md" mb="lg">
        <Text fw={500} mb="md">{t('dashboard.chart.top_paths')}</Text>
        {summary ? (
          <TopApiPathsCard data={summary.api_top_paths} />
        ) : (
          <Skeleton height={200} />
        )}
      </Card>

      <Card withBorder padding="lg" radius="md">
        <Text fw={500} mb="md">{t('dashboard.latency')}</Text>
        {latency ? (
          <>
            <LatencyChart data={latency.steps} />
            <Box mt="md">
              <LatencyTable data={latency.steps} />
            </Box>
          </>
        ) : (
          <Skeleton height={200} />
        )}
      </Card>
    </>
  )
}

interface StatCardProps {
  label: string
  value: number | undefined
  loading: boolean
  color?: string
  icon?: string
}

function StatCard({ label, value, loading, color, icon }: StatCardProps) {
  return (
    <Card withBorder padding="md" radius="md">
      <Group gap="xs" mb={4}>
        {icon && <div className={icon} style={{ width: 18, height: 18 }} />}
        <Text size="xs" c="dimmed" tt="uppercase" fw={700}>{label}</Text>
      </Group>
      {loading ? (
        <Skeleton height={28} width={60} />
      ) : (
        <Text fw={700} size="xl" c={color}>{value ?? '-'}</Text>
      )}
    </Card>
  )
}

function RecentSessionsTable({ sessions }: { sessions: RecentSession[] }) {
  const { t } = useTranslation()
  if (sessions.length === 0) return <Text c="dimmed" ta="center">{t('dashboard.no_sessions')}</Text>

  return (
    <Table>
      <Table.Thead>
        <Table.Tr>
          <Table.Th>{t('dashboard.table.session_id')}</Table.Th>
          <Table.Th>{t('dashboard.table.device_id')}</Table.Th>
          <Table.Th>{t('dashboard.table.device_uid')}</Table.Th>
          <Table.Th>{t('dashboard.table.board_type')}</Table.Th>
          <Table.Th>{t('dashboard.table.board_name')}</Table.Th>
          <Table.Th>{t('dashboard.table.chip_model')}</Table.Th>
          <Table.Th>{t('dashboard.table.time')}</Table.Th>
          <Table.Th ta="right">{t('dashboard.table.turns')}</Table.Th>
        </Table.Tr>
      </Table.Thead>
      <Table.Tbody>
        {sessions.map((s) => (
          <Table.Tr key={s.session_id}>
            <Table.Td>
              <Text size="sm" style={{ fontFamily: 'monospace', fontSize: 12 }}>
                {s.session_id}
              </Text>
            </Table.Td>
            <Table.Td>
              <Text size="sm" style={{ fontFamily: 'monospace', fontSize: 12 }}>
                {s.device_id ?? '-'}
              </Text>
            </Table.Td>
            <Table.Td>
              <Text size="sm" style={{ fontFamily: 'monospace', fontSize: 12 }}>
                {s.uid ?? '-'}
              </Text>
            </Table.Td>
            <Table.Td>
              <Text size="sm">{s.board_type ?? '-'}</Text>
            </Table.Td>
            <Table.Td>
              <Text size="sm">{s.board_name ?? '-'}</Text>
            </Table.Td>
            <Table.Td>
              <Text size="sm">{s.chip_model_name ?? '-'}</Text>
            </Table.Td>
            <Table.Td>
              <Text size="sm" style={{ whiteSpace: 'nowrap' }}>
                {s.create_datetime ? dayjs(s.create_datetime).format('MM-DD HH:mm') : '-'}
              </Text>
            </Table.Td>
            <Table.Td>
              <Text size="sm" ta="right">{s.turn_count}</Text>
            </Table.Td>
          </Table.Tr>
        ))}
      </Table.Tbody>
    </Table>
  )
}

function RecentSecurityEventsTable({ events }: { events: SecuritySummaryEvent[] }) {
  const { t } = useTranslation()
  if (events.length === 0) return <Text c="dimmed" ta="center">{t('dashboard.no_security_events')}</Text>

  return (
    <Table>
      <Table.Thead>
        <Table.Tr>
          <Table.Th>{t('dashboard.table.time')}</Table.Th>
          <Table.Th>{t('dashboard.table.event_type')}</Table.Th>
          <Table.Th>{t('dashboard.table.ip')}</Table.Th>
          <Table.Th>{t('dashboard.table.path')}</Table.Th>
          <Table.Th>{t('dashboard.table.account')}</Table.Th>
        </Table.Tr>
      </Table.Thead>
      <Table.Tbody>
        {events.map((e) => (
          <Table.Tr key={e.id}>
            <Table.Td>
              <Text size="sm" style={{ whiteSpace: 'nowrap' }}>
                {e.create_datetime ? dayjs(e.create_datetime).format('MM-DD HH:mm:ss') : '-'}
              </Text>
            </Table.Td>
            <Table.Td>
              <SecurityEventBadge eventType={e.event_type} />
            </Table.Td>
            <Table.Td>
              <Text size="sm" style={{ fontFamily: 'monospace', fontSize: 12 }}>
                {e.ip ?? '-'}
              </Text>
            </Table.Td>
            <Table.Td>
              <Text size="sm" style={{ fontFamily: 'monospace', fontSize: 12 }}>
                {e.path ?? '-'}
              </Text>
            </Table.Td>
            <Table.Td>
              <Text size="sm">{e.account ?? '-'}</Text>
            </Table.Td>
          </Table.Tr>
        ))}
      </Table.Tbody>
    </Table>
  )
}

function SecurityEventBadge({ eventType }: { eventType: string }) {
  const { t } = useTranslation()
  const color = eventType === 'rate_limited'
    ? 'red'
    : eventType === 'rate_limit_near'
      ? 'orange'
      : eventType === 'auth_login_success'
        ? 'green'
        : 'yellow'
  return <Badge color={color} variant="light">{t(`security.event_type.${eventType}`)}</Badge>
}

function TrendsChart({ data }: { data: TrendPoint[] }) {
  const { t } = useTranslation()
  const chartData = data.map((d) => ({
    date: d.date.slice(5),
    sessions: d.sessions,
    rounds: d.rounds,
    requests: d.requests,
  }))

  return (
    <ResponsiveContainer width="100%" height={240}>
      <AreaChart data={chartData}>
        <defs>
          <linearGradient id="sessionsGrad" x1="0" y1="0" x2="0" y2="1">
            <stop offset="5%" stopColor="var(--mantine-color-blue-6)" stopOpacity={0.3} />
            <stop offset="95%" stopColor="var(--mantine-color-blue-6)" stopOpacity={0} />
          </linearGradient>
          <linearGradient id="roundsGrad" x1="0" y1="0" x2="0" y2="1">
            <stop offset="5%" stopColor="var(--mantine-color-teal-6)" stopOpacity={0.3} />
            <stop offset="95%" stopColor="var(--mantine-color-teal-6)" stopOpacity={0} />
          </linearGradient>
          <linearGradient id="requestsGrad" x1="0" y1="0" x2="0" y2="1">
            <stop offset="5%" stopColor="var(--mantine-color-orange-6)" stopOpacity={0.3} />
            <stop offset="95%" stopColor="var(--mantine-color-orange-6)" stopOpacity={0} />
          </linearGradient>
        </defs>
        <CartesianGrid strokeDasharray="3 3" />
        <XAxis dataKey="date" fontSize={12} />
        <YAxis fontSize={12} allowDecimals={false} />
        <Tooltip />
        <Legend />
        <Area type="monotone" dataKey="sessions" stroke="var(--mantine-color-blue-6)" fill="url(#sessionsGrad)" name={t('dashboard.chart.sessions')} />
        <Area type="monotone" dataKey="rounds" stroke="var(--mantine-color-teal-6)" fill="url(#roundsGrad)" name={t('dashboard.chart.rounds')} />
        <Area type="monotone" dataKey="requests" stroke="var(--mantine-color-orange-6)" fill="url(#requestsGrad)" name={t('dashboard.chart.requests')} />
      </AreaChart>
    </ResponsiveContainer>
  )
}

function TopApiPathsCard({ data }: { data: ApiNameCount[] }) {
  const { t } = useTranslation()
  if (data.length === 0) {
    return <Text c="dimmed" ta="center">{t('dashboard.chart.no_data')}</Text>
  }
  return (
    <ResponsiveContainer width="100%" height={200}>
      <BarChart data={data} layout="vertical">
        <CartesianGrid strokeDasharray="3 3" />
        <XAxis type="number" fontSize={12} allowDecimals={false} />
        <YAxis type="category" dataKey="name" width={150} fontSize={12} />
        <Tooltip />
        <Bar dataKey="count" fill="var(--mantine-color-orange-6)" name={t('dashboard.chart.requests')} radius={[0, 4, 4, 0]} />
      </BarChart>
    </ResponsiveContainer>
  )
}

const STEP_LABELS: Record<string, string> = {  asr: 'ASR',
  llm: 'LLM',
  tts: 'TTS',
  input_audio: 'Audio In',
  input_audio_tail: 'Audio Tail',
  text: 'Text',
}

function LatencyChart({ data }: { data: StepLatency[] }) {
  const { t } = useTranslation()
  const chartData = data
    .filter((s) => ['asr', 'llm', 'tts'].includes(s.data_type))
    .map((s) => ({
      step: STEP_LABELS[s.data_type] ?? s.data_type,
      avg: Math.round(s.avg_ms),
      max: s.max_ms,
      min: s.min_ms,
    }))

  if (chartData.length === 0) return <Text c="dimmed" ta="center">{t('dashboard.no_latency_data')}</Text>

  return (
    <ResponsiveContainer width="100%" height={240}>
      <BarChart data={chartData} layout="vertical">
        <CartesianGrid strokeDasharray="3 3" />
        <XAxis type="number" fontSize={12} allowDecimals={false} />
        <YAxis type="category" dataKey="step" width={90} fontSize={12} />
        <Tooltip />
        <Legend />
        <Bar dataKey="avg" fill="var(--mantine-color-indigo-6)" name={t('dashboard.chart.avg_ms')} radius={[0, 4, 4, 0]} />
        <Bar dataKey="max" fill="var(--mantine-color-pink-6)" name={t('dashboard.chart.max_ms')} radius={[0, 4, 4, 0]} />
      </BarChart>
    </ResponsiveContainer>
  )
}

function LatencyTable({ data }: { data: StepLatency[] }) {
  const { t } = useTranslation()
  if (data.length === 0) return null

  return (
    <Table>
      <Table.Thead>
        <Table.Tr>
          <Table.Th>{t('dashboard.table.step')}</Table.Th>
          <Table.Th ta="right">{t('dashboard.table.avg_ms')}</Table.Th>
          <Table.Th ta="right">{t('dashboard.table.min_ms')}</Table.Th>
          <Table.Th ta="right">{t('dashboard.table.max_ms')}</Table.Th>
        </Table.Tr>
      </Table.Thead>
      <Table.Tbody>
        {data.map((s) => (
          <Table.Tr key={s.data_type}>
            <Table.Td>
              <Text size="sm">{t(`sessions.step.${s.data_type}`)}</Text>
            </Table.Td>
            <Table.Td>
              <Text size="sm" ta="right">{Math.round(s.avg_ms)}</Text>
            </Table.Td>
            <Table.Td>
              <Text size="sm" ta="right">{s.min_ms}</Text>
            </Table.Td>
            <Table.Td>
              <Text size="sm" ta="right">{s.max_ms}</Text>
            </Table.Td>
          </Table.Tr>
        ))}
      </Table.Tbody>
    </Table>
  )
}

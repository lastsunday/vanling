import { getAccessLogStats, listAccessLogs } from '@/api';
import { DataPagination, usePersistedPageSize } from '@/components/DataPagination';
import { SearchForm } from '@/components/SearchForm';
import type { AccessLogHourlyPoint, AccessLogNameCount, AccessLogPrincipalCount, AccessLogStats, ApiAccessLog } from '@/data/security';
import {
  Badge,
  Card,
  Group,
  Paper,
  Select,
  SimpleGrid,
  Skeleton,
  Tabs,
  Table,
  Text,
  TextInput,
  Title,
} from '@mantine/core';
import { useQuery } from '@tanstack/react-query';
import { createFileRoute } from '@tanstack/react-router';
import dayjs from 'dayjs';
import {
  Area,
  AreaChart,
  Bar,
  BarChart,
  CartesianGrid,
  Cell,
  Legend,
  Pie,
  PieChart,
  ResponsiveContainer,
  Tooltip,
  XAxis,
  YAxis,
} from 'recharts';
import { useState } from 'react';
import { useTranslation } from 'react-i18next';

export const Route = createFileRoute('/_pathlessLayout/admin/access-logs/')({
  component: RouteComponent,
});

const METHOD_OPTIONS = ['GET', 'POST', 'PUT', 'DELETE', 'PATCH', 'OPTIONS', 'HEAD'];

function statusBadge(status: number) {
  const color = status < 300 ? 'green' : status < 400 ? 'yellow' : status < 500 ? 'orange' : 'red';
  return <Badge color={color} variant="light">{status}</Badge>;
}

function formatSize(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  return `${(bytes / (1024 * 1024)).toFixed(2)} MB`;
}

const STATUS_COLORS: Record<string, string> = {
  '2xx': '#40c057',
  '3xx': '#fab005',
  '4xx': '#fd7e14',
  '5xx': '#fa5252',
};

function AccessLogStatCard({ label, value, loading }: { label: string; value?: number | string; loading?: boolean }) {
  return (
    <Card withBorder padding="md" radius="md">
      <Text size="xs" c="dimmed" tt="uppercase" fw={700} mb={4}>{label}</Text>
      {loading ? (
        <Skeleton height={28} width={60} />
      ) : (
        <Text fw={700} size="xl">{value ?? '-'}</Text>
      )}
    </Card>
  );
}

function RequestsByHourChart({ data, loading }: { data: AccessLogHourlyPoint[]; loading?: boolean }) {
  const { t } = useTranslation();
  if (loading) return <Skeleton height={240} />;
  if (data.length === 0) return <Text c="dimmed" ta="center" py="xl">{t('security.access_log.chart.empty')}</Text>;
  const chartData = data.map((p) => ({
    hour: p.hour,
    '2xx': p.count_2xx,
    '3xx': p.count_3xx,
    '4xx': p.count_4xx,
    '5xx': p.count_5xx,
  }));
  return (
    <ResponsiveContainer width="100%" height={240}>
      <BarChart data={chartData}>
        <CartesianGrid strokeDasharray="3 3" />
        <XAxis dataKey="hour" fontSize={11} interval={3} />
        <YAxis fontSize={11} allowDecimals={false} />
        <Tooltip />
        <Legend />
        <Bar dataKey="2xx" stackId="a" fill="#40c057" />
        <Bar dataKey="3xx" stackId="a" fill="#fab005" />
        <Bar dataKey="4xx" stackId="a" fill="#fd7e14" />
        <Bar dataKey="5xx" stackId="a" fill="#fa5252" />
      </BarChart>
    </ResponsiveContainer>
  );
}

function StatusDistributionChart({ data, loading }: { data: AccessLogNameCount[]; loading?: boolean }) {
  const { t } = useTranslation();
  if (loading) return <Skeleton height={240} />;
  if (data.length === 0) return <Text c="dimmed" ta="center" py="xl">{t('security.access_log.chart.empty')}</Text>;
  return (
    <ResponsiveContainer width="100%" height={240}>
      <PieChart>
        <Pie data={data} dataKey="count" nameKey="name" cx="50%" cy="50%" outerRadius={80} label>
          {data.map((entry) => (
            <Cell key={entry.name} fill={STATUS_COLORS[entry.name] ?? '#868e96'} />
          ))}
        </Pie>
        <Tooltip />
        <Legend />
      </PieChart>
    </ResponsiveContainer>
  );
}

function LatencyTrendChart({ data, loading }: { data: AccessLogHourlyPoint[]; loading?: boolean }) {
  const { t } = useTranslation();
  if (loading) return <Skeleton height={240} />;
  if (data.length === 0) return <Text c="dimmed" ta="center" py="xl">{t('security.access_log.chart.empty')}</Text>;
  const chartData = data.map((p) => ({
    hour: p.hour,
    avg: Math.round(p.avg_ms * 100) / 100,
    p95: p.p95_ms,
  }));
  return (
    <ResponsiveContainer width="100%" height={240}>
      <AreaChart data={chartData}>
        <defs>
          <linearGradient id="avgGrad" x1="0" y1="0" x2="0" y2="1">
            <stop offset="5%" stopColor="#228be6" stopOpacity={0.3} />
            <stop offset="95%" stopColor="#228be6" stopOpacity={0} />
          </linearGradient>
        </defs>
        <CartesianGrid strokeDasharray="3 3" />
        <XAxis dataKey="hour" fontSize={11} interval={3} />
        <YAxis fontSize={11} />
        <Tooltip />
        <Legend />
        <Area type="monotone" dataKey="avg" stroke="#228be6" fill="url(#avgGrad)" name={t('security.access_log.chart.avg')} />
        <Area type="monotone" dataKey="p95" stroke="#f76707" fill="none" name={t('security.access_log.chart.p95')} />
      </AreaChart>
    </ResponsiveContainer>
  );
}

function TopPathsChart({ data, loading }: { data: AccessLogNameCount[]; loading?: boolean }) {
  const { t } = useTranslation();
  if (loading) return <Skeleton height={240} />;
  if (data.length === 0) return <Text c="dimmed" ta="center" py="xl">{t('security.access_log.chart.empty')}</Text>;
  return (
    <ResponsiveContainer width="100%" height={240}>
      <BarChart data={data} layout="vertical">
        <CartesianGrid strokeDasharray="3 3" />
        <XAxis type="number" fontSize={11} allowDecimals={false} />
        <YAxis type="category" dataKey="name" width={140} fontSize={11} />
        <Tooltip />
        <Bar dataKey="count" fill="#fd7e14" name={t('security.access_log.chart.count')} radius={[0, 4, 4, 0]} />
      </BarChart>
    </ResponsiveContainer>
  );
}

function NameCountTable({ data, loading }: { data: AccessLogNameCount[]; loading?: boolean }) {
  const { t } = useTranslation();
  if (loading) return <Skeleton height={200} />;
  if (data.length === 0) return <Text c="dimmed" ta="center" py="xl">{t('security.access_log.chart.empty')}</Text>;
  return (
    <Table>
      <Table.Thead>
        <Table.Tr>
          <Table.Th>{t('security.access_log.chart.name')}</Table.Th>
          <Table.Th ta="right">{t('security.access_log.chart.count')}</Table.Th>
        </Table.Tr>
      </Table.Thead>
      <Table.Tbody>
        {data.map((item) => (
          <Table.Tr key={item.name}>
            <Table.Td>
              <Text size="sm" style={{ fontFamily: 'monospace', fontSize: 12 }}>{item.name}</Text>
            </Table.Td>
            <Table.Td><Text size="sm" ta="right">{item.count}</Text></Table.Td>
          </Table.Tr>
        ))}
      </Table.Tbody>
    </Table>
  );
}

function TopPrincipalsTable({ data, loading }: { data: AccessLogPrincipalCount[]; loading?: boolean }) {
  const { t } = useTranslation();
  if (loading) return <Skeleton height={200} />;
  if (data.length === 0) return <Text c="dimmed" ta="center" py="xl">{t('security.access_log.chart.empty')}</Text>;
  return (
    <Table>
      <Table.Thead>
        <Table.Tr>
          <Table.Th>{t('security.access_log.chart.name')}</Table.Th>
          <Table.Th ta="right">{t('security.access_log.chart.count')}</Table.Th>
        </Table.Tr>
      </Table.Thead>
      <Table.Tbody>
        {data.map((item) => (
          <Table.Tr key={item.id}>
            <Table.Td>
              <Text size="sm" style={{ fontFamily: 'monospace', fontSize: 12 }}>
                {item.name ? `${item.id} (${item.name})` : item.id}
              </Text>
            </Table.Td>
            <Table.Td><Text size="sm" ta="right">{item.count}</Text></Table.Td>
          </Table.Tr>
        ))}
      </Table.Tbody>
    </Table>
  );
}

function AccessLogAnalytics({ stats, loading }: { stats?: AccessLogStats; loading: boolean }) {
  const { t } = useTranslation();
  return (
    <>
      <SimpleGrid cols={{ base: 2, sm: 3, lg: 6 }} mb="md">
        <AccessLogStatCard label={t('security.access_log.stat.today')} value={stats?.today} loading={loading} />
        <AccessLogStatCard label={t('security.access_log.stat.last_24h')} value={stats?.last_24h} loading={loading} />
        <AccessLogStatCard label={t('security.access_log.stat.avg_duration')} value={stats ? `${stats.avg_duration_24h_ms.toFixed(2)} ms` : undefined} loading={loading} />
        <AccessLogStatCard label={t('security.access_log.stat.p95_duration')} value={stats ? `${stats.p95_duration_24h_ms} ms` : undefined} loading={loading} />
        <AccessLogStatCard label="4xx (24h)" value={stats?.error_4xx_24h} loading={loading} />
        <AccessLogStatCard label="5xx (24h)" value={stats?.error_5xx_24h} loading={loading} />
      </SimpleGrid>

      <SimpleGrid cols={{ base: 1, lg: 2 }} mb="md">
        <Paper withBorder shadow="sm" p="md" radius="md">
          <Title order={5} mb="md">{t('security.access_log.chart.requests_by_hour')}</Title>
          <RequestsByHourChart data={stats?.requests_by_hour ?? []} loading={loading} />
        </Paper>
        <Paper withBorder shadow="sm" p="md" radius="md">
          <Title order={5} mb="md">{t('security.access_log.chart.status_distribution')}</Title>
          <StatusDistributionChart data={stats?.status_classes ?? []} loading={loading} />
        </Paper>
        <Paper withBorder shadow="sm" p="md" radius="md">
          <Title order={5} mb="md">{t('security.access_log.chart.latency_trend')}</Title>
          <LatencyTrendChart data={stats?.requests_by_hour ?? []} loading={loading} />
        </Paper>
        <Paper withBorder shadow="sm" p="md" radius="md">
          <Title order={5} mb="md">{t('security.access_log.chart.top_paths')}</Title>
          <TopPathsChart data={stats?.top_paths ?? []} loading={loading} />
        </Paper>
      </SimpleGrid>

      <SimpleGrid cols={{ base: 1, lg: 2 }} mb="md">
        <Paper withBorder shadow="sm" radius="md" style={{ overflow: 'hidden' }}>
          <Title order={5} p="md" pb={0}>{t('security.access_log.chart.top_principals')}</Title>
          <TopPrincipalsTable data={stats?.top_principals ?? []} loading={loading} />
        </Paper>
        <Paper withBorder shadow="sm" radius="md" style={{ overflow: 'hidden' }}>
          <Title order={5} p="md" pb={0}>{t('security.access_log.chart.top_ips')}</Title>
          <NameCountTable data={stats?.top_ips ?? []} loading={loading} />
        </Paper>
      </SimpleGrid>
    </>
  );
}

function AccessLogsSection() {
  const { t } = useTranslation();
  const [page, setPage] = useState(1);
  const [pageSize, setPageSize] = usePersistedPageSize();
  const [draft, setDraft] = useState({
    method: '',
    path: '',
    ip: '',
    name: '',
    principal_id: '',
    status: '',
  });
  const [filters, setFilters] = useState({
    method: '',
    path: '',
    ip: '',
    name: '',
    principal_id: '',
    status: '',
  });
  const [searchKey, setSearchKey] = useState(0);

  const { data, isLoading, isFetching } = useQuery({
    queryKey: ['security-access-logs', page, pageSize, filters, searchKey],
    queryFn: () =>
      listAccessLogs(
        page,
        pageSize,
        filters.method || undefined,
        filters.path || undefined,
        filters.ip || undefined,
        filters.name || undefined,
        filters.principal_id || undefined,
        filters.status ? Number(filters.status) : undefined,
      ),
  });

  const { data: stats, isLoading: statsLoading } = useQuery({
    queryKey: ['security-access-log-stats', searchKey],
    queryFn: getAccessLogStats,
    refetchInterval: 60_000,
  });

  const handleSearch = () => {
    setFilters((prev) => ({ ...prev, ...draft }));
    setPage(1);
    setSearchKey(k => k + 1);
  };

  const setDraftValue = (key: keyof typeof draft, value: string) => {
    setDraft((prev) => ({ ...prev, [key]: value }));
  };

  const handlePageSizeChange = (size: number) => {
    setPageSize(size);
    setPage(1);
  };

  return (
    <>
      <Title mb="lg">{t('admin.menu.access_logs')}</Title>

      <Tabs defaultValue="analytics" keepMounted>
        <Tabs.List mb="md">
          <Tabs.Tab value="analytics">{t('security.access_log.tabs.analytics')}</Tabs.Tab>
          <Tabs.Tab value="logs">{t('security.access_log.tabs.logs')}</Tabs.Tab>
        </Tabs.List>

        <Tabs.Panel value="analytics">
          <AccessLogAnalytics stats={stats} loading={statsLoading} />
        </Tabs.Panel>

        <Tabs.Panel value="logs">
          <Group justify="space-between" mb="md">
            <SearchForm onSubmit={handleSearch} submitLabel={t('security.search_btn')}>
              <TextInput
                value={draft.principal_id}
                onChange={(e) => setDraftValue('principal_id', e.currentTarget.value)}
                placeholder={t('security.access_log.principal_id_placeholder')}
              />
            </SearchForm>
          </Group>

          <Paper withBorder shadow="sm" p="md" radius="md" mb="md">
            <Group grow>
              <Select
                placeholder={t('security.access_log.method_all')}
                data={METHOD_OPTIONS.map((m) => ({ value: m, label: m }))}
                value={draft.method || null}
                onChange={(v) => setDraftValue('method', v ?? '')}
                clearable
              />
              <TextInput
                value={draft.path}
                onChange={(e) => setDraftValue('path', e.currentTarget.value)}
                placeholder={t('security.access_log.path_placeholder')}
              />
              <TextInput
                value={draft.ip}
                onChange={(e) => setDraftValue('ip', e.currentTarget.value)}
                placeholder={t('security.access_log.ip_placeholder')}
              />
              <TextInput
                value={draft.name}
                onChange={(e) => setDraftValue('name', e.currentTarget.value)}
                placeholder={t('security.access_log.name_placeholder')}
              />
              <TextInput
                value={draft.status}
                onChange={(e) => setDraftValue('status', e.currentTarget.value)}
                placeholder={t('security.access_log.status_placeholder')}
              />
            </Group>
          </Paper>

          {(isLoading || isFetching) && (
            <Text ta="center" py="xl">
              {t('loading')}
            </Text>
          )}

          {data && data.items.length > 0 && (
            <Paper withBorder shadow="sm" radius="md" style={{ overflow: 'hidden' }}>
              <Table>
                <Table.Thead>
                  <Table.Tr>
                    <Table.Th>{t('security.table.time')}</Table.Th>
                    <Table.Th>{t('security.table.method')}</Table.Th>
                    <Table.Th>{t('security.table.path')}</Table.Th>
                    <Table.Th>{t('security.table.ip')}</Table.Th>
                    <Table.Th>{t('security.table.principal_id')}</Table.Th>
                    <Table.Th>{t('security.table.name')}</Table.Th>
                    <Table.Th>{t('security.table.status')}</Table.Th>
                    <Table.Th>{t('security.table.duration')}</Table.Th>
                    <Table.Th>{t('security.table.response_size')}</Table.Th>
                    <Table.Th>{t('security.table.request_id')}</Table.Th>
                  </Table.Tr>
                </Table.Thead>
                <Table.Tbody>
                  {data.items.map((log: ApiAccessLog) => (
                    <Table.Tr key={log.id}>
                      <Table.Td>
                        <Text size="sm" style={{ whiteSpace: 'nowrap' }}>
                          {log.create_datetime
                            ? dayjs(log.create_datetime).format('YYYY-MM-DD HH:mm:ss')
                            : '-'}
                        </Text>
                      </Table.Td>
                      <Table.Td>
                        <Badge variant="light" color="blue">{log.method}</Badge>
                      </Table.Td>
                      <Table.Td>
                        <Text size="sm" style={{ fontFamily: 'monospace', fontSize: 12 }}>
                          {log.path}
                        </Text>
                      </Table.Td>
                      <Table.Td>
                        <Text size="sm" style={{ fontFamily: 'monospace', fontSize: 12 }}>
                          {log.ip || '-'}
                        </Text>
                      </Table.Td>
                      <Table.Td>
                        <Text size="sm" style={{ fontFamily: 'monospace', fontSize: 12 }}>
                          {log.principal_id || '-'}
                        </Text>
                      </Table.Td>
                      <Table.Td>
                        <Text size="sm">{log.name || '-'}</Text>
                      </Table.Td>
                      <Table.Td>{statusBadge(log.status)}</Table.Td>
                      <Table.Td>
                        <Text size="sm">{log.duration_ms} ms</Text>
                      </Table.Td>
                      <Table.Td>
                        <Text size="sm">
                          {log.response_size !== null ? formatSize(log.response_size) : '-'}
                        </Text>
                      </Table.Td>
                      <Table.Td>
                        <Text size="xs" c="dimmed" style={{ fontFamily: 'monospace' }}>
                          {log.request_id}
                        </Text>
                      </Table.Td>
                    </Table.Tr>
                  ))}
                </Table.Tbody>
              </Table>
            </Paper>
          )}
    
          {data && data.items.length === 0 && (
            <Text ta="center" py="xl" c="dimmed">
              {t('security.access_log.no_logs')}
            </Text>
          )}
    
          {data && data.total > 0 && (
            <DataPagination
              page={page}
              pageSize={pageSize}
              total={data.total}
              onPageChange={setPage}
              onPageSizeChange={handlePageSizeChange}
            />
          )}
        </Tabs.Panel>
      </Tabs>
    </>
  );
}

function RouteComponent() {
  return <AccessLogsSection />;
}

import {
  getSecurityEventStats,
  getSecurityUsageStats,
  listSecurityEvents,
} from '@/api';
import type { ResourceUsageInfo, SecurityEvent, SecurityEventType } from '@/data/security';
import { SearchForm } from '@/components/SearchForm';
import {
  Badge,
  Button,
  Card,
  Group,
  Pagination,
  Paper,
  Progress,
  Select,
  SimpleGrid,
  Stack,
  Table,
  Tabs,
  Text,
  TextInput,
  Title,
} from '@mantine/core';
import { useQuery } from '@tanstack/react-query';
import { createFileRoute } from '@tanstack/react-router';
import dayjs from 'dayjs';
import { useState } from 'react';
import { useTranslation } from 'react-i18next';

export const Route = createFileRoute('/_pathlessLayout/admin/security/')({
  component: RouteComponent,
});

const EVENT_TYPES: SecurityEventType[] = [
  'rate_limited',
  'rate_limit_near',
  'auth_login_success',
  'auth_login_failure',
];

const EVENT_COLORS: Record<SecurityEventType, string> = {
  rate_limited: 'red',
  rate_limit_near: 'orange',
  auth_login_success: 'green',
  auth_login_failure: 'yellow',
};

type TimeRange = 'today' | '7d' | '30d' | 'all';

const TIME_RANGES: TimeRange[] = ['today', '7d', '30d', 'all'];

function rangeStart(range: TimeRange): string | undefined {
  switch (range) {
    case 'today':
      return dayjs().startOf('day').format();
    case '7d':
      return dayjs().subtract(7, 'day').startOf('day').format();
    case '30d':
      return dayjs().subtract(30, 'day').startOf('day').format();
    default:
      return undefined;
  }
}

function eventBadge(eventType: SecurityEventType, t: (key: string) => string) {
  return (
    <Badge color={EVENT_COLORS[eventType]} variant="light">
      {t(`security.event_type.${eventType}`)}
    </Badge>
  );
}

interface StatCardProps {
  label: string;
  value: number | string | undefined;
  color?: string;
}

function StatCard({ label, value, color }: StatCardProps) {
  return (
    <Card withBorder padding="md" radius="md">
      <Text size="xs" c="dimmed" tt="uppercase" fw={700}>{label}</Text>
      <Text fw={700} size="xl" c={color}>{value ?? '-'}</Text>
    </Card>
  );
}

function StatsSection() {
  const { t } = useTranslation();
  const { data } = useQuery({
    queryKey: ['security-stats'],
    queryFn: getSecurityEventStats,
    refetchInterval: 30_000,
  });
  const today = data?.today;
  const sum = (c?: { rate_limited: number; rate_limit_near: number; auth_login_success: number; auth_login_failure: number }) =>
    c ? c.rate_limited + c.rate_limit_near + c.auth_login_success + c.auth_login_failure : undefined;

  return (
    <>
      <SimpleGrid cols={{ base: 2, sm: 4, lg: 6 }} mb="md">
        <StatCard label={t('security.stats.today_total')} value={sum(today)} />
        <StatCard label={t('security.stats.today_rate_limited')} value={today?.rate_limited} color="red" />
        <StatCard label={t('security.stats.today_login_failure')} value={today?.auth_login_failure} color="yellow" />
        <StatCard label={t('security.stats.today_login_success')} value={today?.auth_login_success} color="green" />
        <StatCard label={t('security.stats.total_events')} value={sum(data?.total)} />
        <StatCard label={t('security.stats.top_ip_24h')} value={data?.top_ips_last_24h?.[0]?.ip} />
      </SimpleGrid>

      {data && data.top_ips_last_24h.length > 0 && (
        <Paper withBorder shadow="sm" radius="md" p="md" mb="md">
          <Text fw={500} mb="xs" size="sm">{t('security.stats.top_ips_title')}</Text>
          <Table>
            <Table.Thead>
              <Table.Tr>
                <Table.Th>{t('security.table.ip')}</Table.Th>
                <Table.Th ta="right">{t('security.stats.count')}</Table.Th>
              </Table.Tr>
            </Table.Thead>
            <Table.Tbody>
              {data.top_ips_last_24h.map((hit) => (
                <Table.Tr key={hit.ip}>
                  <Table.Td>
                    <Text size="sm" style={{ fontFamily: 'monospace', fontSize: 12 }}>{hit.ip}</Text>
                  </Table.Td>
                  <Table.Td>
                    <Text size="sm" ta="right">{hit.count}</Text>
                  </Table.Td>
                </Table.Tr>
              ))}
            </Table.Tbody>
          </Table>
        </Paper>
      )}
    </>
  );
}

function quotaColor(percent: number): string {
  if (percent < 0.6) return 'blue';
  if (percent < 0.95) return 'orange';
  return 'red';
}

function blockRateColor(percent: number): string {
  if (percent < 0.2) return 'blue';
  if (percent < 0.5) return 'orange';
  return 'red';
}

function UsageSection() {
  const { t } = useTranslation();
  const [showAllKeys, setShowAllKeys] = useState(false);
  const { data } = useQuery({
    queryKey: ['security-usage-stats', showAllKeys],
    queryFn: () => getSecurityUsageStats(showAllKeys ? 100 : 10),
    refetchInterval: 30_000,
  });

  const resources: { key: string; label: string; info?: ResourceUsageInfo }[] = [
    { key: 'auth', label: t('security.usage.resource_auth'), info: data?.auth },
    { key: 'ota', label: t('security.usage.resource_ota'), info: data?.ota },
    { key: 'core', label: t('security.usage.resource_core'), info: data?.core },
  ];

  const mayHaveMore = data
    ? resources.some(({ info }) => (info?.top_keys.length ?? 0) === (showAllKeys ? 100 : 10))
    : false;

  return (
    <Stack gap="md">
      {resources.map(({ key, label, info }) => {
        const total = (info?.allowed ?? 0) + (info?.limited ?? 0);
        const blockRate = total > 0 ? (info?.limited ?? 0) / total : 0;
        const tabularNums = { fontVariantNumeric: 'tabular-nums' } as const;
        return (
          <Card key={key} withBorder radius="md">
            <Group justify="space-between" mb="md">
              <Group gap="sm">
                <Text size="sm" fw={600}>{label}</Text>
                <Text size="xs" c="dimmed" style={{ whiteSpace: 'nowrap' }}>
                  {t('security.usage.limit')}: {info?.limit?.toLocaleString() ?? '-'} / {t('security.usage.window')}: {info?.window_secs ?? '-'}s
                </Text>
              </Group>
            </Group>

            <Group gap="sm" mb="md">
              <Text size="xs" fw={600} style={{ whiteSpace: 'nowrap' }}>
                {t('security.usage.block_rate')} {Math.round(blockRate * 100)}%
              </Text>
              <Progress value={blockRate * 100} color={blockRateColor(blockRate)} flex={1} size="sm" />
              <Text size="xs" c="dimmed" style={{ whiteSpace: 'nowrap' }}>
                {t('security.usage.cumulative')}
              </Text>
            </Group>

            <SimpleGrid cols={{ base: 2, sm: 3 }} mb={info?.top_keys.length ? 'lg' : 0}>
              <StatCard label={t('security.usage.active_keys')} value={info?.active_keys} color="blue" />
              <StatCard label={t('security.usage.allowed')} value={info?.allowed} />
              <StatCard label={t('security.usage.limited')} value={info?.limited} color="red" />
            </SimpleGrid>

            {info && info.top_keys.length > 0 ? (
              <Table>
                <Table.Thead bg="var(--mantine-color-gray-0)">
                  <Table.Tr>
                    <Table.Th>{t('security.usage.key')}</Table.Th>
                    <Table.Th ta="right">{t('security.usage.used')}</Table.Th>
                    <Table.Th ta="right">{t('security.usage.remaining')}</Table.Th>
                    <Table.Th ta="right">{t('security.usage.reset_after')}</Table.Th>
                    <Table.Th>{t('security.usage.consumption')}</Table.Th>
                  </Table.Tr>
                </Table.Thead>
                <Table.Tbody>
                  {info.top_keys.map((bucket) => {
                    const bucketTotal = bucket.used + bucket.remaining;
                    const bucketPercent = bucketTotal > 0 ? bucket.used / bucketTotal : 0;
                    return (
                      <Table.Tr key={bucket.key}>
                        <Table.Td>
                          <Text size="sm" style={{ fontFamily: 'monospace', fontSize: 12 }}>{bucket.key}</Text>
                        </Table.Td>
                        <Table.Td>
                          <Text size="sm" ta="right" style={tabularNums}>{bucket.used}</Text>
                        </Table.Td>
                        <Table.Td>
                          <Text size="sm" ta="right" style={tabularNums}>{bucket.remaining}</Text>
                        </Table.Td>
                        <Table.Td>
                          <Text size="sm" ta="right" style={tabularNums}>{bucket.reset_after_secs}s</Text>
                        </Table.Td>
                        <Table.Td>
                          <Progress value={bucketPercent * 100} size="xs" color={quotaColor(bucketPercent)} />
                        </Table.Td>
                      </Table.Tr>
                    );
                  })}
                </Table.Tbody>
              </Table>
            ) : (
              <Text size="sm" c="dimmed">{t('security.usage.no_keys')}</Text>
            )}
          </Card>
        );
      })}
      {mayHaveMore && (
        <Group justify="center">
          <Button
            variant="subtle"
            size="xs"
            onClick={() => setShowAllKeys((v) => !v)}
          >
            {showAllKeys ? t('security.usage.show_less') : t('security.usage.show_more')}
          </Button>
        </Group>
      )}
    </Stack>
  );
}

function RouteComponent() {
  const { t } = useTranslation();
  return (
    <>
      <Title mb="lg">{t('security.title')}</Title>
      <Tabs defaultValue="overview">
        <Tabs.List mb="md">
          <Tabs.Tab value="overview">{t('security.tabs.overview')}</Tabs.Tab>
          <Tabs.Tab value="usage">{t('security.tabs.usage')}</Tabs.Tab>
        </Tabs.List>
        <Tabs.Panel value="overview">
          <OverviewSection />
        </Tabs.Panel>
        <Tabs.Panel value="usage">
          <UsageSection />
        </Tabs.Panel>
      </Tabs>
    </>
  );
}

function OverviewSection() {
  const { t } = useTranslation();
  const [page, setPage] = useState(1);
  const [draft, setDraft] = useState({ ip: '', account: '', path: '', eventType: '', timeRange: '7d' });
  const [filters, setFilters] = useState({ ip: '', account: '', path: '', eventType: '', timeRange: '7d' });
  const [searchKey, setSearchKey] = useState(0);

  const { data, isLoading, isFetching } = useQuery({
    queryKey: ['security-events', page, filters, searchKey],
    queryFn: () =>
      listSecurityEvents(
        page,
        20,
        filters.eventType || undefined,
        filters.ip || undefined,
        rangeStart(filters.timeRange as TimeRange),
        undefined,
        filters.account || undefined,
        filters.path || undefined,
      ),
  });

  const handleSearch = () => {
    setFilters({
      ip: draft.ip.trim(),
      account: draft.account.trim(),
      path: draft.path.trim(),
      eventType: draft.eventType,
      timeRange: draft.timeRange,
    });
    setPage(1);
    setSearchKey(k => k + 1);
  };

  const setDraftValue = (key: keyof typeof draft, value: string) => {
    setDraft((prev) => ({ ...prev, [key]: value }));
  };

  const tabularNums = { fontVariantNumeric: 'tabular-nums' } as const;

  return (
    <>
      <StatsSection />

      <SearchForm
        onSubmit={handleSearch}
        submitLabel={t('security.search_btn')}
        groupProps={{ mb: 'lg', align: 'end', wrap: 'wrap' }}
      >
        <TextInput
          value={draft.ip}
          onChange={(e) => setDraftValue('ip', e.currentTarget.value)}
          placeholder={t('security.ip_placeholder')}
          w={160}
        />
        <TextInput
          value={draft.account}
          onChange={(e) => setDraftValue('account', e.currentTarget.value)}
          placeholder={t('security.account_placeholder')}
          w={160}
        />
        <TextInput
          value={draft.path}
          onChange={(e) => setDraftValue('path', e.currentTarget.value)}
          placeholder={t('security.path_placeholder')}
          w={180}
        />
        <Select
          placeholder={t('security.filter_all')}
          data={[
            { value: '', label: t('security.filter_all') },
            ...EVENT_TYPES.map((type) => ({ value: type, label: t(`security.event_type.${type}`) })),
          ]}
          value={draft.eventType || null}
          onChange={(v) => setDraftValue('eventType', v ?? '')}
          w={220}
          clearable
        />
        <Select
          data={TIME_RANGES.map((r) => ({ value: r, label: t(`security.time_range.${r}`) }))}
          value={draft.timeRange}
          onChange={(v) => setDraftValue('timeRange', v ?? '7d')}
          w={160}
          allowDeselect={false}
        />
      </SearchForm>

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
                <Table.Th>{t('security.table.event_type')}</Table.Th>
                <Table.Th>{t('security.table.ip')}</Table.Th>
                <Table.Th>{t('security.table.account')}</Table.Th>
                <Table.Th>{t('security.table.path')}</Table.Th>
                <Table.Th ta="right">{t('security.table.limit')}</Table.Th>
                <Table.Th ta="right">{t('security.table.remaining')}</Table.Th>
                <Table.Th ta="right">{t('security.table.window')}</Table.Th>
                <Table.Th ta="right">{t('security.table.retry_after')}</Table.Th>
              </Table.Tr>
            </Table.Thead>
            <Table.Tbody>
              {data.items.map((event: SecurityEvent) => (
                <Table.Tr key={event.id}>
                  <Table.Td>
                    <Text size="sm" style={{ whiteSpace: 'nowrap' }}>
                      {event.create_datetime
                        ? dayjs(event.create_datetime).format('YYYY-MM-DD HH:mm:ss')
                        : '-'}
                    </Text>
                  </Table.Td>
                  <Table.Td>{eventBadge(event.event_type, t)}</Table.Td>
                  <Table.Td>
                    <Text size="sm" style={{ fontFamily: 'monospace', fontSize: 12 }}>
                      {event.ip ?? '-'}
                    </Text>
                  </Table.Td>
                  <Table.Td>
                    <Text size="sm">{event.account ?? '-'}</Text>
                  </Table.Td>
                  <Table.Td>
                    <Text size="sm" style={{ fontFamily: 'monospace', fontSize: 12 }}>
                      {event.path ?? '-'}
                    </Text>
                  </Table.Td>
                  <Table.Td>
                    <Text size="sm" ta="right" style={tabularNums}>{event.limit ?? '-'}</Text>
                  </Table.Td>
                  <Table.Td>
                    <Text size="sm" ta="right" style={tabularNums}>{event.remaining ?? '-'}</Text>
                  </Table.Td>
                  <Table.Td>
                    <Text size="sm" ta="right" style={tabularNums}>
                      {event.window_secs ? `${event.window_secs}s` : '-'}
                    </Text>
                  </Table.Td>
                  <Table.Td>
                    <Text size="sm" ta="right" style={tabularNums}>
                      {event.retry_after_ms !== null ? `${event.retry_after_ms} ms` : '-'}
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
          {t('security.no_events')}
        </Text>
      )}

      {data && data.total > data.page_size && (
        <Group justify="center" mt="md">
          <Pagination
            total={Math.ceil(data.total / data.page_size)}
            value={page}
            onChange={setPage}
          />
        </Group>
      )}
    </>
  );
}

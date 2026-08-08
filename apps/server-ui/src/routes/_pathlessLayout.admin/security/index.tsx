import { listSecurityEvents } from '@/api';
import type { SecurityEvent, SecurityEventType } from '@/data/security';
import {
  Badge,
  Group,
  Pagination,
  Paper,
  Select,
  Table,
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

function eventBadge(eventType: SecurityEventType, t: (key: string) => string) {
  switch (eventType) {
    case 'rate_limited':
      return <Badge color="red" variant="light">{t('security.event_type.rate_limited')}</Badge>;
    case 'rate_limit_near':
      return <Badge color="orange" variant="light">{t('security.event_type.rate_limit_near')}</Badge>;
    case 'auth_login_success':
      return <Badge color="green" variant="light">{t('security.event_type.auth_login_success')}</Badge>;
    case 'auth_login_failure':
      return <Badge color="yellow" variant="light">{t('security.event_type.auth_login_failure')}</Badge>;
  }
}

function RouteComponent() {
  const { t } = useTranslation();
  const [page, setPage] = useState(1);
  const [ipInput, setIpInput] = useState('');
  const [ipFilter, setIpFilter] = useState('');
  const [eventTypeFilter, setEventTypeFilter] = useState<string | null>(null);

  const { data, isLoading, isFetching } = useQuery({
    queryKey: ['security-events', page, eventTypeFilter, ipFilter],
    queryFn: () => listSecurityEvents(page, 20, eventTypeFilter ?? undefined, ipFilter || undefined),
  });

  const handleIpSearch = () => {
    setIpFilter(ipInput.trim());
    setPage(1);
  };

  return (
    <>
      <Group justify="space-between" mb="lg">
        <Title>{t('security.title')}</Title>
        <Group>
          <TextInput
            value={ipInput}
            onChange={(e) => setIpInput(e.currentTarget.value)}
            placeholder={t('security.ip_placeholder')}
            onKeyDown={(e) => {
              if (e.key === 'Enter') handleIpSearch();
            }}
          />
          <Select
            placeholder={t('security.filter_all')}
            data={[
              { value: '', label: t('security.filter_all') },
              ...EVENT_TYPES.map((type) => ({ value: type, label: t(`security.event_type.${type}`) })),
            ]}
            value={eventTypeFilter}
            onChange={(v) => {
              setEventTypeFilter(v);
              setPage(1);
            }}
            w={220}
            clearable
          />
        </Group>
      </Group>

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
                <Table.Th>{t('security.table.path')}</Table.Th>
                <Table.Th>{t('security.table.account')}</Table.Th>
                <Table.Th>{t('security.table.limit')}</Table.Th>
                <Table.Th>{t('security.table.retry_after')}</Table.Th>
              </Table.Tr>
            </Table.Thead>
            <Table.Tbody>
              {data.items.map((event: SecurityEvent) => (
                <Table.Tr key={event.id}>
                  <Table.Td>
                    <Text size="sm">
                      {event.create_datetime
                        ? dayjs(event.create_datetime).format('YYYY-MM-DD HH:mm:ss')
                        : '-'}
                    </Text>
                  </Table.Td>
                  <Table.Td>{eventBadge(event.event_type, t)}</Table.Td>
                  <Table.Td>
                    <Text size="sm" style={{ fontFamily: 'monospace', fontSize: 12 }}>
                      {event.ip || '-'}
                    </Text>
                  </Table.Td>
                  <Table.Td>
                    <Text size="sm" style={{ fontFamily: 'monospace', fontSize: 12 }}>
                      {event.path || '-'}
                    </Text>
                  </Table.Td>
                  <Table.Td>
                    <Text size="sm">{event.account || '-'}</Text>
                  </Table.Td>
                  <Table.Td>
                    {event.limit !== null
                      ? `${event.remaining ?? '-'}/${event.limit}`
                      : '-'}
                  </Table.Td>
                  <Table.Td>
                    <Text size="sm">
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

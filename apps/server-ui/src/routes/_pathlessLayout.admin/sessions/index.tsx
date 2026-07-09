import { listSessions } from '@/api';
import { SessionDetail } from '@/components/SessionDetail';
import type { SessionListItem } from '@/data/session';
import {
  Button,
  Grid,
  Group,
  Pagination,
  Paper,
  Select,
  Stack,
  Table,
  Text,
  TextInput,
  Title,
} from '@mantine/core';
import { DateTimePicker } from '@mantine/dates';
import { useQuery } from '@tanstack/react-query';
import { createFileRoute } from '@tanstack/react-router';
import dayjs from 'dayjs';
import { useState } from 'react';
import { useTranslation } from 'react-i18next';

export const Route = createFileRoute('/_pathlessLayout/admin/sessions/')({
  component: RouteComponent,
});

function getSessionTitle(session: SessionListItem): string {
  const firstTurn = session.turns[0];
  if (firstTurn) {
    for (const step of firstTurn.steps) {
      if (step.has_data && step.text) {
        return step.text.length > 40
          ? step.text.slice(0, 40) + '...'
          : step.text;
      }
    }
  }
  return '#' + session.session_id.slice(-8);
}

function formatDuration(ms: number): string {
  if (ms <= 0) return '';
  if (ms < 1000) return `${ms}ms`;
  const sec = ms / 1000;
  if (sec < 60) return `${sec.toFixed(sec < 10 ? 1 : 0)}s`;
  const min = Math.floor(sec / 60);
  const remainSec = Math.round(sec % 60);
  return `${min}m${remainSec}s`;
}

function RouteComponent() {
  const { t } = useTranslation();

  const [page, setPage] = useState(1);
  const [searchInput, setSearchInput] = useState('');
  const [search, setSearch] = useState('');
  const [dateFromInput, setDateFromInput] = useState<string | null>(null);
  const [dateToInput, setDateToInput] = useState<string | null>(null);
  const [dateFrom, setDateFrom] = useState<string | null>(null);
  const [dateTo, setDateTo] = useState<string | null>(null);
  const [sortOrder, setSortOrder] = useState<string>('desc');

  const [selectedSessionId, setSelectedSessionId] = useState<string | null>(null);

  const { data, isLoading } = useQuery({
    queryKey: ['sessions', page, search, dateFrom, dateTo, sortOrder],
    queryFn: () =>
      listSessions({
        page,
        page_size: 20,
        ...(search ? { search } : {}),
        ...(dateFrom ? { date_from: dayjs(dateFrom).toISOString() } : {}),
        ...(dateTo ? { date_to: dayjs(dateTo).toISOString() } : {}),
        sort_order: sortOrder as 'asc' | 'desc',
      }),
  });

  const handleSearch = () => {
    setPage(1);
    setSearch(searchInput);
    setDateFrom(dateFromInput);
    setDateTo(dateToInput);
  };

  const handleSelectSession = (sessionId: string) => {
    setSelectedSessionId((prev) => (prev === sessionId ? null : sessionId));
  };

  const sortOptions = [
    { value: 'desc', label: t('sessions.sort_created_desc') },
    { value: 'asc', label: t('sessions.sort_created_asc') },
  ];

  return (
    <>
      <Title mb="lg">{t('sessions.title')}</Title>

      <Grid>
        <Grid.Col span={{ base: 12, md: 5 }}>
          <Stack>
            <Paper withBorder shadow="sm" p="md" radius="md">
              <Stack>
                <Group gap="sm">
                  <TextInput
                    placeholder={t('sessions.search')}
                    value={searchInput}
                    onChange={(e) => setSearchInput(e.currentTarget.value)}
                    onKeyDown={(e) => {
                      if (e.key === 'Enter') handleSearch();
                    }}
                    style={{ flex: 1 }}
                  />
                  <Button onClick={handleSearch}>{t('sessions.search_btn')}</Button>
                </Group>

                <Group grow>
                  <DateTimePicker
                    placeholder={t('sessions.date_from')}
                    value={dateFromInput}
                    onChange={setDateFromInput}
                    clearable
                    valueFormat="YYYY-MM-DD HH:mm"
                  />
                  <DateTimePicker
                    placeholder={t('sessions.date_to')}
                    value={dateToInput}
                    onChange={setDateToInput}
                    clearable
                    valueFormat="YYYY-MM-DD HH:mm"
                  />
                  <Select
                    data={sortOptions}
                    value={sortOrder}
                    onChange={(val) => {
                      if (!val) return;
                      setSortOrder(val);
                      setPage(1);
                    }}
                  />
                </Group>
              </Stack>
            </Paper>

            {isLoading && (
              <Text ta="center" py="xl">
                {t('loading')}
              </Text>
            )}

            {data && data.items.length > 0 && (
              <Paper withBorder shadow="sm" radius="md" style={{ overflow: 'hidden' }}>
                <Table>
                  <Table.Thead>
                    <Table.Tr>
                      <Table.Th style={{ width: 170, whiteSpace: 'nowrap' }}>{t('sessions.table.time')}</Table.Th>
                      <Table.Th>{t('sessions.table.summary')}</Table.Th>
                      <Table.Th style={{ width: 50, whiteSpace: 'nowrap' }}>{t('sessions.table.turns')}</Table.Th>
                      <Table.Th style={{ width: 70, whiteSpace: 'nowrap' }}>{t('sessions.table.duration')}</Table.Th>
                      <Table.Th style={{ width: 50, whiteSpace: 'nowrap' }}>{t('sessions.table.actions')}</Table.Th>
                    </Table.Tr>
                  </Table.Thead>
                  <Table.Tbody>
                    {data.items.map((session) => {
                      const totalMs = session.turns.reduce(
                        (sum, turn) => sum + turn.steps.reduce((s, st) => s + (st.duration_ms ?? 0), 0),
                        0,
                      );
                      const isSelected = selectedSessionId === session.session_id;
                      return (
                        <Table.Tr
                          key={session.session_id}
                          onClick={() => handleSelectSession(session.session_id)}
                          style={{
                            cursor: 'pointer',
                            borderLeft: isSelected ? '3px solid var(--mantine-color-blue-5)' : '3px solid transparent',
                            background: isSelected ? 'var(--mantine-color-blue-0)' : undefined,
                          }}
                          onMouseEnter={(e) => {
                            if (!isSelected) e.currentTarget.style.background = 'var(--mantine-color-gray-0)';
                          }}
                          onMouseLeave={(e) => {
                            if (!isSelected) e.currentTarget.style.background = '';
                          }}
                        >
                          <Table.Td>
                            <Text
                              size="sm"
                              style={{ whiteSpace: 'nowrap' }}
                              title={session.create_datetime ? dayjs(session.create_datetime).format('YYYY-MM-DD HH:mm:ss') : undefined}
                            >
                              {session.create_datetime
                                ? dayjs(session.create_datetime).format('YYYY-MM-DD HH:mm')
                                : ''}
                            </Text>
                          </Table.Td>
                          <Table.Td>
                            <Text size="sm" truncate="end" style={{ maxWidth: 200 }} title={getSessionTitle(session)}>
                              {getSessionTitle(session)}
                            </Text>
                          </Table.Td>
                          <Table.Td>
                            <Text size="sm">{session.turn_count}</Text>
                          </Table.Td>
                          <Table.Td>
                            <Text size="sm">{formatDuration(totalMs)}</Text>
                          </Table.Td>
                          <Table.Td>
                            <Button
                              variant="subtle"
                              size="compact-xs"
                              onClick={(e) => { e.stopPropagation(); handleSelectSession(session.session_id); }}
                              px={4}
                            >
                              👁
                            </Button>
                          </Table.Td>
                        </Table.Tr>
                      );
                    })}
                  </Table.Tbody>
                </Table>
              </Paper>
            )}

            {data && data.items.length === 0 && (
              <Text ta="center" py="xl" c="dimmed">
                {t('sessions.select_hint')}
              </Text>
            )}

            {data && data.total > data.page_size && (
              <Group justify="center">
                <Pagination
                  total={Math.ceil(data.total / data.page_size)}
                  value={page}
                  onChange={setPage}
                />
              </Group>
            )}
          </Stack>
        </Grid.Col>

        <Grid.Col span={{ base: 12, md: 7 }}>
          {selectedSessionId ? (
            <SessionDetail sessionId={selectedSessionId} />
          ) : (
            <Paper withBorder shadow="sm" p="xl" radius="md" style={{ textAlign: 'center' }}>
              <Text c="dimmed">{t('sessions.select_hint')}</Text>
            </Paper>
          )}
        </Grid.Col>
      </Grid>
    </>
  );
}

import { listSessions } from '@/api';
import { DataPagination, usePersistedPageSize } from '@/components/DataPagination';
import { SearchForm } from '@/components/SearchForm';
import { SessionDetail } from '@/components/SessionDetail';
import type { SessionListItem, TurnSummary } from '@/data/session';
import {
  Button,
  Group,
  Modal,
  Paper,
  Select,
  Table,
  Text,
  TextInput,
  Title,
} from '@mantine/core';
import { DateTimePicker } from '@mantine/dates';
import { useDisclosure } from '@mantine/hooks';
import { useQuery } from '@tanstack/react-query';
import { createFileRoute } from '@tanstack/react-router';
import dayjs from 'dayjs';
import { useState } from 'react';
import { useTranslation } from 'react-i18next';

export const Route = createFileRoute('/_pathlessLayout/admin/sessions/')({
  component: RouteComponent,
});

function getSessionSummary(session: SessionListItem): string {
  for (const turn of session.turns) {
    const hasWake = turn.steps.some((s) => s.mode === 'wake');
    if (hasWake) continue;
    for (const step of turn.steps) {
      if (step.has_data && step.text && (step.step === 'asr' || step.step === 'text')) {
        return step.text.length > 40
          ? step.text.slice(0, 40) + '...'
          : step.text;
      }
    }
  }
  return session.session_id;
}

function formatDeviceId(id: string | null): string {
  return id ?? '-';
}

function getTurnDuration(turn: TurnSummary): number {
  if (turn.steps.length === 0) return 0;
  let minStart = Infinity;
  let maxEnd = -Infinity;
  for (const step of turn.steps) {
    const start = step.duration_ms ?? 0;
    const dur = step.step === 'input_audio' || step.step === 'tts'
      ? (step.audio_duration_ms ?? 0) : 0;
    minStart = Math.min(minStart, start);
    maxEnd = Math.max(maxEnd, start + dur);
  }
  return maxEnd > minStart ? maxEnd - minStart : 0;
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
  const [pageSize, setPageSize] = usePersistedPageSize();
  const [searchInput, setSearchInput] = useState('');
  const [search, setSearch] = useState('');
  const [dateFromInput, setDateFromInput] = useState<string | null>(null);
  const [dateToInput, setDateToInput] = useState<string | null>(null);
  const [dateFrom, setDateFrom] = useState<string | null>(null);
  const [dateTo, setDateTo] = useState<string | null>(null);
  const [sortOrder, setSortOrder] = useState<string>('desc');
  const [searchKey, setSearchKey] = useState(0);

  const [detailSessionId, setDetailSessionId] = useState<string | null>(null);
  const [detailOpened, { open: openDetail, close: closeDetail }] = useDisclosure(false);

  const { data, isLoading } = useQuery({
    queryKey: ['sessions', page, pageSize, search, dateFrom, dateTo, sortOrder, searchKey],
    queryFn: () =>
      listSessions({
        page,
        page_size: pageSize,
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
    setSearchKey(k => k + 1);
  };

  const handlePageSizeChange = (size: number) => {
    setPageSize(size);
    setPage(1);
  };

  const openDetailModal = (sessionId: string) => {
    setDetailSessionId(sessionId);
    openDetail();
  };

  const sortOptions = [
    { value: 'desc', label: t('sessions.sort_created_desc') },
    { value: 'asc', label: t('sessions.sort_created_asc') },
  ];

  return (
    <>
      <Group justify="space-between" mb="lg">
        <Title>{t('sessions.title')}</Title>
        <SearchForm onSubmit={handleSearch} submitLabel={t('sessions.search_btn')}>
          <TextInput
            value={searchInput}
            onChange={(e) => setSearchInput(e.currentTarget.value)}
            placeholder={t('sessions.search')}
          />
        </SearchForm>
      </Group>

      <Paper withBorder shadow="sm" p="md" radius="md" mb="md">
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
                <Table.Th>{t('sessions.table.session_id')}</Table.Th>
                <Table.Th>{t('sessions.table.device_id')}</Table.Th>
                <Table.Th>{t('sessions.table.summary')}</Table.Th>
                <Table.Th>{t('sessions.table.time')}</Table.Th>
                <Table.Th ta="right">{t('sessions.table.turns')}</Table.Th>
                <Table.Th ta="right">{t('sessions.table.duration')}</Table.Th>
                <Table.Th>{t('sessions.table.actions')}</Table.Th>
              </Table.Tr>
            </Table.Thead>
            <Table.Tbody>
              {data.items.map((session) => {
                const totalMs = session.turns.reduce((sum, turn) => sum + getTurnDuration(turn), 0);
                const summary = getSessionSummary(session);
                return (
                  <Table.Tr
                    key={session.session_id}
                    style={{ cursor: 'pointer' }}
                    onClick={() => openDetailModal(session.session_id)}
                  >
                    <Table.Td>
                      <Text size="sm" style={{ fontFamily: 'monospace', fontSize: 12 }}>
                        {session.session_id}
                      </Text>
                    </Table.Td>
                    <Table.Td>
                      <Text size="sm" style={{ fontFamily: 'monospace', fontSize: 12, wordBreak: 'break-all' }}>
                        {formatDeviceId(session.device_id)}
                      </Text>
                    </Table.Td>
                    <Table.Td>
                      <Text size="sm" truncate="end" style={{ maxWidth: 240 }} title={summary}>
                        {summary}
                      </Text>
                    </Table.Td>
                    <Table.Td>
                      <Text size="sm" style={{ whiteSpace: 'nowrap' }}>
                        {session.create_datetime
                          ? dayjs(session.create_datetime).format('YYYY-MM-DD HH:mm')
                          : ''}
                      </Text>
                    </Table.Td>
                    <Table.Td>
                      <Text size="sm" ta="right">{session.turn_count}</Text>
                    </Table.Td>
                    <Table.Td>
                      <Text size="sm" ta="right">{formatDuration(totalMs)}</Text>
                    </Table.Td>
                    <Table.Td onClick={(e) => e.stopPropagation()}>
                      <Button size="xs" variant="subtle" onClick={() => openDetailModal(session.session_id)}>
                        {t('sessions.detail.title')}
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

      {data && data.total > 0 && (
        <DataPagination
          page={page}
          pageSize={pageSize}
          total={data.total}
          onPageChange={setPage}
          onPageSizeChange={handlePageSizeChange}
        />
      )}

      <Modal
        opened={detailOpened}
        onClose={closeDetail}
        title={t('sessions.detail.title')}
        size="80%"
        centered
      >
        {detailSessionId && <SessionDetail sessionId={detailSessionId} />}
      </Modal>
    </>
  );
}

import { listAccessLogs } from '@/api';
import { DataPagination, usePersistedPageSize } from '@/components/DataPagination';
import { SearchForm } from '@/components/SearchForm';
import type { ApiAccessLog } from '@/data/security';
import {
  Badge,
  Group,
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
      <Group justify="space-between" mb="lg">
        <Title>{t('admin.menu.access_logs')}</Title>
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
    </>
  );
}

function RouteComponent() {
  return <AccessLogsSection />;
}

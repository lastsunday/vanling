import { Group, Pagination, Select, Text } from '@mantine/core';
import { useState } from 'react';
import { useTranslation } from 'react-i18next';

const PAGE_SIZE_OPTIONS = [10, 20, 50, 100];
const PAGE_SIZE_KEY = 'pagination.pageSize';
const DEFAULT_PAGE_SIZE = 20;

export function usePersistedPageSize() {
  const [pageSize, setPageSizeState] = useState(() => {
    try {
      const stored = Number(localStorage.getItem(PAGE_SIZE_KEY));
      return PAGE_SIZE_OPTIONS.includes(stored) ? stored : DEFAULT_PAGE_SIZE;
    } catch {
      return DEFAULT_PAGE_SIZE;
    }
  });

  const setPageSize = (size: number) => {
    setPageSizeState(size);
    try {
      localStorage.setItem(PAGE_SIZE_KEY, String(size));
    } catch {
      // ignore storage errors
    }
  };

  return [pageSize, setPageSize] as const;
}

interface DataPaginationProps {
  page: number;
  pageSize: number;
  total: number;
  onPageChange: (page: number) => void;
  onPageSizeChange: (pageSize: number) => void;
}

export function DataPagination({
  page,
  pageSize,
  total,
  onPageChange,
  onPageSizeChange,
}: DataPaginationProps) {
  const { t } = useTranslation();
  const totalPages = Math.max(1, Math.ceil(total / pageSize));
  const start = total === 0 ? 0 : (page - 1) * pageSize + 1;
  const end = Math.min(page * pageSize, total);

  return (
    <Group justify="space-between" mt="md" wrap="wrap">
      <Text size="sm" c="dimmed">
        {t('pagination.total_info', { start, end, count: total })}
      </Text>
      <Group gap="md" wrap="wrap">
        <Select
          aria-label={t('pagination.per_page')}
          data={PAGE_SIZE_OPTIONS.map((n) => ({ value: String(n), label: String(n) }))}
          value={String(pageSize)}
          onChange={(v) => {
            const size = Number(v);
            if (v && PAGE_SIZE_OPTIONS.includes(size)) onPageSizeChange(size);
          }}
          w={80}
          allowDeselect={false}
        />
        <Pagination total={totalPages} value={page} onChange={onPageChange} />
      </Group>
    </Group>
  );
}

import { activateDevice, activateDeviceById, deleteDevice, disableDevice, enableDevice, listDevices } from '@/api';
import { DataPagination, usePersistedPageSize } from '@/components/DataPagination';
import { SearchForm } from '@/components/SearchForm';
import type { DeviceResult } from '@/data/device';
import {
  ActionIcon,
  Badge,
  Button,
  Card,
  CopyButton,
  Group,
  Modal,
  Paper,
  Select,
  SimpleGrid,
  Stack,
  Table,
  Text,
  TextInput,
  Title,
  Tooltip,
} from '@mantine/core';
import { useDisclosure } from '@mantine/hooks';
import { useQuery, useQueryClient } from '@tanstack/react-query';
import { createFileRoute } from '@tanstack/react-router';
import dayjs from 'dayjs';
import { useState } from 'react';
import { useTranslation } from 'react-i18next';

export const Route = createFileRoute('/_pathlessLayout/admin/devices/')({
  component: RouteComponent,
});

function RouteComponent() {
  const { t } = useTranslation();
  const queryClient = useQueryClient();

  const [page, setPage] = useState(1);
  const [pageSize, setPageSize] = usePersistedPageSize();
  const [searchInput, setSearchInput] = useState('');
  const [search, setSearch] = useState('');
  const [searchKey, setSearchKey] = useState(0);
  const [statusFilter, setStatusFilter] = useState<string | null>('all');
  const [opened, { open, close }] = useDisclosure(false);
  const [activationCode, setActivationCode] = useState('');
  const [activating, setActivating] = useState(false);
  const [activateError, setActivateError] = useState('');
  const [detailDevice, setDetailDevice] = useState<DeviceResult | null>(null);
  const [detailOpened, { open: openDetail, close: closeDetail }] = useDisclosure(false);
  const [confirmTarget, setConfirmTarget] = useState<{ device: DeviceResult; action: 'disable' | 'enable' | 'delete' } | null>(null);

  const { data, isLoading, isFetching } = useQuery({
    queryKey: ['devices', page, pageSize, search, statusFilter, searchKey],
    queryFn: () => listDevices(page, pageSize, search, statusFilter === 'all' ? undefined : statusFilter ?? undefined),
  });

  const handleSearch = () => {
    setSearch(searchInput);
    setPage(1);
    setSearchKey(k => k + 1);
  };

  const handlePageSizeChange = (size: number) => {
    setPageSize(size);
    setPage(1);
  };

  const handleActivate = async () => {
    if (!activationCode.trim()) return;
    setActivating(true);
    setActivateError('');
    try {
      await activateDevice(activationCode.trim());
      setActivationCode('');
      close();
      queryClient.invalidateQueries({ queryKey: ['devices'] });
    } catch (e) {
      setActivateError(`${e}`);
    } finally {
      setActivating(false);
    }
  };

  const handleActivateById = async (deviceId: string) => {
    try {
      await activateDeviceById(deviceId);
      queryClient.invalidateQueries({ queryKey: ['devices'] });
    } catch (e) {
      // ignore
    }
  };

  const confirmAction = async () => {
    if (!confirmTarget) return;
    const { device, action } = confirmTarget;
    try {
      if (action === 'disable') await disableDevice(device.uid);
      else if (action === 'enable') await enableDevice(device.uid);
      else if (action === 'delete') await deleteDevice(device.uid);
      setConfirmTarget(null);
      queryClient.invalidateQueries({ queryKey: ['devices'] });
    } catch (e) {
      setConfirmTarget(null);
    }
  };

  const openDetailModal = (device: DeviceResult) => {
    setDetailDevice(device);
    openDetail();
  };

  const stats = {
    total: data?.total ?? 0,
    activated: data?.items.filter((d) => d.activated && !d.disabled).length ?? 0,
    pending: data?.items.filter((d) => !d.activated && !d.disabled).length ?? 0,
    disabled: data?.items.filter((d) => d.disabled).length ?? 0,
  };

  const confirmMessage = confirmTarget
    ? confirmTarget.action === 'delete'
      ? t('devices.delete_confirmation', { id: confirmTarget.device.uid })
      : confirmTarget.action === 'disable'
        ? t('devices.disable_confirmation', { id: confirmTarget.device.uid })
        : t('devices.enable_confirmation', { id: confirmTarget.device.uid })
    : '';

  return (
    <>
      <Group justify="space-between" mb="lg">
        <Title>{t('devices.title')}</Title>
        <SearchForm
          onSubmit={handleSearch}
          submitLabel={t('devices.search_btn')}
          extra={<Button type="button" onClick={open}>{t('devices.activate_btn')}</Button>}
        >
          <TextInput
            value={searchInput}
            onChange={(e) => setSearchInput(e.currentTarget.value)}
            placeholder={t('devices.search')}
          />
        </SearchForm>
      </Group>

      <SimpleGrid cols={{ base: 1, sm: 2, md: 4 }} mb="lg">
        <Card withBorder padding="md" radius="md">
          <Text size="xs" c="dimmed" tt="uppercase" fw={700}>{t('devices.stat.total')}</Text>
          <Text fw={700} size="xl">{data?.total ?? '-'}</Text>
        </Card>
        <Card withBorder padding="md" radius="md">
          <Text size="xs" c="dimmed" tt="uppercase" fw={700}>{t('devices.stat.activated')}</Text>
          <Text fw={700} size="xl" c="green">{data ? stats.activated : '-'}</Text>
        </Card>
        <Card withBorder padding="md" radius="md">
          <Text size="xs" c="dimmed" tt="uppercase" fw={700}>{t('devices.stat.pending')}</Text>
          <Text fw={700} size="xl" c="gray">{data ? stats.pending : '-'}</Text>
        </Card>
        <Card withBorder padding="md" radius="md">
          <Text size="xs" c="dimmed" tt="uppercase" fw={700}>{t('devices.stat.disabled')}</Text>
          <Text fw={700} size="xl" c="red">{data ? stats.disabled : '-'}</Text>
        </Card>
      </SimpleGrid>

      <Group mb="md">
        <Select
          placeholder={t('devices.filter_all')}
          data={[
            { value: 'all', label: t('devices.filter_all') },
            { value: 'pending', label: t('devices.filter_pending') },
            { value: 'activated', label: t('devices.filter_activated') },
            { value: 'disabled', label: t('devices.filter_disabled') },
          ]}
          value={statusFilter}
          onChange={(v) => {
            setStatusFilter(v);
            setPage(1);
          }}
          w={160}
          clearable={false}
        />
      </Group>

      <Modal opened={opened} onClose={close} title={t('devices.activate_title')} centered>
        <Stack>
          <Text size="sm">{t('devices.activate_hint')}</Text>
          <TextInput
            placeholder={t('devices.activate_placeholder')}
            value={activationCode}
            onChange={(e) => {
              setActivationCode(e.currentTarget.value);
              setActivateError('');
            }}
            error={activateError}
            onKeyDown={(e) => {
              if (e.key === 'Enter') handleActivate();
            }}
            data-autofocus
          />
          <Group justify="flex-end">
            <Button variant="default" onClick={close}>
              {t('devices.cancel')}
            </Button>
            <Button onClick={handleActivate} loading={activating}>
              {t('devices.confirm')}
            </Button>
          </Group>
        </Stack>
      </Modal>

      <Modal
        opened={confirmTarget !== null}
        onClose={() => setConfirmTarget(null)}
        title={confirmTarget?.action === 'delete' ? t('devices.delete_btn') : confirmTarget?.action === 'disable' ? t('devices.disable_btn') : t('devices.enable_btn')}
        centered
      >
        <Stack>
          <Text size="sm">{confirmMessage}</Text>
          <Group justify="flex-end">
            <Button variant="default" onClick={() => setConfirmTarget(null)}>
              {t('devices.cancel')}
            </Button>
            <Button
              color={confirmTarget?.action === 'delete' ? 'red' : confirmTarget?.action === 'disable' ? 'red' : 'green'}
              onClick={confirmAction}
            >
              {t('devices.confirm_btn')}
            </Button>
          </Group>
        </Stack>
      </Modal>

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
                <Table.Th>{t('devices.table.uid')}</Table.Th>
                <Table.Th>{t('devices.table.device_uid')}</Table.Th>
                <Table.Th>{t('devices.table.board_type')}</Table.Th>
                <Table.Th>{t('devices.table.chip_model')}</Table.Th>
                <Table.Th>{t('devices.table.board_name')}</Table.Th>
                <Table.Th>{t('devices.table.activation_code')}</Table.Th>
                <Table.Th>{t('devices.table.firmware_version')}</Table.Th>
                <Table.Th>{t('devices.table.status')}</Table.Th>
                <Table.Th>{t('devices.table.last_online')}</Table.Th>
                <Table.Th>{t('devices.actions')}</Table.Th>
              </Table.Tr>
            </Table.Thead>
            <Table.Tbody>
              {data.items.map((device: DeviceResult) => (
                <Table.Tr
                  key={device.id}
                  style={{ cursor: 'pointer' }}
                  onClick={() => openDetailModal(device)}
                >
                  <Table.Td>
                    <Text size="sm" style={{ fontFamily: 'monospace', fontSize: 12 }}>
                      {device.id}
                    </Text>
                  </Table.Td>
                  <Table.Td>
                    <Text size="sm" style={{ fontFamily: 'monospace', fontSize: 12 }}>
                      {device.uid}
                    </Text>
                  </Table.Td>
                  <Table.Td>
                    <Text size="sm">{device.board_type}</Text>
                  </Table.Td>
                  <Table.Td>
                    <Text size="sm">{device.chip_model_name || '-'}</Text>
                  </Table.Td>
                  <Table.Td>
                    <Text size="sm">{device.board_name || '-'}</Text>
                  </Table.Td>
                  <Table.Td>
                    {!device.activated && device.activation_code ? (
                      <Group gap={4}>
                        <Text size="sm" style={{ fontFamily: 'monospace', fontSize: 12 }}>
                          {device.activation_code}
                        </Text>
                        <CopyButton value={device.activation_code}>
                          {({ copied, copy }) => (
                            <Tooltip label={copied ? t('devices.copied') : t('devices.copy')}>
                              <ActionIcon variant="subtle" color={copied ? 'teal' : 'gray'} size="sm" onClick={(e) => { e.stopPropagation(); copy(); }}>
                                {copied ? '✓' : '📋'}
                              </ActionIcon>
                            </Tooltip>
                          )}
                        </CopyButton>
                      </Group>
                    ) : (
                      <Text size="sm" c="dimmed">-</Text>
                    )}
                  </Table.Td>
                  <Table.Td>
                    <Text size="sm">{device.application_version || '-'}</Text>
                  </Table.Td>
                  <Table.Td>
                    {device.disabled ? (
                      <Badge color="red" variant="light">{t('devices.status.disabled')}</Badge>
                    ) : device.activated ? (
                      <Badge color="green" variant="light">{t('devices.status.activated')}</Badge>
                    ) : (
                      <Badge color="gray" variant="light">{t('devices.status.pending')}</Badge>
                    )}
                  </Table.Td>
                  <Table.Td>
                    <Text size="sm">
                      {device.last_online_datetime
                        ? dayjs(device.last_online_datetime).fromNow()
                        : '-'}
                    </Text>
                  </Table.Td>
                  <Table.Td onClick={(e) => e.stopPropagation()}>
                    <Group gap={4}>
                      {!device.activated && !device.disabled && (
                        <Button size="xs" variant="light" onClick={() => handleActivateById(device.uid)}>
                          {t('devices.activate_by_id_btn')}
                        </Button>
                      )}
                      {device.disabled ? (
                        <Button size="xs" variant="light" color="green" onClick={() => setConfirmTarget({ device, action: 'enable' })}>
                          {t('devices.enable_btn')}
                        </Button>
                      ) : (
                        <Button size="xs" variant="light" color="red" onClick={() => setConfirmTarget({ device, action: 'disable' })}>
                          {t('devices.disable_btn')}
                        </Button>
                      )}
                      <Button size="xs" variant="light" color="red" onClick={() => setConfirmTarget({ device, action: 'delete' })}>
                        {t('devices.delete_btn')}
                      </Button>
                      <Button size="xs" variant="subtle" onClick={() => openDetailModal(device)}>
                        {t('devices.view_detail')}
                      </Button>
                    </Group>
                  </Table.Td>
                </Table.Tr>
              ))}
            </Table.Tbody>
          </Table>
        </Paper>
      )}

      {data && data.items.length === 0 && (
        <Text ta="center" py="xl" c="dimmed">
          {t('devices.no_devices')}
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

      <Modal opened={detailOpened} onClose={closeDetail} title={t('devices.detail_title')} size="lg" centered>
        {detailDevice && (
          <Stack>
            <Group>
              <Text fw={500} w={120}>{t('devices.table.uid')}</Text>
              <Text size="sm" style={{ fontFamily: 'monospace' }}>{detailDevice.id}</Text>
            </Group>
            <Group>
              <Text fw={500} w={120}>{t('devices.detail.device_uid')}</Text>
              <Text size="sm" style={{ fontFamily: 'monospace' }}>{detailDevice.uid}</Text>
            </Group>
            <Group>
              <Text fw={500} w={120}>{t('devices.detail.client_id')}</Text>
              <Text size="sm" style={{ fontFamily: 'monospace' }}>{detailDevice.client_id || '-'}</Text>
            </Group>
            <Group>
              <Text fw={500} w={120}>{t('devices.table.board_type')}</Text>
              <Text size="sm">{detailDevice.board_type}</Text>
            </Group>
            <Group>
              <Text fw={500} w={120}>{t('devices.detail.board_name')}</Text>
              <Text size="sm">{detailDevice.board_name || '-'}</Text>
            </Group>
            <Group>
              <Text fw={500} w={120}>{t('devices.table.mac_address')}</Text>
              <Text size="sm" style={{ fontFamily: 'monospace' }}>{detailDevice.mac_address || '-'}</Text>
            </Group>
            <Group>
              <Text fw={500} w={120}>{t('devices.detail.chip_model')}</Text>
              <Text size="sm">{detailDevice.chip_model_name || '-'}</Text>
            </Group>
            <Group>
              <Text fw={500} w={120}>{t('devices.table.status')}</Text>
              {detailDevice.disabled ? (
                <Badge color="red" variant="light">{t('devices.status.disabled')}</Badge>
              ) : detailDevice.activated ? (
                <Badge color="green" variant="light">{t('devices.status.activated')}</Badge>
              ) : (
                <Badge color="gray" variant="light">{t('devices.status.pending')}</Badge>
              )}
            </Group>
            <Group>
              <Text fw={500} w={120}>{t('devices.table.firmware_version')}</Text>
              <Text size="sm">{detailDevice.application_version || '-'}</Text>
            </Group>
            <Group>
              <Text fw={500} w={120}>{t('devices.table.activation_code')}</Text>
              <Text size="sm" style={{ fontFamily: 'monospace' }}>
                {detailDevice.activation_code ? (
                  <Group gap={4}>
                    <span>{detailDevice.activation_code}</span>
                    <CopyButton value={detailDevice.activation_code}>
                      {({ copied, copy }) => (
                        <ActionIcon variant="subtle" color={copied ? 'teal' : 'gray'} size="sm" onClick={(e) => { e.stopPropagation(); copy(); }}>
                          {copied ? '✓' : '📋'}
                        </ActionIcon>
                      )}
                    </CopyButton>
                  </Group>
                ) : '-'}
              </Text>
            </Group>
            <Group>
              <Text fw={500} w={120}>{t('devices.detail.activation_code_expires_at')}</Text>
              <Text size="sm">
                {detailDevice.activation_code_expires_at
                  ? dayjs(detailDevice.activation_code_expires_at).format('YYYY-MM-DD HH:mm')
                  : '-'}
              </Text>
            </Group>
            <Group>
              <Text fw={500} w={120}>{t('devices.detail.user_agent')}</Text>
              <Text size="sm">{detailDevice.user_agent || '-'}</Text>
            </Group>
            <Group>
              <Text fw={500} w={120}>{t('devices.table.last_online')}</Text>
              <Text size="sm">
                {detailDevice.last_online_datetime
                  ? dayjs(detailDevice.last_online_datetime).format('YYYY-MM-DD HH:mm:ss')
                  : '-'}
              </Text>
            </Group>
            <Group>
              <Text fw={500} w={120}>{t('devices.table.created')}</Text>
              <Text size="sm">
                {detailDevice.create_datetime
                  ? dayjs(detailDevice.create_datetime).format('YYYY-MM-DD HH:mm:ss')
                  : '-'}
              </Text>
            </Group>
            <Group justify="flex-end" mt="md">
              {!detailDevice.activated && !detailDevice.disabled && (
                <Button variant="light" onClick={() => { closeDetail(); handleActivateById(detailDevice.uid); }}>
                  {t('devices.activate_by_id_btn')}
                </Button>
              )}
              {detailDevice.disabled ? (
                <Button variant="light" color="green" onClick={() => { closeDetail(); setConfirmTarget({ device: detailDevice, action: 'enable' }); }}>
                  {t('devices.enable_btn')}
                </Button>
              ) : (
                <Button variant="light" color="red" onClick={() => { closeDetail(); setConfirmTarget({ device: detailDevice, action: 'disable' }); }}>
                  {t('devices.disable_btn')}
                </Button>
              )}
              <Button variant="light" color="red" onClick={() => { closeDetail(); setConfirmTarget({ device: detailDevice, action: 'delete' }); }}>
                {t('devices.delete_btn')}
              </Button>
            </Group>
          </Stack>
        )}
      </Modal>
    </>
  );
}

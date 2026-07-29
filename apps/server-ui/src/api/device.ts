import type { ActivateResult, DeviceListResult } from '@/data/device';
import { delJson, getJson, postJson } from './http';

export async function activateDevice(activationCode: string): Promise<ActivateResult> {
  return postJson('/api/devices/activate', { activation_code: activationCode });
}

export async function activateDeviceById(deviceId: string): Promise<ActivateResult> {
  return postJson(`/api/devices/${deviceId}/activate`, {});
}

export async function disableDevice(deviceId: string): Promise<void> {
  return postJson(`/api/devices/${deviceId}/disable`, {});
}

export async function enableDevice(deviceId: string): Promise<void> {
  return postJson(`/api/devices/${deviceId}/enable`, {});
}

export async function deleteDevice(deviceId: string): Promise<void> {
  return delJson(`/api/devices/${deviceId}`);
}

export async function listDevices(
  page?: number,
  pageSize?: number,
  search?: string,
  status?: string,
): Promise<DeviceListResult> {
  return getJson('/api/devices', {
    page,
    page_size: pageSize,
    search: search || undefined,
    status: status || undefined,
  } as Record<string, unknown>);
}

import type { Frame } from "@/data/frame";
import type { PaginatedData } from "@/data/round";
import type { RoundData } from "@/data/round-data";
import type { SessionListItem, SessionRound } from "@/data/session";
import { getJson, instance } from "./http";

export async function listSessions(params?: {
  search?: string;
  date_from?: string;
  date_to?: string;
  sort_order?: 'asc' | 'desc';
  page?: number;
  page_size?: number;
}): Promise<PaginatedData<SessionListItem>> {
  return getJson("/api/record/sessions", params as Record<string, unknown>);
}

export async function getSessionRounds(sessionId: string): Promise<SessionRound[]> {
  return getJson(`/api/record/sessions/${sessionId}/rounds`);
}

export async function listRoundData(roundId: string): Promise<RoundData[]> {
  return getJson(`/api/record/rounds/${roundId}/data`);
}

export async function getAudioBlob(roundId: string, dataId: string): Promise<Blob> {
  const resp = await instance.get(
    `/api/record/rounds/${roundId}/data/${dataId}/blob`,
    { responseType: "blob" },
  );
  return resp.data;
}

export async function listFrames(
  roundId: string,
  params?: { page?: number; page_size?: number },
): Promise<PaginatedData<Frame>> {
  return getJson(
    `/api/record/rounds/${roundId}/frames`,
    params as Record<string, unknown>,
  );
}

export interface RoundData {
  id: string;
  round_id: string;
  data_type: string;
  data: string | null;
  text: string | null;
  metadata: Record<string, unknown> | null;
  create_datetime: string | null;
  update_datetime: string | null;
}

export interface Frame {
  id: number;
  round_id: string | null;
  session_id: string | null;
  seq: number;
  dir: string;
  kind: string;
  detail: string | null;
  elapsed_us: number | null;
  data: number[] | null;
  create_datetime: string | null;
  update_datetime: string | null;
}

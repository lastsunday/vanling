export interface TurnStep {
  step: string;
  has_data: boolean;
  text: string | null;
  duration_ms: number | null;
  audio_duration_ms: number | null;
  mode?: string | null;
}

export interface TurnSummary {
  turn_index: number;
  round_id: string;
  create_datetime: string | null;
  steps: TurnStep[];
}

export interface SessionListItem {
  session_id: string;
  create_datetime: string | null;
  update_datetime: string | null;
  turn_count: number;
  turns: TurnSummary[];
}

export interface SessionRound {
  round_id: string;
  create_datetime: string | null;
  steps: TurnStep[];
}

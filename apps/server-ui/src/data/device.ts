export interface DeviceResult {
  id: string;
  uid: string;
  client_id: string | null;
  user_agent: string | null;
  mac_address: string | null;
  chip_model_name: string | null;
  application_name: string | null;
  application_version: string;
  board_type: string;
  board_name: string | null;
  activated: boolean;
  disabled: boolean;
  activation_code: string | null;
  activation_code_expires_at: string | null;
  user_id: string | null;
  last_online_datetime: string | null;
  create_datetime: string | null;
  update_datetime: string | null;
}

export interface ActivateResult {
  uid: string;
  board_type: string;
  board_name: string | null;
  activated: boolean;
  token: string;
}

export interface DeviceListResult {
  items: DeviceResult[];
  total: number;
  page: number;
  page_size: number;
}

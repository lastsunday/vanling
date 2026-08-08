export interface PageData<T> {
  items: T[];
  total: number;
  page: number;
  page_size: number;
}

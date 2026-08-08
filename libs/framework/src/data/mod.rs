pub mod json;
pub mod path;
pub mod query;
pub mod serder;
pub mod valid;

use axum::response::IntoResponse;
use sea_orm::{DatabaseConnection, DbErr, EntityTrait, PaginatorTrait, Select};
use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};

pub use serder::{deserialize_number, empty_string_as_none};

pub const DEFAULT_PAGE: u64 = 1;
pub const DEFAULT_PAGE_SIZE: u64 = 20;
pub const MAX_PAGE_SIZE: u64 = 100;

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct ApiResponse<T> {
    pub code: i32,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<T>,
}

impl<T> ApiResponse<T> {
    pub fn new(code: i32, message: String, data: Option<T>) -> Self {
        Self {
            code,
            message,
            data,
        }
    }

    pub fn success(data: Option<T>) -> Self {
        Self::new(0, String::new(), data)
    }

    pub fn error<M: AsRef<str>>(code: i32, message: M) -> Self {
        Self::new(code, String::from(message.as_ref()), None)
    }

    pub fn failure<M: AsRef<str>>(message: M) -> Self {
        Self::new(-1, String::from(message.as_ref()), None)
    }
}

impl<T: Serialize> IntoResponse for ApiResponse<T> {
    fn into_response(self) -> axum::response::Response {
        axum::Json(self).into_response()
    }
}

/// 分页查询参数。页码从 1 开始，`page_size` 范围 1-100。
#[derive(Debug, Default, Deserialize, Serialize, Clone, PartialEq, Eq, IntoParams, ToSchema)]
#[into_params(parameter_in = Query)]
pub struct PageParam {
    /// 页码，从 1 开始
    #[param(example = 1, default = 1)]
    pub page: Option<u64>,
    /// 每页条数，范围 1-100，默认 20
    #[param(example = 20, default = 20)]
    pub page_size: Option<u64>,
}

impl PageParam {
    pub fn new(page: Option<u64>, page_size: Option<u64>) -> Self {
        Self { page, page_size }
    }

    /// 归一化后的页码（最小 1）
    pub fn page(&self) -> u64 {
        self.page.unwrap_or(DEFAULT_PAGE).max(DEFAULT_PAGE)
    }

    /// 归一化后的每页条数（clamp 到 1-100）
    pub fn page_size(&self) -> u64 {
        self.page_size
            .unwrap_or(DEFAULT_PAGE_SIZE)
            .clamp(1, MAX_PAGE_SIZE)
    }

    /// 偏移量
    pub fn offset(&self) -> u64 {
        (self.page() - 1) * self.page_size()
    }
}

#[derive(Debug, Serialize, ToSchema)]
pub struct PageData<T> {
    pub items: Vec<T>,
    pub total: u64,
    pub page: u64,
    pub page_size: u64,
}

impl<T> PageData<T> {
    pub fn new(items: Vec<T>, total: u64, pagination: &PageParam) -> Self {
        Self {
            items,
            total,
            page: pagination.page(),
            page_size: pagination.page_size(),
        }
    }
}

/// 对查询执行分页，返回统一的分页结果。
pub async fn paginate<M>(
    query: Select<M>,
    conn: &DatabaseConnection,
    pagination: &PageParam,
) -> Result<PageData<M::Model>, DbErr>
where
    M: EntityTrait,
    M::Model: Send + Sync + 'static,
{
    let page_size = pagination.page_size();
    let paginator = query.paginate(conn, page_size);
    let items = paginator.fetch_page(pagination.page() - 1).await?;
    let total = paginator.num_items().await?;
    Ok(PageData::new(items, total, pagination))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn page_query_defaults() {
        let p = PageParam::new(None, None);
        assert_eq!(p.page(), DEFAULT_PAGE);
        assert_eq!(p.page_size(), DEFAULT_PAGE_SIZE);
        assert_eq!(p.offset(), 0);
    }

    #[test]
    fn page_query_clamps() {
        assert_eq!(PageParam::new(Some(0), Some(0)).page(), 1);
        assert_eq!(PageParam::new(Some(0), Some(0)).page_size(), 1);
        assert_eq!(
            PageParam::new(Some(2), Some(101)).page_size(),
            MAX_PAGE_SIZE
        );
        assert_eq!(PageParam::new(Some(3), Some(10)).offset(), 20);
    }
}

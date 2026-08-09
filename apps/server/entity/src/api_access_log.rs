//! `SeaORM` Entity, API access log (append-only).

use chrono::Local;
use framework::id::gen_id;
use sea_orm::{ActiveValue, entity::prelude::*, prelude::async_trait::async_trait};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Eq, Serialize, Deserialize, ToSchema)]
#[sea_orm(table_name = "api_access_log")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: String,
    pub request_id: String,
    pub method: String,
    pub path: String,
    pub query: Option<String>,
    pub ip: Option<String>,
    pub principal_id: Option<String>,
    pub name: Option<String>,
    pub status: i32,
    pub duration_ms: i64,
    pub response_size: Option<i64>,
    pub user_agent: Option<String>,
    #[schema(schema_with = crate::schema::date_time_with_time_zone_or_null_schema)]
    pub create_datetime: Option<DateTimeWithTimeZone>,
    #[schema(schema_with = crate::schema::date_time_with_time_zone_or_null_schema)]
    pub update_datetime: Option<DateTimeWithTimeZone>,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

#[async_trait]
impl ActiveModelBehavior for ActiveModel {
    async fn before_save<C>(mut self, _db: &C, insert: bool) -> Result<Self, DbErr>
    where
        C: ConnectionTrait,
    {
        if insert {
            self.id = ActiveValue::Set(gen_id());
            self.create_datetime = ActiveValue::Set(Some(Local::now().fixed_offset()));
        }
        self.update_datetime = ActiveValue::Set(Some(Local::now().fixed_offset()));
        Ok(self)
    }
}

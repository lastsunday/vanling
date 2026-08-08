//! `SeaORM` Entity, security event audit trail (append-only).

use chrono::Local;
use framework::id::gen_id;
use sea_orm::{ActiveValue, entity::prelude::*, prelude::async_trait::async_trait};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, EnumIter, DeriveActiveEnum, Serialize, Deserialize, ToSchema,
)]
#[sea_orm(
    rs_type = "String",
    db_type = "String(StringLen::N(64))",
    enum_name = "security_event_type"
)]
#[serde(rename_all = "snake_case")]
pub enum SecurityEventType {
    #[sea_orm(string_value = "rate_limited")]
    RateLimited,
    #[sea_orm(string_value = "rate_limit_near")]
    RateLimitNear,
    #[sea_orm(string_value = "auth_login_success")]
    AuthLoginSuccess,
    #[sea_orm(string_value = "auth_login_failure")]
    AuthLoginFailure,
}

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Eq, Serialize, Deserialize, ToSchema)]
#[sea_orm(table_name = "security_event")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: String,
    pub event_type: SecurityEventType,
    pub ip: Option<String>,
    pub path: Option<String>,
    pub account: Option<String>,
    pub retry_after_ms: Option<i64>,
    pub limit: Option<i64>,
    pub remaining: Option<i64>,
    pub window_secs: Option<i64>,
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

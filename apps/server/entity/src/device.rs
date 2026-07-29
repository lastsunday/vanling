use chrono::Local;
use framework::id::gen_id;
use sea_orm::{ActiveValue, entity::prelude::*, prelude::async_trait::async_trait};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::schema::date_time_with_time_zone_or_null_schema;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Eq, Serialize, Deserialize, ToSchema)]
#[sea_orm(table_name = "device")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: String,
    #[sea_orm(unique)]
    pub device_id: String,
    pub client_id: Option<String>,
    pub user_agent: Option<String>,
    pub mac_address: Option<String>,
    pub chip_model_name: Option<String>,
    pub application_name: Option<String>,
    pub application_version: String,
    pub board_type: String,
    pub board_name: Option<String>,
    pub activated: bool,
    pub disabled: bool,
    pub activation_code: Option<String>,
    #[schema(schema_with = date_time_with_time_zone_or_null_schema)]
    pub activation_code_expires_at: Option<DateTimeWithTimeZone>,
    pub user_id: Option<String>,
    #[schema(schema_with = date_time_with_time_zone_or_null_schema)]
    pub last_online_datetime: Option<DateTimeWithTimeZone>,
    #[schema(schema_with = date_time_with_time_zone_or_null_schema)]
    pub create_datetime: Option<DateTimeWithTimeZone>,
    #[schema(schema_with = date_time_with_time_zone_or_null_schema)]
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
        let now = Local::now().fixed_offset();
        if insert {
            self.id = ActiveValue::Set(gen_id());
            self.create_datetime = ActiveValue::Set(Some(now));
        }
        self.update_datetime = ActiveValue::Set(Some(now));
        Ok(self)
    }
}

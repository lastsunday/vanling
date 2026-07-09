use chrono::Local;
use sea_orm::{ActiveValue, entity::prelude::*, prelude::async_trait::async_trait};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::schema::date_time_with_time_zone_or_null_schema;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Eq, Serialize, Deserialize, ToSchema)]
#[sea_orm(table_name = "round_data")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: String,
    pub round_id: String,
    pub data_type: String,
    pub data: Option<Vec<u8>>,
    pub text: Option<String>,
    #[sea_orm(column_type = "JsonBinary", nullable)]
    pub metadata: Option<Json>,
    #[schema(schema_with = date_time_with_time_zone_or_null_schema)]
    pub create_datetime: Option<DateTimeWithTimeZone>,
    #[schema(schema_with = date_time_with_time_zone_or_null_schema)]
    pub update_datetime: Option<DateTimeWithTimeZone>,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "super::round::Entity",
        from = "Column::RoundId",
        to = "super::round::Column::Id"
    )]
    Round,
}

#[async_trait]
impl ActiveModelBehavior for ActiveModel {
    async fn before_save<C>(mut self, _db: &C, insert: bool) -> Result<Self, DbErr>
    where
        C: ConnectionTrait,
    {
        let now = Local::now().fixed_offset();
        if insert {
            self.create_datetime = ActiveValue::Set(Some(now));
        }
        self.update_datetime = ActiveValue::Set(Some(now));
        Ok(self)
    }
}

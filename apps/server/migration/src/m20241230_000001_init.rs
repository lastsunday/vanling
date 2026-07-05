use entity::*;
use sea_orm::Set;
use sea_orm::entity::*;
use sea_orm_migration::{async_trait::async_trait, prelude::*, schema::*};

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();
        manager
            .create_table(
                Table::create()
                    .table(Config::Table)
                    .if_not_exists()
                    .col(string_uniq(Config::Id))
                    .col(string_uniq(Config::Key))
                    .col(json_binary_null(Config::Value))
                    .col(timestamp_with_time_zone_null(Config::CreateDatetime))
                    .col(timestamp_with_time_zone_null(Config::UpdateDatetime))
                    .primary_key(Index::create().name("pk-config-id").col(Config::Id))
                    .to_owned(),
            )
            .await?;
        manager
            .create_table(
                Table::create()
                    .table(User::Table)
                    .if_not_exists()
                    .col(string_uniq(User::Id))
                    .col(string_uniq(User::Account))
                    .col(string(User::Password))
                    .col(string_null(User::Email))
                    .col(boolean(User::Enable))
                    .col(timestamp_with_time_zone_null(User::CreateDatetime))
                    .col(timestamp_with_time_zone_null(User::UpdateDatetime))
                    .primary_key(Index::create().name("pk-user-id").col(User::Id))
                    .to_owned(),
            )
            .await?;
        manager
            .create_table(
                Table::create()
                    .table(Session::Table)
                    .if_not_exists()
                    .col(string_uniq(Session::Id))
                    .col(timestamp_with_time_zone_null(Session::CreateDatetime))
                    .col(timestamp_with_time_zone_null(Session::UpdateDatetime))
                    .primary_key(Index::create().name("pk-session-id").col(Session::Id))
                    .to_owned(),
            )
            .await?;
        manager
            .create_table(
                Table::create()
                    .table(Round::Table)
                    .if_not_exists()
                    .col(string_uniq(Round::Id))
                    .col(string(Round::SessionId))
                    .col(json_binary_null(Round::ClientInfo))
                    .col(string_null(Round::Status))
                    .col(timestamp_with_time_zone_null(Round::CreateDatetime))
                    .col(timestamp_with_time_zone_null(Round::UpdateDatetime))
                    .primary_key(Index::create().name("pk-round-id").col(Round::Id))
                    .to_owned(),
            )
            .await?;
        manager
            .create_table(
                Table::create()
                    .table(RoundData::Table)
                    .if_not_exists()
                    .col(string_uniq(RoundData::Id))
                    .col(string(RoundData::RoundId))
                    .col(string(RoundData::DataType))
                    .col(blob_null(RoundData::Data))
                    .col(string_null(RoundData::Text))
                    .col(json_binary_null(RoundData::Metadata))
                    .col(timestamp_with_time_zone_null(RoundData::CreateDatetime))
                    .col(timestamp_with_time_zone_null(RoundData::UpdateDatetime))
                    .primary_key(Index::create().name("pk-round-data-id").col(RoundData::Id))
                    .to_owned(),
            )
            .await?;
        manager
            .create_table(
                Table::create()
                    .table(Frame::Table)
                    .if_not_exists()
                    .col(integer(Frame::Id).auto_increment().primary_key())
                    .col(string_null(Frame::RoundId))
                    .col(string_null(Frame::SessionId))
                    .col(integer(Frame::Seq))
                    .col(string(Frame::Dir))
                    .col(string(Frame::Kind))
                    .col(string_null(Frame::Detail))
                    .col(blob_null(Frame::Data))
                    .col(big_integer_null(Frame::ElapsedUs))
                    .col(timestamp_with_time_zone_null(Frame::CreateDatetime))
                    .col(timestamp_with_time_zone_null(Frame::UpdateDatetime))
                    .to_owned(),
            )
            .await?;
        let root_user = user::ActiveModel {
            account: Set("root".to_string()),
            password: Set(
                "$2b$12$n7NaDXwHdpCQI5LlsM1viuDJWZWofuhz/HnGAi8X.BmPRIuHvaXUy".to_string(),
            ),
            enable: Set(true),
            ..Default::default()
        };
        root_user.insert(db).await?;
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(Frame::Table).to_owned())
            .await?;
        manager
            .drop_table(Table::drop().table(RoundData::Table).to_owned())
            .await?;
        manager
            .drop_table(Table::drop().table(Round::Table).to_owned())
            .await?;
        manager
            .drop_table(Table::drop().table(Session::Table).to_owned())
            .await?;
        manager
            .drop_table(Table::drop().table(Config::Table).to_owned())
            .await?;
        manager
            .drop_table(Table::drop().table(User::Table).to_owned())
            .await?;
        Ok(())
    }
}

#[derive(DeriveIden)]
enum Config {
    Table,
    Id,
    Key,
    Value,
    CreateDatetime,
    UpdateDatetime,
}

#[derive(DeriveIden)]
enum User {
    Table,
    Id,
    Account,
    Password,
    Email,
    Enable,
    CreateDatetime,
    UpdateDatetime,
}

#[derive(DeriveIden)]
enum Session {
    Table,
    Id,
    CreateDatetime,
    UpdateDatetime,
}

#[derive(DeriveIden)]
enum Round {
    Table,
    Id,
    SessionId,
    ClientInfo,
    Status,
    CreateDatetime,
    UpdateDatetime,
}

#[derive(DeriveIden)]
enum RoundData {
    Table,
    Id,
    RoundId,
    DataType,
    Data,
    Text,
    Metadata,
    CreateDatetime,
    UpdateDatetime,
}

#[derive(DeriveIden)]
pub enum Frame {
    Table,
    Id,
    RoundId,
    Seq,
    Dir,
    Kind,
    Detail,
    Data,
    SessionId,
    CreateDatetime,
    UpdateDatetime,
    ElapsedUs,
}

use chrono::{DateTime, FixedOffset};
use framework::database;
use migration::MigratorTrait;
use service::AppState;
use testcontainers::ContainerAsync;
use testcontainers::runners::AsyncRunner;
use testcontainers_modules::postgres::Postgres;

#[allow(dead_code)]
pub async fn setup_database() -> (Option<ContainerAsync<Postgres>>, AppState) {
    match std::env::var("TEST_DATABASE").as_deref() {
        Ok("pg") => {
            let container = Postgres::default().start().await.unwrap();
            let host_port = container.get_host_port_ipv4(5432).await.unwrap();
            let database_url =
                format!("postgres://postgres:postgres@127.0.0.1:{host_port}/postgres");
            let conn: sea_orm::DatabaseConnection =
                database::establish_connection(&database_url).await.unwrap();
            migration::Migrator::up(&conn, None).await.unwrap();
            let state = AppState { conn };
            (Some(container), state)
        }
        _ => {
            let container = None;
            let database_url = "sqlite::memory:";
            let conn: sea_orm::DatabaseConnection =
                database::establish_connection(database_url).await.unwrap();
            migration::Migrator::up(&conn, None).await.unwrap();
            let state = AppState { conn };
            (container, state)
        }
    }
}

#[allow(dead_code)]
pub async fn tear_down(container: Option<ContainerAsync<Postgres>>) {
    if let Some(container) = container {
        container.rm().await.unwrap();
    }
}

#[allow(dead_code)]
pub fn str_to_datetime(value: String) -> Option<DateTime<FixedOffset>> {
    DateTime::parse_from_rfc3339(&value).ok()
}

#[allow(dead_code)]
pub fn datetime_to_str(datetime: Option<DateTime<FixedOffset>>) -> String {
    match datetime {
        Some(item) => item.to_rfc3339(),
        None => "".to_owned(),
    }
}

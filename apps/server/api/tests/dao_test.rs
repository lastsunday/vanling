mod common;
use common::{setup_database, tear_down};
use entity::{prelude::*, user};
use sea_orm::{ActiveValue::Set, prelude::*};

#[tokio::test]
async fn test_find_by_id() {
    let (container, state) = setup_database().await;
    let root_user = user::ActiveModel {
        account: Set("root2".to_string()),
        password: Set("$2b$12$n7NaDXwHdpCQI5LlsM1viuDJWZWofuhz/HnGAi8X.BmPRIuHvaXUy".to_string()),
        enable: Set(true),
        ..Default::default()
    };
    root_user.insert(&state.conn).await.unwrap();
    let user: Option<user::Model> = User::find()
        .filter(user::Column::Account.eq("root"))
        .one(&state.conn)
        .await
        .unwrap();
    assert!(user.is_some());
    let _ = &state.conn.close().await.unwrap();
    tear_down(container).await;
}

mod common;

use api::config::ListeningAddr;
use api::config::ListeningPort;
use api::config::server::ServerConfig;
use api::start_app;
use either::Either;
use framework::auth::Jwt;
use framework::config::auth::AuthConfig;
use std::sync::Arc;
use std::time::Duration;

use common::{setup_database, tear_down};

#[tokio::test]
async fn test_graceful_shutdown_on_cancel() {
    let (container, state) = setup_database().await;
    Jwt::init(Arc::new(AuthConfig {
        access_token_secret: Some(String::from("QLjJTeVblAlM47de")),
        access_token_expires_in: Some(28800),
        refresh_token_secret: Some(String::from("N8lI0uitNzJl6vYK")),
        refresh_token_expires_in: Some(15897600),
        audience: Some(String::from("audience")),
        issuer: Some(String::from("issuer")),
        client_id: Some(String::from("d1aicsr57dijo7h963ig")),
        client_secret: Some(String::from("ujTgh2lEQYy0PXhK")),
    }));
    let ct = state.cancellation_token.clone();
    let server_config = Arc::new(ServerConfig {
        server_name: Some(String::from("test")),
        address: Some(ListeningAddr {
            addrs: Either::Left("127.0.0.1".parse().unwrap()),
        }),
        port: Some(ListeningPort {
            ports: Either::Left(0),
        }),
    });
    let handle = tokio::spawn(start_app(server_config, state, ct.clone()));
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert!(!handle.is_finished(), "server should still be running");
    ct.cancel();
    let result = tokio::time::timeout(Duration::from_secs(5), handle)
        .await
        .expect("server should shut down within timeout");
    assert!(
        result.expect("start_app should not panic").is_ok(),
        "start_app should return Ok after cancellation"
    );
    tear_down(container).await;
}

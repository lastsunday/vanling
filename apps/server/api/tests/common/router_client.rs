use std::{borrow::Cow, collections::HashMap, sync::Arc};

use axum::{
    Router,
    body::Body,
    http::{self, HeaderName, HeaderValue, Request, StatusCode},
};
use futures::{StreamExt, stream::BoxStream};
use http_body_util::BodyExt;
use rmcp::transport::common::http_header::HEADER_LAST_EVENT_ID;
use rmcp::{
    model::ClientJsonRpcMessage, transport::streamable_http_client::StreamableHttpPostResponse,
};
use rmcp::{
    model::ServerJsonRpcMessage,
    transport::{
        common::http_header::{EVENT_STREAM_MIME_TYPE, HEADER_SESSION_ID, JSON_MIME_TYPE},
        streamable_http_client::{AuthRequiredError, StreamableHttpClient, StreamableHttpError},
    },
};
use sse_stream::{Error as SseError, Sse, SseStream};
use tower::ServiceExt;

#[allow(dead_code)]
#[derive(Clone)]
pub struct RouterClient {
    pub router: Router,
}

#[allow(dead_code)]
#[derive(Debug, thiserror::Error)]
pub struct MyError {
    msg: String,
}

impl std::fmt::Display for MyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.msg)
    }
}

impl StreamableHttpClient for RouterClient {
    type Error = MyError;

    // reference https://github.com/modelcontextprotocol/rust-sdk/blob/main/crates/rmcp/src/transport/common/reqwest/streamable_http_client.rs
    async fn post_message(
        &self,
        uri: Arc<str>,
        message: ClientJsonRpcMessage,
        session_id: Option<Arc<str>>,
        auth_header: Option<String>,
        custom_headers: HashMap<HeaderName, HeaderValue>,
    ) -> Result<
        rmcp::transport::streamable_http_client::StreamableHttpPostResponse,
        rmcp::transport::streamable_http_client::StreamableHttpError<Self::Error>,
    > {
        let mut builder = Request::builder()
            .uri(uri.to_string())
            .method("POST")
            .header(http::header::CONTENT_TYPE, mime::APPLICATION_JSON.as_ref())
            .header(
                http::header::ACCEPT,
                [
                    mime::APPLICATION_JSON.as_ref(),
                    mime::TEXT_EVENT_STREAM.as_ref(),
                ]
                .join(", "),
            );
        // rmcp 2.1.0 requires a Host header for DNS rebinding protection
        builder = builder.header(http::header::HOST, "localhost");
        if let Some(auth_token) = auth_header {
            builder = builder.header(http::header::AUTHORIZATION, format!("Bearer {auth_token}"));
        }
        if let Some(session_id) = session_id {
            builder = builder.header(HEADER_SESSION_ID, session_id.as_ref());
        }
        for (name, value) in &custom_headers {
            builder = builder.header(name, value);
        }

        let router = self.router.clone();

        let mut response = router
            .oneshot(
                builder
                    .body(Body::from(serde_json::to_string(&message).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();

        let status = response.status();
        if let Some(header) = response.headers().get(http::header::WWW_AUTHENTICATE)
            && status == StatusCode::UNAUTHORIZED
        {
            let header = header
                .to_str()
                .map_err(|_| {
                    StreamableHttpError::UnexpectedServerResponse(Cow::from(
                        "invalid www-authenticate header value",
                    ))
                })?
                .to_string();
            return Err(StreamableHttpError::AuthRequired(AuthRequiredError::new(
                header,
            )));
        }
        if status.is_client_error() || status.is_server_error() {
            return Err(StreamableHttpError::UnexpectedServerResponse(
                format!("error status code = {}", status).into(),
            ));
        }
        if matches!(status, StatusCode::ACCEPTED | StatusCode::NO_CONTENT) {
            return Ok(StreamableHttpPostResponse::Accepted);
        }
        let content_type = response.headers().get(http::header::CONTENT_TYPE);
        let session_id = response.headers().get(HEADER_SESSION_ID);
        let session_id = session_id
            .and_then(|v| v.to_str().ok())
            .map(|s| s.to_string());
        match content_type {
            Some(ct) if ct.as_bytes().starts_with(EVENT_STREAM_MIME_TYPE.as_bytes()) => {
                let (_parts, body) = response.into_parts();
                let event_stream = SseStream::new(body).boxed();
                Ok(StreamableHttpPostResponse::Sse(event_stream, session_id))
            }
            Some(ct) if ct.as_bytes().starts_with(JSON_MIME_TYPE.as_bytes()) => {
                let bytes = match response.body_mut().collect().await {
                    Ok(collected) => collected.to_bytes(),
                    Err(e) => {
                        return Err(StreamableHttpError::UnexpectedServerResponse(
                            format!("{}", e).into(),
                        ));
                    }
                };

                // Deserialize the bytes into a Value or a specific struct
                let message: ServerJsonRpcMessage = match serde_json::from_slice(&bytes) {
                    Ok(value) => value,
                    Err(e) => {
                        return Err(StreamableHttpError::UnexpectedServerResponse(
                            format!("{}", e).into(),
                        ));
                    }
                };
                Ok(StreamableHttpPostResponse::Json(message, session_id))
            }
            _ => {
                // unexpected content type
                tracing::error!("unexpected content type: {:?}", content_type);
                Err(StreamableHttpError::UnexpectedContentType(
                    content_type.map(|ct| String::from_utf8_lossy(ct.as_bytes()).to_string()),
                ))
            }
        }
    }

    async fn delete_session(
        &self,
        uri: Arc<str>,
        session_id: Arc<str>,
        auth_header: Option<String>,
        custom_headers: HashMap<HeaderName, HeaderValue>,
    ) -> Result<(), rmcp::transport::streamable_http_client::StreamableHttpError<Self::Error>> {
        let mut builder = Request::builder()
            .uri(uri.to_string())
            .method("DELETE")
            .header(HEADER_SESSION_ID, session_id.as_ref());
        builder = builder.header(http::header::HOST, "localhost");
        if let Some(auth_token) = auth_header {
            builder = builder.header(http::header::AUTHORIZATION, format!("Bearer {auth_token}"));
        }
        for (name, value) in &custom_headers {
            builder = builder.header(name, value);
        }

        let router = self.router.clone();

        let response = router
            .oneshot(builder.body(Body::empty()).unwrap())
            .await
            .unwrap();

        let status = response.status();
        // if method no allowed
        if status == StatusCode::METHOD_NOT_ALLOWED {
            tracing::debug!("this server doesn't support deleting session");
            return Ok(());
        }
        if status.is_client_error() || status.is_server_error() {
            return Err(StreamableHttpError::UnexpectedServerResponse(
                format!("error status code = {}", status).into(),
            ));
        }
        Ok(())
    }

    async fn get_stream(
        &self,
        uri: Arc<str>,
        session_id: Arc<str>,
        last_event_id: Option<String>,
        auth_header: Option<String>,
        custom_headers: HashMap<HeaderName, HeaderValue>,
    ) -> Result<
        BoxStream<'static, Result<Sse, SseError>>,
        rmcp::transport::streamable_http_client::StreamableHttpError<Self::Error>,
    > {
        let mut builder = Request::builder()
            .uri(uri.to_string())
            .method("POST")
            .header(http::header::CONTENT_TYPE, mime::APPLICATION_JSON.as_ref())
            .header(
                http::header::ACCEPT,
                [
                    mime::APPLICATION_JSON.as_ref(),
                    mime::TEXT_EVENT_STREAM.as_ref(),
                ]
                .join(", "),
            )
            .header(HEADER_SESSION_ID, session_id.as_ref());
        builder = builder.header(http::header::HOST, "localhost");
        if let Some(auth_token) = auth_header {
            builder = builder.header(http::header::AUTHORIZATION, format!("Bearer {auth_token}"));
        }
        if let Some(last_event_id) = last_event_id {
            builder = builder.header(HEADER_LAST_EVENT_ID, last_event_id);
        }
        for (name, value) in &custom_headers {
            builder = builder.header(name, value);
        }

        let router = self.router.clone();

        let response = router
            .oneshot(builder.body(Body::empty()).unwrap())
            .await
            .unwrap();

        let status = response.status();
        if status == StatusCode::METHOD_NOT_ALLOWED {
            return Err(StreamableHttpError::ServerDoesNotSupportSse);
        }
        if status.is_client_error() || status.is_server_error() {
            return Err(StreamableHttpError::UnexpectedServerResponse(
                format!("error status code = {}", status).into(),
            ));
        }
        match response.headers().get(http::header::CONTENT_TYPE) {
            Some(ct) => {
                if !ct.as_bytes().starts_with(EVENT_STREAM_MIME_TYPE.as_bytes())
                    && !ct.as_bytes().starts_with(JSON_MIME_TYPE.as_bytes())
                {
                    return Err(StreamableHttpError::UnexpectedContentType(Some(
                        String::from_utf8_lossy(ct.as_bytes()).to_string(),
                    )));
                }
            }
            None => {
                return Err(StreamableHttpError::UnexpectedContentType(None));
            }
        }

        let (_parts, body) = response.into_parts();
        let event_stream = SseStream::new(body).boxed();
        Ok(event_stream)
    }
}

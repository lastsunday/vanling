use axum::extract::Path;
use axum::http::{Method, StatusCode, header};
use axum::response::IntoResponse;
use rust_embed::Embed;

#[derive(Embed)]
#[folder = "dist"]
#[include = "index.html"]
#[allow_missing = true]
struct IndexHtml;

pub async fn index_handler(method: Method) -> impl IntoResponse {
    if method == Method::GET {
        let file = IndexHtml::get("index.html").expect("index.html not found");
        ([(header::CONTENT_TYPE, "text/html")], file.data).into_response()
    } else {
        (StatusCode::NOT_FOUND, "Not Found").into_response()
    }
}

#[derive(Embed)]
#[folder = "dist/assets"]
#[allow_missing = true]
struct Assets;

struct AssetsFile<T>(T);

impl<T: AsRef<str>> IntoResponse for AssetsFile<T> {
    fn into_response(self) -> axum::response::Response {
        let path = self.0.as_ref();
        match Assets::get(path) {
            Some(file) => {
                let mime = file.metadata.mimetype();
                let body = file.data;
                ([(header::CONTENT_TYPE, mime)], body).into_response()
            }
            None => (StatusCode::NOT_FOUND, "Not found").into_response(),
        }
    }
}

pub async fn assets_handler(Path(path): Path<String>) -> impl IntoResponse {
    AssetsFile(path).into_response()
}

#[derive(Embed)]
#[folder = "dist/locales"]
#[allow_missing = true]
struct Locales;

struct LocalesFile<T>(T);

impl<T: AsRef<str>> IntoResponse for LocalesFile<T> {
    fn into_response(self) -> axum::response::Response {
        let path = self.0.as_ref();
        match Locales::get(path) {
            Some(file) => {
                let mime = file.metadata.mimetype();
                let body = file.data;
                ([(header::CONTENT_TYPE, mime)], body).into_response()
            }
            None => (StatusCode::NOT_FOUND, "Not found").into_response(),
        }
    }
}

pub async fn locales_handler(Path(path): Path<String>) -> impl IntoResponse {
    LocalesFile(path).into_response()
}

#[derive(Embed)]
#[folder = "device/assets"]
#[allow_missing = true]
struct DeviceAssets;

struct DeviceAssetsFile<T>(T);

impl<T: AsRef<str>> IntoResponse for DeviceAssetsFile<T> {
    fn into_response(self) -> axum::response::Response {
        let path = self.0.as_ref();
        match DeviceAssets::get(path) {
            Some(file) => {
                let mime = file.metadata.mimetype();
                let body = file.data;
                ([(header::CONTENT_TYPE, mime)], body).into_response()
            }
            None => (StatusCode::NOT_FOUND, "Not found").into_response(),
        }
    }
}

pub async fn device_assets_handler(Path(path): Path<String>) -> impl IntoResponse {
    DeviceAssetsFile(path).into_response()
}

#[derive(Embed)]
#[folder = "dist/test"]
#[allow_missing = true]
struct Test;

struct TestFile<T>(T);

impl<T: AsRef<str>> IntoResponse for TestFile<T> {
    fn into_response(self) -> axum::response::Response {
        let path = self.0.as_ref();
        match Test::get(path) {
            Some(file) => {
                let mime = file.metadata.mimetype();
                let body = file.data;
                ([(header::CONTENT_TYPE, mime)], body).into_response()
            }
            None => (StatusCode::NOT_FOUND, "Not found").into_response(),
        }
    }
}

pub async fn test_handler(Path(path): Path<String>) -> impl IntoResponse {
    TestFile(path).into_response()
}

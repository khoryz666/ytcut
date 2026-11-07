//! HTTP routes. Axum does the parsing, routing, JSON and file streaming.

use axum::body::Body;
use axum::extract::{Path, Query, Request, State};
use axum::http::{header, HeaderValue, StatusCode, Uri};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use rust_embed::Embed;
use serde::Deserialize;
use tower::ServiceExt;
use tower_http::services::ServeFile;

use crate::app::App;
use crate::downloads::{self, Job, StartForm, VideoInfo};
use crate::library::{self, VideoFile};

/// The Svelte build, embedded at compile time.
#[derive(Embed)]
#[folder = "src/web/dist"]
struct Web;

pub fn router(app: App) -> Router {
    Router::new()
        .route("/", get(static_asset))
        .route("/alive", post(alive))
        .route("/bye", post(bye))
        .route("/jobs", get(jobs))
        .route("/files", get(files))
        .route("/info", get(info))
        .route("/download", post(download))
        .route("/rename", post(rename))
        .route("/delete", post(delete))
        .route("/media/{name}", get(media))
        .fallback(get(static_asset))
        .layer(middleware::from_fn_with_state(app.clone(), trusted))
        .with_state(app)
}

/// A page of the Svelte build: index.html, hashed assets, or the logo.
async fn static_asset(uri: Uri) -> Response {
    let path = match uri.path().trim_start_matches('/') {
        "" => "index.html",
        path => path,
    };
    match Web::get(path) {
        Some(file) => {
            let mime = mime_guess::from_path(path).first_or_octet_stream();
            let mut response = file.data.into_owned().into_response();
            let value = HeaderValue::from_str(mime.as_ref()).expect("mime types are headers");
            response.headers_mut().insert(header::CONTENT_TYPE, value);
            response
        }
        None => StatusCode::NOT_FOUND.into_response(),
    }
}

/// A page is still open. It posts here once a second while it is alive.
async fn alive(State(app): State<App>) -> StatusCode {
    app.touch();
    StatusCode::NO_CONTENT
}

/// A page is closing. Another open page cancels this with its next heartbeat.
async fn bye(State(app): State<App>) -> StatusCode {
    app.closing();
    StatusCode::NO_CONTENT
}

async fn jobs(State(app): State<App>) -> Json<Vec<Job>> {
    Json(app.jobs.lock().unwrap().iter().rev().cloned().collect())
}

async fn files(State(app): State<App>) -> Result<Json<Vec<VideoFile>>, ApiError> {
    library::list(&app.out)
        .map(Json)
        .map_err(|e| ApiError(e.to_string()))
}

#[derive(Deserialize)]
struct InfoQuery {
    #[serde(default)]
    url: String,
}

async fn info(
    State(app): State<App>,
    Query(query): Query<InfoQuery>,
) -> Result<Json<VideoInfo>, ApiError> {
    downloads::info(&app, &query.url)
        .await
        .map(Json)
        .map_err(ApiError)
}

async fn download(
    State(app): State<App>,
    axum::Form(form): axum::Form<StartForm>,
) -> Result<StatusCode, ApiError> {
    downloads::start(&app, form)
        .await
        .map(|()| StatusCode::ACCEPTED)
        .map_err(ApiError)
}

#[derive(Deserialize)]
struct NameForm {
    #[serde(default)]
    name: String,
    #[serde(default)]
    to: String,
}

async fn rename(
    State(app): State<App>,
    axum::Form(form): axum::Form<NameForm>,
) -> Result<Json<String>, ApiError> {
    library::rename(&app.out, &form.name, &form.to)
        .map(Json)
        .map_err(ApiError)
}

async fn delete(
    State(app): State<App>,
    axum::Form(form): axum::Form<NameForm>,
) -> Result<StatusCode, ApiError> {
    library::delete(&app.out, &form.name)
        .map(|()| StatusCode::NO_CONTENT)
        .map_err(ApiError)
}

/// Stream a library video. ServeFile handles Content-Type and Range requests.
async fn media(State(app): State<App>, Path(name): Path<String>, req: Request) -> Response {
    let path = match library::find(&app.out, &name) {
        Ok(path) => path,
        Err(e) => return (StatusCode::NOT_FOUND, e).into_response(),
    };
    match ServeFile::new(path).oneshot(req).await {
        Ok(response) => response.map(Body::new),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

/// Only our own page may call us.
async fn trusted(State(app): State<App>, req: Request, next: Next) -> Response {
    if app.trusted(req.headers()) {
        next.run(req).await
    } else {
        (StatusCode::FORBIDDEN, "forbidden").into_response()
    }
}

struct ApiError(String);

impl From<String> for ApiError {
    fn from(message: String) -> Self {
        Self(message)
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (StatusCode::BAD_REQUEST, self.0).into_response()
    }
}

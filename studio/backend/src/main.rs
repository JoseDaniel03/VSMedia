//! Development-only VSMedia Studio HTTP bridge. Reuses the actual SQLite CAS,
//! native document parser, project revisions and non-destructive Studio editor.
//! Only listens on loopback. No plugin broadcasting or radio transmitters.
use std::{net::SocketAddr, path::PathBuf, sync::Arc};
use axum::{
    body::Bytes, extract::{DefaultBodyLimit, Path, State},
    http::{header, HeaderMap, HeaderValue, Method, StatusCode},
    response::{IntoResponse, Response}, routing::{get, post}, Json, Router,
};
use serde::{Deserialize,Serialize};
use serde_json::{json,Value};
use tower_http::cors::{AllowOrigin,CorsLayer};
use vsmedia_api::{AssetKey,AssetKind,AssetRecord,AssetRepository,MediaError};
use vsmedia_core::sqlite::SqliteAssetStore;
use vsmedia_formats::{NativeDocument,NativePayload};
use vsmedia_formats::audio_studio::{AudioStudio,StudioCommand};

const STUDIO_NAMESPACE: &str = "studio";
const MAX_UPLOAD: usize = 16 * 1024 * 1024;
const MAX_DOCUMENT: usize = 2 * 1024 * 1024;
type Store = Arc<SqliteAssetStore>;

#[derive(Debug)]
struct ApiError(StatusCode,String);
impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.0,Json(json!({"error":self.1}))).into_response()
    }
}
fn bad<E:std::fmt::Display>(why:E)->ApiError {
    ApiError(StatusCode::BAD_REQUEST,why.to_string())
}
fn not_found(error:MediaError)->ApiError {
    match error {
        MediaError::NotFound(_) => ApiError(StatusCode::NOT_FOUND,error.to_string()),
        MediaError::Conflict(_) => ApiError(StatusCode::CONFLICT,error.to_string()),
        _ => bad(error),
    }
}
fn authorize_origin(headers:&HeaderMap)->Result<(),ApiError> {
    if let Some(origin)=headers.get(header::ORIGIN) {
        let allowed=origin.as_bytes()==b"http://127.0.0.1:3060"
            || origin.as_bytes()==b"http://localhost:3060";
        if !allowed {return Err(ApiError(StatusCode::FORBIDDEN,
            "Origin not allowed for local Studio".into()));}
    }
    Ok(())
}
fn key(name:String)->Result<AssetKey,ApiError> {
    AssetKey::new(STUDIO_NAMESPACE,name).map_err(bad)
}
async fn health() -> Json<Value> {
    Json(json!({"status":"ready","api_version":"v1",
        "storage_connected":true,"plugin_connected":false}))
}
async fn capabilities() -> Json<Value> {
    Json(json!({"api_version":"v1","audio_process":false,
        "radio_broadcast":false,"live_monitoring":false,
        "asset_storage":true,"audio_projects":true,
        "accepted_formats":["wav","flac","mp3","ogg","webm","opus"]}))
}
async fn put_asset(
    State(store):State<Store>,Path(name):Path<String>,
    headers:HeaderMap,body:Bytes
)->Result<Json<AssetRecord>,ApiError> {
    authorize_origin(&headers)?;
    if body.is_empty() || body.len()>MAX_UPLOAD {
        return Err(ApiError(StatusCode::PAYLOAD_TOO_LARGE,
            "Asset upload must contain 1..16 MiB".into()));
    }
    let record=store.publish_for(
        STUDIO_NAMESPACE,key(format!("audio/{name}"))?,AssetKind::Sound,&body
    ).map_err(not_found)?;
    Ok(Json(record))
}
async fn get_asset(
    State(store):State<Store>,Path(name):Path<String>
)->Result<Response,ApiError> {
    let bytes=store.read(&key(format!("audio/{name}"))?).map_err(not_found)?;
    Ok(([(header::CONTENT_TYPE,"application/octet-stream")],bytes).into_response())
}
async fn get_audio(
    State(store):State<Store>,Path(name):Path<String>
)->Result<Json<NativeDocument>,ApiError> {
    let key=key(format!("projects/{name}"))?;
    let doc=vsmedia_formats::load(store.as_ref(),&key,"vsaudio").map_err(not_found)?;
    Ok(Json(doc))
}
async fn put_audio(
    State(store):State<Store>,Path(name):Path<String>,
    headers:HeaderMap,Json(doc):Json<NativeDocument>
)->Result<Json<AssetRecord>,ApiError> {
    authorize_origin(&headers)?;
    if !matches!(&doc.payload,NativePayload::Audio(_)) {
        return Err(bad("Expected .vsaudio document"));
    }
    let key=key(format!("projects/{name}"))?;
    let bytes=vsmedia_formats::prepare_publish(&key,&doc).map_err(bad)?;
    let record=store.publish_for(
        STUDIO_NAMESPACE,key,AssetKind::Other,&bytes
    ).map_err(not_found)?;
    Ok(Json(record))
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CommandRequest {
    expected_sha256:String,
    command:StudioCommand,
}
#[derive(Serialize)]
struct CommandResult {
    record:AssetRecord,
    document:NativeDocument,
}
async fn command_audio(
    State(store):State<Store>,
    Path(name):Path<String>, headers:HeaderMap,
    Json(request):Json<CommandRequest>
)->Result<Json<CommandResult>,ApiError> {
    authorize_origin(&headers)?;
    let key=key(format!("projects/{name}"))?;
    let current=store.metadata(&key).map_err(not_found)?;
    if current.sha256!=request.expected_sha256 {
        return Err(ApiError(StatusCode::CONFLICT,
            "Outdated project revision. Reload before editing".into()));
    }
    let doc=vsmedia_formats::load(store.as_ref(),&key,"vsaudio").map_err(not_found)?;
    let NativePayload::Audio(audio)=doc.payload else {
        return Err(bad("Expected .vsaudio document"));
    };
    let mut studio=AudioStudio::open(audio).map_err(bad)?;
    studio.apply(request.command).map_err(bad)?;
    let updated=NativeDocument::new(NativePayload::Audio(studio.into_document()));
    let bytes=vsmedia_formats::prepare_publish(&key,&updated).map_err(bad)?;
    // Version check above is optimistic, not a serializable CAS transaction.
    // The service is single-editor development only until atomic compare-and-publish.
    let record=store.publish_for(
        STUDIO_NAMESPACE,key,AssetKind::Other,&bytes
    ).map_err(not_found)?;
    Ok(Json(CommandResult{record,document:updated}))
}
async fn unsupported() -> impl IntoResponse {
    (StatusCode::NOT_IMPLEMENTED,Json(json!({
        "error":"not_connected",
        "message":"La emisión en Minecraft y el streaming todavía no están habilitados."
    })))
}

fn router(store:Store)->Router {
    let cors=CorsLayer::new()
        .allow_origin(AllowOrigin::list([
            HeaderValue::from_static("http://127.0.0.1:3060"),
            HeaderValue::from_static("http://localhost:3060"),
        ]))
        .allow_methods([Method::GET,Method::POST])
        .allow_headers([header::CONTENT_TYPE]);
    Router::new()
        .route("/api/v1/health",get(health))
        .route("/api/v1/capabilities",get(capabilities))
        .route("/api/v1/assets/{name}",post(put_asset).get(get_asset))
        .route("/api/v1/audio/projects/{name}",post(put_audio).get(get_audio))
        .route("/api/v1/audio/projects/{name}/commands",post(command_audio))
        .route("/api/v1/media/jobs",post(unsupported))
        .route("/api/v1/radio/sessions",post(unsupported))
        .layer(DefaultBodyLimit::max(MAX_UPLOAD))
        .layer(cors)
        .with_state(store)
}
#[tokio::main]
async fn main()->Result<(),Box<dyn std::error::Error>> {
    let path=std::env::var_os("VSMEDIA_DATA_DIR")
        .map(PathBuf::from).unwrap_or_else(||PathBuf::from("./.vsmedia-data"));
    let db=Arc::new(SqliteAssetStore::open(path)?);
    db.grant(STUDIO_NAMESPACE,STUDIO_NAMESPACE,true,true)?;
    let addr:SocketAddr="127.0.0.1:3061".parse()?;
    let listener=tokio::net::TcpListener::bind(addr).await?;
    println!("VSMedia Studio bridge: http://{addr}/api/v1/health");
    axum::serve(listener,router(db)).await?;
    Ok(())
}

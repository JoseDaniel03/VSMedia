//! Development-only VSMedia Studio HTTP bridge. Reuses the actual SQLite CAS,
//! native document parser, project revisions and non-destructive Studio editor.
//! Only listens on loopback. No plugin broadcasting or radio transmitters.
use std::{collections::HashMap, net::SocketAddr, path::PathBuf, sync::Arc};
mod live;
mod packs;
mod pack_manager;
use vsmedia_audio::{codec::{decode_wav,encode_wav_pcm16,StereoAudio},timeline::render_project_advanced};
use sha2::{Digest,Sha256};
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
    Json(json!({"api_version":"v1","audio_process":true,
        "audio_render_formats":["wav"],
        "audio_render_output":"wav_pcm16_48000_stereo",
        "radio_broadcast":false,"live_monitoring":false,
        "webrtc_signaling":true,"webrtc_voice_rooms":false,
        "webrtc_radio_peer_mesh":true,"max_radio_peers":16,
        "vsstreams_provider":"plasmo_voice","vsstreams_connected":false,
        "vsstreams_moderation":false,
        "asset_storage":true,"audio_projects":true,
        "vspack_editor":true,"vspack_merge":"structured_priority",
        "vspack_zip_export":true,"vspack_session_only":true,
        "vspack_resource_catalog":"vspack-resources-v1",
        "vspack_ui_catalog":true,"vspack_path_overrides":true,
        "vspack_minecraft_delivery":false,
        "vspack_manager_recipes":"vspack-recipe-v1",
        "vspack_category_splitting":true,
        "vspack_profiles_portable_json":true,
        "accepted_formats":["wav","flac","mp3","ogg","webm","opus"]}))
}
/// Real read-only integration state. This backend deliberately does not
/// impersonate a Plasmo Voice server or expose unimplemented moderation.
async fn streams_integration() -> Json<Value> {
    Json(json!({
        "provider":"plasmo_voice",
        "transport":"native_minecraft_voice",
        "server_connected":false,
        "client_mod_probed":false,
        "administrative_actions":false,
        "minecraft_broadcast":false,
        "local_radio_webrtc":true,
        "required":["plasmo_voice_server_plugin_26_3",
                    "plasmo_voice_fabric_client_26_3",
                    "vse_engine_adapter"],
        "message":"VSEngine todavía no expone telemetría de Plasmo Voice a este bridge. No se iniciará un chat WebRTC alternativo."
    }))
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
    let key=key(format!("audio/{name}"))?;
    let hash=format!("{:x}",Sha256::digest(&body));
    // Repeated draft previews reuse the same source instead of generating
    // unnecessary catalog revisions and invalidating other project pins.
    if let Ok(existing)=store.metadata(&key) {
        if existing.kind==AssetKind::Sound && existing.sha256==hash {
            return Ok(Json(existing));
        }
    }
    let record=store.publish_for(
        STUDIO_NAMESPACE,key,AssetKind::Sound,&body
    ).map_err(not_found)?;
    Ok(Json(record))
}
async fn get_asset(
    State(store):State<Store>,Path(name):Path<String>
)->Result<Response,ApiError> {
    let bytes=store.read(&key(format!("audio/{name}"))?).map_err(not_found)?;
    Ok(([(header::CONTENT_TYPE,"application/octet-stream")],bytes).into_response())
}
/// Only Studio's audio-project namespace is enumerable from this endpoint.
async fn list_audio_projects(
    State(store):State<Store>
)->Result<Json<Vec<AssetRecord>>,ApiError> {
    let entries=store.list_assets(STUDIO_NAMESPACE,"projects/",256)
        .map_err(not_found)?;
    Ok(Json(entries.into_iter()
        .filter(|record|record.kind==AssetKind::Other)
        .collect()))
}
async fn get_asset_metadata(
    State(store):State<Store>,Path(name):Path<String>
)->Result<Json<AssetRecord>,ApiError> {
    let record=store.metadata(&key(format!("audio/{name}"))?).map_err(not_found)?;
    if record.kind!=AssetKind::Sound {return Err(bad("Not an audio asset"));}
    Ok(Json(record))
}
async fn get_audio_metadata(
    State(store):State<Store>,Path(name):Path<String>
)->Result<Json<AssetRecord>,ApiError> {
    let record=store.metadata(&key(format!("projects/{name}"))?)
        .map_err(not_found)?;
    if record.kind!=AssetKind::Other {
        return Err(bad("Not a native project"));
    }
    Ok(Json(record))
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
    // Load the immutable document and use its current SHA-256 as an optimistic
    // precondition checked atomically by SQLite at commit time.
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
    // Atomic precondition in the SQLite IMMEDIATE write transaction:
    // concurrent editors cannot overwrite each other after passing a stale check.
    let record=store.compare_and_publish_for(
        STUDIO_NAMESPACE,key,AssetKind::Other,&bytes,&request.expected_sha256
    ).map_err(not_found)?;
    Ok(Json(CommandResult{record,document:updated}))
}
/// Offline render of a saved native .vsaudio, using the *actual* Rust DSP
/// implementation (fades, EQ, pan and gain envelopes). Processing is isolated
/// from Axum's async executor and capped by the Audio Engine's own quotas.
/// Only uncompressed WAV is supported until safe compressed decoders exist.
/// Shared render path: the document may come from SQLite or an unsaved
/// editing session. Neither path stores a rendered preview in the catalog.
fn render_document(store:&Store,document:NativeDocument)->Result<Vec<u8>,ApiError> {
    document.validate().map_err(bad)?;
    let NativePayload::Audio(audio)=document.payload else {
        return Err(bad("Expected .vsaudio document"));
    };
    if audio.sources.len()>256 {
        return Err(bad("Too many audio sources"));
    }
    let mut sources=HashMap::new();
    for source in &audio.sources {
        // A Studio project must not turn this unauthenticated local
        // preview route into a cross-plugin read of another namespace.
        if source.key.namespace!=STUDIO_NAMESPACE ||
            !source.key.name.starts_with("audio/") {
            return Err(ApiError(StatusCode::FORBIDDEN,
                "Studio solo puede renderizar archivos de su propio namespace".into()));
        }
        if source.codec!=vsmedia_formats::MediaCodec::Wav {
            return Err(ApiError(StatusCode::UNPROCESSABLE_ENTITY,
                "Render de VSCore admite WAV por ahora. MP3, OGG, Opus y FLAC requieren un decodificador nativo.".into()));
        }
        let current=store.metadata(&source.key).map_err(not_found)?;
        if current.kind!=AssetKind::Sound {
            return Err(ApiError(StatusCode::UNPROCESSABLE_ENTITY,
                "El proyecto referencia un archivo que no es audio".into()));
        }
        if current.sha256!=source.sha256 {
            return Err(ApiError(StatusCode::CONFLICT,
                "El audio fuente cambió desde la revisión del proyecto".into()));
        }
        let bytes=store.read(&source.key).map_err(not_found)?;
        // Recheck the fetched bytes against the manifest pin, too:
        // metadata can advance between two independent read operations.
        if format!("{:x}",Sha256::digest(&bytes))!=source.sha256 {
            return Err(ApiError(StatusCode::CONFLICT,
                "La fuente ya no coincide con el SHA-256 del proyecto".into()));
        }
        sources.insert(source.key.clone(),decode_wav(&bytes).map_err(bad)?);
    }
    let output=render_project_advanced(
        &audio.timeline,&sources,48_000,&audio.clip_edits,&audio.track_edits
    ).map_err(bad)?;
    encode_wav_pcm16(&output).map_err(bad)
}

fn wav_response(wav:Vec<u8>)->Response {
    ([
        (header::CONTENT_TYPE,"audio/wav"),
        (header::CACHE_CONTROL,"no-store"),
        (header::CONTENT_DISPOSITION,"inline")
    ],wav).into_response()
}

/// Render previously saved .vsaudio without changing its stored revision.
async fn render_audio_project(
    State(store):State<Store>,Path(name):Path<String>
)->Result<Response,ApiError> {
    let key=key(format!("projects/{name}"))?;
    let wav=tokio::task::spawn_blocking(move||{
        let document=vsmedia_formats::load(store.as_ref(),&key,"vsaudio")
            .map_err(not_found)?;
        render_document(&store,document)
    }).await.map_err(|e|ApiError(StatusCode::INTERNAL_SERVER_ERROR,
        format!("Audio render task terminated: {e}")))??;
    Ok(wav_response(wav))
}

/// Render an unsaved project. Files must already exist in Studio CAS with
/// their SHA-256 pinned by this request. No project revision is created.
async fn render_unsaved_audio(
    State(store):State<Store>,headers:HeaderMap,Json(document):Json<NativeDocument>
)->Result<Response,ApiError> {
    authorize_origin(&headers)?;
    if !matches!(&document.payload,NativePayload::Audio(_)) {
        return Err(bad("Expected .vsaudio document"));
    }
    if serde_json::to_vec(&document).map_err(bad)?.len()>MAX_DOCUMENT {
        return Err(ApiError(StatusCode::PAYLOAD_TOO_LARGE,
            "Preview document exceeds 2 MiB".into()));
    }
    let wav=tokio::task::spawn_blocking(move||render_document(&store,document))
        .await.map_err(|e|ApiError(StatusCode::INTERNAL_SERVER_ERROR,
            format!("Audio render task terminated: {e}")))??;
    Ok(wav_response(wav))
}
async fn unsupported() -> impl IntoResponse {
    (StatusCode::NOT_IMPLEMENTED,Json(json!({
        "error":"not_connected",
        "message":"La emisión en Minecraft y el streaming todavía no están habilitados."
    })))
}

fn router(store:Store)->Router {
    let hub=live::LiveHub::default();
    let pack_hub=packs::PackHub::default();
    let cors=CorsLayer::new()
        .allow_origin(AllowOrigin::list([
            HeaderValue::from_static("http://127.0.0.1:3060"),
            HeaderValue::from_static("http://localhost:3060"),
        ]))
        .allow_methods([Method::GET,Method::POST,Method::DELETE])
        .allow_headers([header::CONTENT_TYPE,
            header::HeaderName::from_static("x-vspack-label")]);
    Router::new()
        .route("/api/v1/health",get(health))
        .route("/api/v1/capabilities",get(capabilities))
        .route("/api/v1/live/ws",get(live::socket))
        .route("/api/v1/live/status",get(live::status))
        .route("/api/v1/streams/integration",get(streams_integration))
        .route("/api/v1/packs",get(packs::list))
        .route("/api/v1/packs/manager/capabilities",get(pack_manager::capabilities))
        .route("/api/v1/packs/manager/recipe",get(pack_manager::draft))
        .route("/api/v1/packs/manager/plan",post(pack_manager::plan)
            .layer(DefaultBodyLimit::max(64*1024)))
        .route("/api/v1/packs/manager/export",post(pack_manager::export)
            .layer(DefaultBodyLimit::max(64*1024)))
        .route("/api/v1/packs/manager/split/plan",post(pack_manager::split_plan)
            .layer(DefaultBodyLimit::max(64*1024)))
        .route("/api/v1/packs/manager/split/export",post(pack_manager::split_export)
            .layer(DefaultBodyLimit::max(64*1024)))
        .route("/api/v1/packs/preview",get(packs::preview))
        .route("/api/v1/packs/resources",get(packs::resources))
        .route("/api/v1/packs/interfaces",get(packs::interfaces))
        .route("/api/v1/packs/merge/plan",get(packs::resources))
        .route("/api/v1/packs/merge/overrides",get(packs::resource_choices)
            .post(packs::choose_resource).delete(packs::clear_resource_choice))
        .route("/api/v1/packs/resources/file",get(packs::resource_file))
        .route("/api/v1/packs/texture",get(packs::texture))
        .route("/api/v1/packs/export",get(packs::export))
        .route("/api/v1/packs/layers/export",post(packs::export_layers))
        .route("/api/v1/packs/{id}/download",get(packs::download_source))
        .route("/api/v1/packs/{id}",get(packs::source_info)
            .post(packs::import).delete(packs::remove)
            .layer(DefaultBodyLimit::max(96*1024*1024)))
        .route("/api/v1/packs/{id}/priority",post(packs::priority))
        .route("/api/v1/assets/{name}/metadata",get(get_asset_metadata))
        .route("/api/v1/assets/{name}",post(put_asset).get(get_asset))
        .route("/api/v1/audio/projects",get(list_audio_projects))
        .route("/api/v1/audio/projects/{name}/metadata",get(get_audio_metadata))
        .route("/api/v1/audio/projects/{name}",post(put_audio).get(get_audio))
        .route("/api/v1/audio/render",post(render_unsaved_audio).layer(DefaultBodyLimit::max(MAX_DOCUMENT)))
        .route("/api/v1/audio/projects/{name}/render",get(render_audio_project))
        .route("/api/v1/audio/projects/{name}/commands",post(command_audio))
        .route("/api/v1/media/jobs",post(unsupported))
        .route("/api/v1/radio/sessions",post(unsupported))
        .layer(DefaultBodyLimit::max(MAX_UPLOAD))
        .layer(cors)
        .layer(axum::Extension(hub))
        .layer(axum::Extension(pack_hub))
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

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{body::{Body,to_bytes},http::Request};
    use std::sync::atomic::{AtomicUsize,Ordering};
    use tower::ServiceExt;
    static NEXT:AtomicUsize=AtomicUsize::new(0);
    fn store()->(Store,PathBuf) {
        let dir=std::env::temp_dir().join(format!("vsmedia-studio-http-{}-{}",
            std::process::id(),NEXT.fetch_add(1,Ordering::Relaxed)));
        let store=Arc::new(SqliteAssetStore::open(&dir).unwrap());
        store.grant("studio","studio",true,true).unwrap();
        (store,dir)
    }
    fn request(method:Method,uri:&str,body:Vec<u8>)->Request<Body> {
        Request::builder().method(method).uri(uri)
            .header(header::ORIGIN,"http://127.0.0.1:3060")
            .header(header::CONTENT_TYPE,"application/json")
            .body(Body::from(body)).unwrap()
    }
    #[tokio::test]
    async fn vspack_http_contract_is_accessible_for_future_interface_manager(){
        use std::collections::BTreeMap;
        use vsmedia_pack::{MergeOutput,archive::{export_zip,ArchiveLimits}};
        let (db,dir)=store();
        let app=router(db.clone());
        let filename="assets/virgo/textures/gui/button.png";
        let zipped=export_zip(&MergeOutput{
            files:BTreeMap::from([
                ("pack.mcmeta".to_owned(),
                    br#"{"pack":{"pack_format":1,"description":"Demo"}}"#.to_vec()),
                (filename.to_owned(),b"image1".to_vec()),
            ]),conflicts:vec![],
        },ArchiveLimits::default()).unwrap();
        let imported=app.clone().oneshot(request(Method::POST,
            "/api/v1/packs/panel",zipped)).await.unwrap();
        assert_eq!(imported.status(),StatusCode::OK);
        let index=app.clone().oneshot(request(Method::GET,
            "/api/v1/packs/interfaces?namespace=virgo",vec![])).await.unwrap();
        assert_eq!(index.status(),StatusCode::OK);
        let value:Value=serde_json::from_slice(&to_bytes(index.into_body(),65_536)
            .await.unwrap()).unwrap();
        assert_eq!(value["schema"],"vspack-resources-v1");
        assert_eq!(value["total"],1);
        assert_eq!(value["items"][0]["path"],filename);
        assert_eq!(value["items"][0]["interface_kind"],"gui_textures");

        let chosen=app.clone().oneshot(request(Method::POST,
            "/api/v1/packs/merge/overrides",
            serde_json::to_vec(&json!({
                "path":filename,"source_id":"panel"
            })).unwrap())).await.unwrap();
        assert_eq!(chosen.status(),StatusCode::OK);
        let preview=app.clone().oneshot(request(Method::GET,
            "/api/v1/packs/resources?scope=interfaces",vec![])).await.unwrap();
        let preview:Value=serde_json::from_slice(&to_bytes(preview.into_body(),65_536)
            .await.unwrap()).unwrap();
        assert_eq!(preview["items"][0]["chosen_source"],"panel");
        assert_eq!(preview["items"][0]["override_source"],"panel");

        let file=app.clone().oneshot(request(Method::GET,
            "/api/v1/packs/resources/file?path=assets%2Fvirgo%2Ftextures%2Fgui%2Fbutton.png",
            vec![])).await.unwrap();
        assert_eq!(file.status(),StatusCode::OK);
        assert_eq!(file.headers()[header::X_CONTENT_TYPE_OPTIONS],"nosniff");
        assert_eq!(to_bytes(file.into_body(),4096).await.unwrap().as_ref(),b"image1");

        let source=app.clone().oneshot(request(Method::GET,
            "/api/v1/packs/panel/download",vec![])).await.unwrap();
        assert_eq!(source.status(),StatusCode::OK);
        assert_eq!(source.headers()[header::CONTENT_TYPE],"application/zip");

        let mut denied=request(Method::POST,
            "/api/v1/packs/merge/overrides",
            serde_json::to_vec(&json!({"path":filename,"source_id":"panel"})).unwrap());
        denied.headers_mut().insert(header::ORIGIN,
            header::HeaderValue::from_static("https://malicious.test"));
        let denied=app.clone().oneshot(denied).await.unwrap();
        assert_eq!(denied.status(),StatusCode::FORBIDDEN);
        drop(app);drop(db);let _=std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn live_room_status_and_capabilities_are_aggregated_without_room_secrets(){
        let (db,dir)=store();
        let app=router(db.clone());
        let status=app.clone().oneshot(request(Method::GET,
            "/api/v1/live/status",vec![])).await.unwrap();
        assert_eq!(status.status(),StatusCode::OK);
        let value:Value=serde_json::from_slice(
            &to_bytes(status.into_body(),4096).await.unwrap()
        ).unwrap();
        assert_eq!(value["transport"],"webrtc_p2p");
        assert_eq!(value["rooms"],0);
        assert_eq!(value["peers"],0);
        assert_eq!(value["max_radio"],16);
        assert_eq!(value["voice_rooms"],false);
        assert_eq!(value["audio_recorded"],false);
        assert!(value.get("room").is_none());
        let caps=app.clone().oneshot(request(Method::GET,
            "/api/v1/capabilities",vec![])).await.unwrap();
        assert_eq!(caps.status(),StatusCode::OK);
        let capabilities:Value=serde_json::from_slice(
            &to_bytes(caps.into_body(),4096).await.unwrap()
        ).unwrap();
        assert_eq!(capabilities["webrtc_signaling"],true);
        assert_eq!(capabilities["webrtc_voice_rooms"],false);
        assert_eq!(capabilities["vsstreams_provider"],"plasmo_voice");
        assert_eq!(capabilities["vsstreams_connected"],false);
        assert_eq!(capabilities["webrtc_radio_peer_mesh"],true);
        assert_eq!(capabilities["radio_broadcast"],false);
        let integration=app.clone().oneshot(request(Method::GET,
            "/api/v1/streams/integration",vec![])).await.unwrap();
        assert_eq!(integration.status(),StatusCode::OK);
        let streams:Value=serde_json::from_slice(
            &to_bytes(integration.into_body(),4096).await.unwrap()
        ).unwrap();
        assert_eq!(streams["provider"],"plasmo_voice");
        assert_eq!(streams["server_connected"],false);
        assert_eq!(streams["administrative_actions"],false);
        let unsupported=app.clone().oneshot(request(Method::POST,
            "/api/v1/radio/sessions",vec![])).await.unwrap();
        assert_eq!(unsupported.status(),StatusCode::NOT_IMPLEMENTED);
        drop(app);drop(db);let _=std::fs::remove_dir_all(dir);
    }
    #[tokio::test]
    async fn catalog_retrieval_and_editor_commands_work_end_to_end() {
        let (db,dir)=store();
        let sound_key=AssetKey::new("studio","audio/fixture").unwrap();
        let sound=db.publish_for("studio",sound_key.clone(),
            AssetKind::Sound,b"example PCM bytes").unwrap();
        let payload=json!({"magic":"VSMEDIA","version":1,
            "payload":{"kind":"audio","data":{
                "timeline":{"name":"Fixture","tracks":[
                    {"name":"Voz","muted":false,"clips":[
                        {"asset":sound_key,"start_ms":0,"duration_ms":1000,"gain_db":0}
                    ]}
                ]},
                "sources":[{"key":{"namespace":"studio","name":"audio/fixture"},
                    "sha256":sound.sha256,"codec":"wav"}]
            }}
        });
        let app=router(db.clone());
        let created=app.clone().oneshot(request(Method::POST,
            "/api/v1/audio/projects/fixture",serde_json::to_vec(&payload).unwrap()
        )).await.unwrap();
        assert_eq!(created.status(),StatusCode::OK);
        let initial:Value=serde_json::from_slice(&to_bytes(created.into_body(),2*1024*1024)
            .await.unwrap()).unwrap();
        let hash=initial["sha256"].as_str().unwrap();
        let list=app.clone().oneshot(request(Method::GET,"/api/v1/audio/projects",vec![]))
            .await.unwrap();
        assert_eq!(list.status(),StatusCode::OK);
        let rows:Value=serde_json::from_slice(&to_bytes(list.into_body(),2*1024*1024)
            .await.unwrap()).unwrap();
        assert_eq!(rows.as_array().unwrap().len(),1);
        assert_eq!(rows[0]["key"]["name"],"projects/fixture");
        let command=json!({"expected_sha256":hash,
            "command":{"operation":"rename_track","index":0,"name":"Nueva voz"}});
        let updated=app.clone().oneshot(request(Method::POST,
            "/api/v1/audio/projects/fixture/commands",serde_json::to_vec(&command).unwrap()
        )).await.unwrap();
        assert_eq!(updated.status(),StatusCode::OK);
        let stale=app.clone().oneshot(request(Method::POST,
            "/api/v1/audio/projects/fixture/commands",serde_json::to_vec(&command).unwrap()
        )).await.unwrap();
        assert_eq!(stale.status(),StatusCode::CONFLICT);
        let metadata=app.clone().oneshot(request(Method::GET,
            "/api/v1/audio/projects/fixture/metadata",vec![]
        )).await.unwrap();
        assert_eq!(metadata.status(),StatusCode::OK);
        let restored=app.clone().oneshot(request(Method::GET,
            "/api/v1/audio/projects/fixture",vec![]
        )).await.unwrap();
        assert_eq!(restored.status(),StatusCode::OK);
        let native:Value=serde_json::from_slice(&to_bytes(restored.into_body(),2*1024*1024)
            .await.unwrap()).unwrap();
        assert_eq!(native["payload"]["data"]["timeline"]["tracks"][0]["name"],
            "Nueva voz");
        let audio=app.clone().oneshot(request(Method::GET,
            "/api/v1/assets/fixture",vec![]
        )).await.unwrap();
        assert_eq!(audio.status(),StatusCode::OK);
        drop(app);
        drop(db);
        let _=std::fs::remove_dir_all(dir);
    }
    #[tokio::test]
    async fn rust_engine_wav_preview_applies_pan_and_fades() {
        let (db,dir)=store();
        let key=AssetKey::new("studio","audio/wav-preview").unwrap();
        let source=StereoAudio {sample_rate:8_000,frames:vec![[0.6,0.6];800]};
        let raw=encode_wav_pcm16(&source).unwrap();
        let record=db.publish_for("studio",key.clone(),AssetKind::Sound,&raw).unwrap();
        let project=json!({"magic":"VSMEDIA","version":1,
            "payload":{"kind":"audio","data":{
                "timeline":{"name":"DSP demo","tracks":[{
                    "name":"Clip","muted":false,
                    "clips":[{"asset":key,"start_ms":0,"duration_ms":100,"gain_db":0}]
                }]},
                "sources":[{"key":{"namespace":"studio","name":"audio/wav-preview"},
                    "sha256":record.sha256,"codec":"wav"}],
                "clip_edits":[{"track_index":0,"clip_index":0,"source_in_ms":0,
                    "pan":1.0,"fade_in_ms":20,"fade_out_ms":20,
                    "reversed":false,"muted":false,
                    "gain_points":[{"at_ms":0,"gain_db":-9.0},
                        {"at_ms":100,"gain_db":0.0}],
                    "filters":[{"type":"low_pass","frequency_hz":1500.0,"q":0.707}]}]
            }}
        });
        let app=router(db.clone());
        let created=app.clone().oneshot(request(Method::POST,
            "/api/v1/audio/projects/wav-preview",
            serde_json::to_vec(&project).unwrap())).await.unwrap();
        assert_eq!(created.status(),StatusCode::OK);
        let response=app.clone().oneshot(request(Method::GET,
            "/api/v1/audio/projects/wav-preview/render",vec![])).await.unwrap();
        assert_eq!(response.status(),StatusCode::OK);
        assert_eq!(response.headers()[header::CONTENT_TYPE],"audio/wav");
        let bytes=to_bytes(response.into_body(),64*1024*1024).await.unwrap();
        let decoded=decode_wav(&bytes).unwrap();
        assert_eq!(decoded.sample_rate,48_000);
        assert!(decoded.frames.len()>4000);
        assert!(decoded.frames[0][1].abs()<0.01);
        assert!(decoded.frames[2000][1].abs()>0.15);
        // A linearly rising volume automation should be measurable well
        // before the fade-out begins, after the EQ has stabilized.
        assert!(decoded.frames[3500][1].abs()>decoded.frames[1200][1].abs());
        assert!(decoded.frames[2000][0].abs()<0.001);
        drop(app);drop(db);let _=std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn unsaved_preview_renders_pcm_without_creating_a_project_revision() {
        let (db,dir)=store();
        let source_key=AssetKey::new("studio","audio/preview-only").unwrap();
        let wav=encode_wav_pcm16(&StereoAudio {
            sample_rate:8000,frames:vec![[0.3,0.4];800]
        }).unwrap();
        let stored=db.publish_for("studio",source_key.clone(),AssetKind::Sound,&wav).unwrap();
        let doc=json!({"magic":"VSMEDIA","version":1,"payload":{"kind":"audio",
            "data":{"timeline":{"name":"Unsaved","tracks":[{"name":"Voice","muted":false,
                "clips":[{"asset":source_key,"start_ms":0,"duration_ms":100,"gain_db":0}]}]},
                "sources":[{"key":{"namespace":"studio","name":"audio/preview-only"},
                    "sha256":stored.sha256,"codec":"wav"}]
            }}});
        let app=router(db.clone());
        let result=app.clone().oneshot(request(Method::POST,
            "/api/v1/audio/render",serde_json::to_vec(&doc).unwrap()
        )).await.unwrap();
        assert_eq!(result.status(),StatusCode::OK);
        let frames=decode_wav(&to_bytes(result.into_body(),64*1024*1024)
            .await.unwrap()).unwrap();
        assert!(frames.frames.len()>4000);
        assert!(db.list_assets("studio","projects/",10).unwrap().is_empty());
        drop(app);drop(db);let _=std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn uploading_identical_audio_twice_reuses_existing_catalog_revision() {
        let (db,dir)=store();
        let app=router(db.clone());
        let first=app.clone().oneshot(request(Method::POST,
            "/api/v1/assets/idempotent",b"same audio".to_vec()
        )).await.unwrap();
        assert_eq!(first.status(),StatusCode::OK);
        let a:Value=serde_json::from_slice(&to_bytes(first.into_body(),4096)
            .await.unwrap()).unwrap();
        let second=app.clone().oneshot(request(Method::POST,
            "/api/v1/assets/idempotent",b"same audio".to_vec()
        )).await.unwrap();
        assert_eq!(second.status(),StatusCode::OK);
        let b:Value=serde_json::from_slice(&to_bytes(second.into_body(),4096)
            .await.unwrap()).unwrap();
        assert_eq!(a["sha256"],b["sha256"]);
        assert_eq!(a["revision"],b["revision"]);
        assert_eq!(a["revision"],1);
        drop(app);drop(db);let _=std::fs::remove_dir_all(dir);
    }
    #[tokio::test]
    async fn unsaved_render_respects_overlapping_regions_and_clip_pan() {
        let (db,dir)=store();
        let file=AssetKey::new("studio","audio/regions").unwrap();
        let wav=encode_wav_pcm16(&StereoAudio{
            sample_rate:48_000,frames:vec![[0.25,0.25];4_800],
        }).unwrap();
        let recorded=db.publish_for("studio",file.clone(),AssetKind::Sound,&wav).unwrap();
        let document=json!({"magic":"VSMEDIA","version":1,
            "payload":{"kind":"audio","data":{
                "timeline":{"name":"Two regions","tracks":[{
                    "name":"FX","muted":false,"clips":[
                        {"asset":file,"start_ms":0,"duration_ms":100,"gain_db":0},
                        {"asset":{"namespace":"studio","name":"audio/regions"},
                         "start_ms":50,"duration_ms":100,"gain_db":-6.0}
                    ]
                }]},
                "sources":[{"key":{"namespace":"studio","name":"audio/regions"},
                    "sha256":recorded.sha256,"codec":"wav"}],
                "clip_edits":[{"track_index":0,"clip_index":1,
                    "source_in_ms":0,"pan":1.0,
                    "fade_in_ms":0,"fade_out_ms":0,"reversed":false,
                    "muted":false,"gain_points":[],"filters":[]}]
            }}});
        let app=router(db.clone());
        let result=app.clone().oneshot(request(Method::POST,
            "/api/v1/audio/render",serde_json::to_vec(&document).unwrap()
        )).await.unwrap();
        assert_eq!(result.status(),StatusCode::OK);
        let bytes=to_bytes(result.into_body(),64*1024*1024).await.unwrap();
        let mixed=decode_wav(&bytes).unwrap();
        assert_eq!(mixed.frames.len(),7200);
        assert!((mixed.frames[1000][0]-0.25).abs()<0.002);
        assert!((mixed.frames[3000][0]-0.25).abs()<0.002);
        assert!(mixed.frames[3000][1]>0.35);
        assert!(mixed.frames[6000][0].abs()<0.002);
        assert!((mixed.frames[6000][1]-0.125).abs()<0.008);
        assert!(db.list_assets("studio","projects/",10).unwrap().is_empty());
        drop(app);drop(db);let _=std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn draft_equal_power_mix_uses_native_fades_without_saving_project() {
        let (db,dir)=store();
        let key=AssetKey::new("studio","audio/equal-power").unwrap();
        let wav=encode_wav_pcm16(&StereoAudio{
            sample_rate:8_000,frames:vec![[0.25,0.25];8_000],
        }).unwrap();
        let stored=db.publish_for("studio",key.clone(),AssetKind::Sound,&wav).unwrap();
        let document=json!({"magic":"VSMEDIA","version":1,
            "payload":{"kind":"audio","data":{
                "timeline":{"name":"Draft transition","tracks":[{
                    "name":"Dialogue","muted":false,"clips":[
                        {"asset":key,"start_ms":0,"duration_ms":1000,"gain_db":0},
                        {"asset":{"namespace":"studio","name":"audio/equal-power"},
                         "start_ms":500,"duration_ms":1000,"gain_db":0}
                    ]
                }]},
                "sources":[{"key":{"namespace":"studio","name":"audio/equal-power"},
                    "sha256":stored.sha256,"codec":"wav"}],
                "clip_edits":[
                    {"track_index":0,"clip_index":0,"fade_out_ms":500,
                     "fade_curve":"equal_power"},
                    {"track_index":0,"clip_index":1,"fade_in_ms":500,
                     "fade_curve":"equal_power"}
                ]
            }}});
        let app=router(db.clone());
        let result=app.clone().oneshot(request(Method::POST,
            "/api/v1/audio/render",serde_json::to_vec(&document).unwrap()
        )).await.unwrap();
        assert_eq!(result.status(),StatusCode::OK);
        let bytes=to_bytes(result.into_body(),64*1024*1024).await.unwrap();
        let rendered=decode_wav(&bytes).unwrap();
        // Studio draft render normalizes to 48 kHz irrespective of source rate.
        assert_eq!(rendered.sample_rate,48_000);
        assert_eq!(rendered.frames.len(),72_000);
        let expected=0.5*std::f32::consts::FRAC_1_SQRT_2;
        assert!((rendered.frames[36_000][0]-expected).abs()<0.002);
        assert!(db.list_assets("studio","projects/",10).unwrap().is_empty());
        drop(app);drop(db);let _=std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn draft_gain_automation_renders_without_saving_a_revision() {
        let (db,dir)=store();
        let key=AssetKey::new("studio","audio/keyframes").unwrap();
        let wav=encode_wav_pcm16(&StereoAudio {
            sample_rate:48_000,frames:vec![[0.25,0.25];48_000],
        }).unwrap();
        let source=db.publish_for("studio",key.clone(),AssetKind::Sound,&wav).unwrap();
        let document=json!({"magic":"VSMEDIA","version":1,
            "payload":{"kind":"audio","data":{
                "timeline":{"name":"Automation draft","tracks":[{
                    "name":"Voice","muted":false,
                    "clips":[{"asset":key,"start_ms":0,"duration_ms":1000,"gain_db":0}]
                }]},
                "sources":[{"key":{"namespace":"studio","name":"audio/keyframes"},
                    "sha256":source.sha256,"codec":"wav"}],
                "clip_edits":[{"track_index":0,"clip_index":0,"source_in_ms":0,
                    "gain_points":[
                        {"at_ms":0,"gain_db":-6.0206},
                        {"at_ms":500,"gain_db":0.0},
                        {"at_ms":1000,"gain_db":-6.0206}
                    ]}]
            }}});
        let app=router(db.clone());
        let result=app.clone().oneshot(request(Method::POST,
            "/api/v1/audio/render",serde_json::to_vec(&document).unwrap()
        )).await.unwrap();
        assert_eq!(result.status(),StatusCode::OK);
        let raw=to_bytes(result.into_body(),64*1024*1024).await.unwrap();
        let rendered=decode_wav(&raw).unwrap();
        assert_eq!(rendered.sample_rate,48_000);
        assert_eq!(rendered.frames.len(),48_000);
        assert!((rendered.frames[0][0]-0.125).abs()<0.002);
        assert!((rendered.frames[12_000][0]-0.1875).abs()<0.003);
        assert!((rendered.frames[24_000][0]-0.25).abs()<0.002);
        assert!((rendered.frames[36_000][0]-0.1875).abs()<0.003);
        assert!(db.list_assets("studio","projects/",10).unwrap().is_empty());
        drop(app);drop(db);let _=std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn compressed_audio_preview_is_explicitly_unsupported() {
        let (db,dir)=store();
        let key=AssetKey::new("studio","audio/mp3-preview").unwrap();
        let source=db.publish_for("studio",key.clone(),AssetKind::Sound,
            b"uncompressed decoder is not available").unwrap();
        let doc=json!({"magic":"VSMEDIA","version":1,
            "payload":{"kind":"audio","data":{
                "timeline":{"name":"MP3 demo","tracks":[{
                    "name":"Audio","muted":false,
                    "clips":[{"asset":key,"start_ms":0,"duration_ms":100,"gain_db":0}]
                }]},
                "sources":[{"key":{"namespace":"studio","name":"audio/mp3-preview"},
                    "sha256":source.sha256,"codec":"mp3"}]
            }}
        });
        let app=router(db.clone());
        let created=app.clone().oneshot(request(Method::POST,
            "/api/v1/audio/projects/mp3-preview",
            serde_json::to_vec(&doc).unwrap())).await.unwrap();
        assert_eq!(created.status(),StatusCode::OK);
        let response=app.clone().oneshot(request(Method::GET,
            "/api/v1/audio/projects/mp3-preview/render",vec![])).await.unwrap();
        assert_eq!(response.status(),StatusCode::UNPROCESSABLE_ENTITY);
        drop(app);drop(db);let _=std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn preview_does_not_read_assets_of_other_plugin_namespaces() {
        let (db,dir)=store();
        let source_key=AssetKey::new("private","audio/secret").unwrap();
        let bytes=encode_wav_pcm16(&StereoAudio {
            sample_rate:8_000,frames:vec![[0.4,0.4];800],
        }).unwrap();
        let record=db.publish(source_key.clone(),AssetKind::Sound,&bytes).unwrap();
        let document=json!({"magic":"VSMEDIA","version":1,
            "payload":{"kind":"audio","data":{
                "timeline":{"name":"Forbidden source","tracks":[{
                    "name":"Private","muted":false,"clips":[{
                        "asset":source_key,"start_ms":0,"duration_ms":100,"gain_db":0
                    }]
                }]},
                "sources":[{"key":{"namespace":"private","name":"audio/secret"},
                    "sha256":record.sha256,"codec":"wav"}]
            }}
        });
        let app=router(db.clone());
        let created=app.clone().oneshot(request(Method::POST,
            "/api/v1/audio/projects/forbidden",
            serde_json::to_vec(&document).unwrap()
        )).await.unwrap();
        assert_eq!(created.status(),StatusCode::OK);
        let attempted=app.clone().oneshot(request(Method::GET,
            "/api/v1/audio/projects/forbidden/render",vec![]
        )).await.unwrap();
        assert_eq!(attempted.status(),StatusCode::FORBIDDEN);
        drop(app);drop(db);let _=std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn studio_local_writes_reject_external_origins() {
        let (db,dir)=store();
        let invalid=Request::builder().method(Method::POST)
            .uri("/api/v1/assets/a")
            .header(header::ORIGIN,"http://malicious.example")
            .body(Body::from("data")).unwrap();
        let result=router(db.clone()).oneshot(invalid).await.unwrap();
        assert_eq!(result.status(),StatusCode::FORBIDDEN);
        drop(db);
        let _=std::fs::remove_dir_all(dir);
    }
}

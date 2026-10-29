//! Development-only VSMedia Studio HTTP bridge. Reuses the actual SQLite CAS,
//! native document parser, project revisions and non-destructive Studio editor.
//! Only listens on loopback. No plugin broadcasting or radio transmitters.
use std::{collections::HashMap, net::SocketAddr, path::PathBuf, sync::Arc};
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
async fn render_audio_project(
    State(store):State<Store>,Path(name):Path<String>
)->Result<Response,ApiError> {
    let key=key(format!("projects/{name}"))?;
    let wav=tokio::task::spawn_blocking(move||->Result<Vec<u8>,ApiError>{
        let document=vsmedia_formats::load(store.as_ref(),&key,"vsaudio")
            .map_err(not_found)?;
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
    }).await.map_err(|e|ApiError(StatusCode::INTERNAL_SERVER_ERROR,
        format!("Audio render task terminated: {e}")))??;
    Ok(([
        (header::CONTENT_TYPE,"audio/wav"),
        (header::CACHE_CONTROL,"no-store"),
        (header::CONTENT_DISPOSITION,"inline")
    ],wav).into_response())
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
        .route("/api/v1/assets/{name}/metadata",get(get_asset_metadata))
        .route("/api/v1/assets/{name}",post(put_asset).get(get_asset))
        .route("/api/v1/audio/projects",get(list_audio_projects))
        .route("/api/v1/audio/projects/{name}/metadata",get(get_audio_metadata))
        .route("/api/v1/audio/projects/{name}",post(put_audio).get(get_audio))
        .route("/api/v1/audio/projects/{name}/render",get(render_audio_project))
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

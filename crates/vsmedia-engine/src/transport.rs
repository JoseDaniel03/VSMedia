//! Loopback-only authenticated IPC. No public HTTP listener, no Minecraft I/O here.
use base64::{engine::general_purpose::STANDARD, Engine as _};
use std::{collections::HashMap, sync::Arc, time::Duration};
use crate::transfers::{DownloadRegistry, UploadRegistry, CHUNK_BYTES};
use subtle::ConstantTimeEq;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    sync::Semaphore,
    time::timeout,
};
use vsmedia_api::{AssetRepository, MediaError};
use vsmedia_core::sqlite::SqliteAssetStore;
use vsmedia_protocol::{
    WireCommand, WireRequest, WireResponse, WireResult, MAX_FRAME_BYTES,
    MAX_INLINE_BLOB_BYTES, WIRE_VERSION,
};

const MAX_CONNECTIONS: usize = 32;
const IO_TIMEOUT: Duration = Duration::from_secs(10);
type TokenMap = HashMap<String, String>;

pub async fn serve(
    port: u16,
    store: Arc<SqliteAssetStore>,
    tokens: TokenMap,
) -> std::io::Result<()> {
    let listener = TcpListener::bind(("127.0.0.1", port)).await?;
    println!("VSMedia internal IPC listening on 127.0.0.1:{port}");
    let permit_limit = Arc::new(Semaphore::new(MAX_CONNECTIONS));
    let tokens = Arc::new(tokens);
    let transfers = Arc::new(DownloadRegistry::default());
    let uploads = Arc::new(UploadRegistry::default());
    loop {
        let permit = permit_limit.clone().acquire_owned().await
            .map_err(std::io::Error::other)?;
        let (stream, _) = listener.accept().await?;
        let store = Arc::clone(&store);
        let tokens = Arc::clone(&tokens);
        let transfers = Arc::clone(&transfers);
        let uploads = Arc::clone(&uploads);
        tokio::spawn(async move {
            let _permit = permit;
            // Do not log requests or credentials. Each connection carries one request.
            let _ = timeout(IO_TIMEOUT, process_connection(stream, store, tokens, transfers, uploads)).await;
        });
    }
}
async fn process_connection(
    mut stream: TcpStream,
    store: Arc<SqliteAssetStore>,
    tokens: Arc<TokenMap>,
    transfers: Arc<DownloadRegistry>,
    uploads: Arc<UploadRegistry>,
) -> std::io::Result<()> {
    let length = stream.read_u32().await? as usize;
    if length == 0 || length > MAX_FRAME_BYTES {
        return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "invalid frame size"));
    }
    let mut buf = vec![0; length];
    stream.read_exact(&mut buf).await?;
    let request: WireRequest = match serde_json::from_slice(&buf) {
        Ok(value) => value,
        Err(_) => {
            send(&mut stream, &WireResponse::failure("".into(), "invalid_json", "malformed request")).await?;
            return Ok(());
        }
    };
    let response = tokio::task::spawn_blocking(move || dispatch(store.as_ref(), &tokens, transfers.as_ref(), uploads.as_ref(), request))
        .await.map_err(std::io::Error::other)?;
    send(&mut stream, &response).await?;
    Ok(())
}
async fn send(stream: &mut TcpStream, response: &WireResponse) -> std::io::Result<()> {
    let body = serde_json::to_vec(response).map_err(std::io::Error::other)?;
    if body.len() > MAX_FRAME_BYTES {
        return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "response too large"));
    }
    stream.write_u32(body.len() as u32).await?;
    stream.write_all(&body).await?;
    stream.flush().await
}
fn dispatch(store: &SqliteAssetStore, tokens: &TokenMap, transfers: &DownloadRegistry, uploads: &UploadRegistry, req: WireRequest) -> WireResponse {
    let id = req.request_id.clone();
    let outcome = handle(store, tokens, transfers, uploads, req);
    match outcome {
        Ok(value) => WireResponse::ok(id, value),
        Err((code, message)) => WireResponse::failure(id, code, &message),
    }
}
type Failure = (&'static str, String);
fn handle(store: &SqliteAssetStore, tokens: &TokenMap, transfers: &DownloadRegistry, uploads: &UploadRegistry, req: WireRequest) -> Result<WireResult, Failure> {
    if req.version != WIRE_VERSION {
        return Err(("version_mismatch", "unsupported API version".into()));
    }
    if req.request_id.is_empty() || req.request_id.len() > 64 || !req.request_id.is_ascii() {
        return Err(("invalid_request", "invalid request id".into()));
    }
    // Fail closed: a token never grants access to another producer namespace.
    let authorized = tokens.get(&req.producer).is_some_and(|expected|
        expected.as_bytes().ct_eq(req.token.as_bytes()).unwrap_u8() == 1
    );
    if !authorized {
        return Err(("unauthorized", "invalid credentials".into()));
    }
    match req.command {
        WireCommand::Ping => Ok(WireResult::Pong),
        WireCommand::Publish { key, kind, content_base64 } => {
            store.authorize(&req.producer, &key.namespace, true).map_err(failure)?;
            // Check encoded size before decoding; avoid allocating huge buffers.
            if content_base64.len() > (MAX_INLINE_BLOB_BYTES / 3 + 1) * 4 {
                return Err(("too_large", "asset exceeds IPC inline limit".into()));
            }
            let bytes = decode_blob(&content_base64)?;
            let record = store.publish(key, kind, &bytes).map_err(failure)?;
            Ok(WireResult::Asset { record })
        }
        WireCommand::PublishNative { key, extension, content_base64 } => {
            store.authorize(&req.producer, &key.namespace, true).map_err(failure)?;
            let bytes=decode_blob(&content_base64)?;
            let doc=vsmedia_formats::decode(&bytes,&extension).map_err(failure)?;
            let record=vsmedia_formats::publish(store,key,&doc).map_err(failure)?;
            Ok(WireResult::Asset { record })
        }
        WireCommand::ReadNative { key, extension } => {
            store.authorize(&req.producer, &key.namespace, false).map_err(failure)?;
            let record=store.metadata(&key).map_err(failure)?;
            if record.size > MAX_INLINE_BLOB_BYTES as u64 {
                return Err(("too_large", "native manifest exceeds IPC limit".into()));
            }
            let doc=vsmedia_formats::load(store,&key,&extension).map_err(failure)?;
            let bytes=vsmedia_formats::encode(&doc).map_err(failure)?;
            Ok(WireResult::Content { content_base64: STANDARD.encode(bytes) })
        }
        WireCommand::Lookup { key } => {
            store.authorize(&req.producer, &key.namespace, false).map_err(failure)?;
            Ok(WireResult::Asset { record: store.metadata(&key).map_err(failure)? })
        }
        WireCommand::BeginUpload { key, kind, size, sha256 } => {
            store.authorize(&req.producer, &key.namespace, true).map_err(failure)?;
            let transfer_id=uploads.begin(&req.producer,key,kind,size,sha256).map_err(failure)?;
            Ok(WireResult::Upload { transfer_id, offset:0 })
        }
        WireCommand::UploadChunk { transfer_id, offset, content_base64 } => {
            if content_base64.len() > (CHUNK_BYTES/3+1)*4+4 {
                return Err(("too_large", "upload chunk exceeds limit".into()));
            }
            let bytes=STANDARD.decode(content_base64)
                .map_err(|_| ("invalid_request","invalid base64 chunk".into()))?;
            let next=uploads.chunk(&req.producer,&transfer_id,offset,&bytes).map_err(failure)?;
            Ok(WireResult::Upload { transfer_id, offset:next })
        }
        WireCommand::CompleteUpload { transfer_id } => {
            let record=uploads.finish(store,&req.producer,&transfer_id).map_err(failure)?;
            Ok(WireResult::Asset { record })
        }
        WireCommand::AbortUpload { transfer_id } => {
            uploads.abort(&req.producer,&transfer_id).map_err(failure)?;
            Ok(WireResult::Acknowledged)
        }
        WireCommand::BeginRead { key } => {
            store.authorize(&req.producer, &key.namespace, false).map_err(failure)?;
            let (transfer_id, size, sha256) =
                transfers.begin(store, &req.producer, &key).map_err(failure)?;
            Ok(WireResult::Download { transfer_id, size, sha256 })
        }
        WireCommand::ReadChunk { transfer_id, offset } => {
            // The session is bound to an authenticated producer and key; a
            // revoked namespace read grant also invalidates the next chunk.
            let key=transfers.key(&req.producer, &transfer_id).map_err(failure)?;
            store.authorize(&req.producer, &key.namespace, false).map_err(failure)?;
            let (chunk, done)=transfers.chunk(&req.producer, &transfer_id, offset).map_err(failure)?;
            Ok(WireResult::Chunk {
                offset, content_base64: STANDARD.encode(chunk), done
            })
        }
        WireCommand::EndRead { transfer_id } => {
            transfers.end(&req.producer, &transfer_id).map_err(failure)?;
            Ok(WireResult::Acknowledged)
        }
        WireCommand::Read { key } => {
            store.authorize(&req.producer, &key.namespace, false).map_err(failure)?;
            let record = store.metadata(&key).map_err(failure)?;
            if record.size > MAX_INLINE_BLOB_BYTES as u64 {
                return Err(("too_large", "asset exceeds IPC inline limit".into()));
            }
            let bytes = store.read(&key).map_err(failure)?;
            Ok(WireResult::Content { content_base64: STANDARD.encode(bytes) })
        }
    }
}
fn decode_blob(content_base64:&str) -> Result<Vec<u8>, Failure> {
    if content_base64.len() > (MAX_INLINE_BLOB_BYTES / 3 + 1) * 4 {
        return Err(("too_large", "asset exceeds IPC inline limit".into()));
    }
    let bytes=STANDARD.decode(content_base64)
        .map_err(|_| ("invalid_request", "invalid base64 data".into()))?;
    if bytes.len() > MAX_INLINE_BLOB_BYTES {
        return Err(("too_large", "asset exceeds IPC inline limit".into()));
    }
    Ok(bytes)
}
fn failure(err: MediaError) -> Failure {
    match err {
        MediaError::InvalidInput(_) => ("forbidden", "invalid input or permission denied".into()),
        MediaError::NotFound(_) => ("not_found", "asset not found".into()),
        MediaError::Conflict(_) => ("conflict", "asset conflict".into()),
        MediaError::Io(_) => ("internal", "internal storage failure".into()),
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use std::{fs, path::PathBuf, sync::atomic::{AtomicU64, Ordering}};
    use vsmedia_api::{AssetKey, AssetKind};
    fn transfers()->DownloadRegistry { DownloadRegistry::default() }
    fn uploads()->UploadRegistry { UploadRegistry::default() }
    fn setup() -> (PathBuf, SqliteAssetStore, TokenMap) {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let root = std::env::temp_dir().join(format!(
            "vsmedia-ipc-test-{}-{}", std::process::id(), COUNTER.fetch_add(1, Ordering::Relaxed)));
        let db = SqliteAssetStore::open(&root).unwrap();
        db.grant("vsrender", "demo", true, true).unwrap();
        (root, db, HashMap::from([("vsrender".into(), "secret-token".into())]))
    }
    fn request(command: WireCommand) -> WireRequest {
        WireRequest { version: WIRE_VERSION, request_id: "req-1".into(),
            producer: "vsrender".into(), token: "secret-token".into(), command }
    }
    #[test]
    fn authenticated_publish_and_read() {
        let (root, db, tokens) = setup();
        let key = AssetKey::new("demo", "music/intro.ogg").unwrap();
        let pub_result = dispatch(&db, &tokens, &transfers(),&uploads(),
            request(WireCommand::Publish { key: key.clone(), kind: AssetKind::Sound,
                content_base64: STANDARD.encode(b"hello") }));
        assert!(pub_result.error.is_none());
        let read_result = dispatch(&db, &tokens, &transfers(), &uploads(), request(WireCommand::Read { key }));
        match read_result.result.unwrap() {
            WireResult::Content { content_base64 } => assert_eq!(STANDARD.decode(content_base64).unwrap(), b"hello"),
            _ => panic!("expected content"),
        }
        drop(db);
        let _ = fs::remove_dir_all(root);
    }
    #[test]
    fn native_format_calls_validate_extension_and_round_trip() {
        use vsmedia_formats::{NativeDocument,NativePayload,VSProject};
        let (root,db,tokens)=setup();
        let key=AssetKey::new("demo","project/master.vsproject").unwrap();
        let doc=NativeDocument::new(NativePayload::Project(VSProject {
            name:"Master Project".into(),nodes:vec![],entry_points:vec![]
        }));
        let bytes=vsmedia_formats::encode(&doc).unwrap();
        let bad=dispatch(&db,&tokens,&transfers(),&uploads(),request(WireCommand::PublishNative {
            key:key.clone(),extension:"vsaudio".into(),
            content_base64:STANDARD.encode(&bytes)
        }));
        assert_eq!(bad.error.unwrap().code,"forbidden");
        let saved=dispatch(&db,&tokens,&transfers(),&uploads(),request(WireCommand::PublishNative {
            key:key.clone(),extension:"vsproject".into(),
            content_base64:STANDARD.encode(&bytes)
        }));
        assert!(saved.error.is_none());
        let retrieved=dispatch(&db,&tokens,&transfers(),&uploads(),request(WireCommand::ReadNative {
            key,extension:"vsproject".into()
        }));
        match retrieved.result.unwrap() {
            WireResult::Content{content_base64} => {
                let raw=STANDARD.decode(content_base64).unwrap();
                assert_eq!(vsmedia_formats::decode(&raw,"vsproject").unwrap().kind(),
                    vsmedia_formats::NativeKind::Project);
            }
            _ => panic!("expected native document bytes"),
        }
        drop(db);
        let _=fs::remove_dir_all(root);
    }
    #[test]
    fn credentials_and_acl_are_both_checked() {
        let (root, db, tokens) = setup();
        let key = AssetKey::new("private", "data").unwrap();
        assert_eq!(dispatch(&db, &tokens, &transfers(), &uploads(), request(WireCommand::Lookup { key })).error.unwrap().code, "forbidden");
        let mut forged = request(WireCommand::Ping);
        forged.token = "wrong".into();
        assert_eq!(dispatch(&db, &tokens, &transfers(), &uploads(), forged).error.unwrap().code, "unauthorized");
        drop(db);
        let _ = fs::remove_dir_all(root);
    }
}

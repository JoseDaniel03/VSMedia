//! Loopback-only authenticated IPC. No public HTTP listener, no Minecraft I/O here.
use base64::{engine::general_purpose::STANDARD, Engine as _};
use std::{collections::HashMap, sync::Arc, time::Duration};
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
    loop {
        let permit = permit_limit.clone().acquire_owned().await
            .map_err(std::io::Error::other)?;
        let (stream, _) = listener.accept().await?;
        let store = Arc::clone(&store);
        let tokens = Arc::clone(&tokens);
        tokio::spawn(async move {
            let _permit = permit;
            // Do not log requests or credentials. Each connection carries one request.
            let _ = timeout(IO_TIMEOUT, process_connection(stream, store, tokens)).await;
        });
    }
}
async fn process_connection(
    mut stream: TcpStream,
    store: Arc<SqliteAssetStore>,
    tokens: Arc<TokenMap>,
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
    let response = tokio::task::spawn_blocking(move || dispatch(store.as_ref(), &tokens, request))
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
fn dispatch(store: &SqliteAssetStore, tokens: &TokenMap, req: WireRequest) -> WireResponse {
    let id = req.request_id.clone();
    let outcome = handle(store, tokens, req);
    match outcome {
        Ok(value) => WireResponse::ok(id, value),
        Err((code, message)) => WireResponse::failure(id, code, &message),
    }
}
type Failure = (&'static str, String);
fn handle(store: &SqliteAssetStore, tokens: &TokenMap, req: WireRequest) -> Result<WireResult, Failure> {
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
            let bytes = STANDARD.decode(content_base64)
                .map_err(|_| ("invalid_request", "invalid base64 data".into()))?;
            if bytes.len() > MAX_INLINE_BLOB_BYTES {
                return Err(("too_large", "asset exceeds IPC inline limit".into()));
            }
            let record = store.publish(key, kind, &bytes).map_err(failure)?;
            Ok(WireResult::Asset { record })
        }
        WireCommand::Lookup { key } => {
            store.authorize(&req.producer, &key.namespace, false).map_err(failure)?;
            Ok(WireResult::Asset { record: store.metadata(&key).map_err(failure)? })
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
        let pub_result = dispatch(&db, &tokens,
            request(WireCommand::Publish { key: key.clone(), kind: AssetKind::Sound,
                content_base64: STANDARD.encode(b"hello") }));
        assert!(pub_result.error.is_none());
        let read_result = dispatch(&db, &tokens, request(WireCommand::Read { key }));
        match read_result.result.unwrap() {
            WireResult::Content { content_base64 } => assert_eq!(STANDARD.decode(content_base64).unwrap(), b"hello"),
            _ => panic!("expected content"),
        }
        drop(db);
        let _ = fs::remove_dir_all(root);
    }
    #[test]
    fn credentials_and_acl_are_both_checked() {
        let (root, db, tokens) = setup();
        let key = AssetKey::new("private", "data").unwrap();
        assert_eq!(dispatch(&db, &tokens, request(WireCommand::Lookup { key })).error.unwrap().code, "forbidden");
        let mut forged = request(WireCommand::Ping);
        forged.token = "wrong".into();
        assert_eq!(dispatch(&db, &tokens, forged).error.unwrap().code, "unauthorized");
        drop(db);
        let _ = fs::remove_dir_all(root);
    }
}

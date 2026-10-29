//! VSMedia Rust engine and loopback IPC daemon (development protocol v1).
mod transport;
mod transfers;
use std::{collections::HashMap, env, fs, path::Path, sync::Arc};
use vsmedia_core::sqlite::SqliteAssetStore;

fn help() -> ! {
    eprintln!("Usage:
  vsmedia-engine status
  vsmedia-engine grant <data-dir> <producer> <namespace> <read|write|both>
  vsmedia-engine serve <data-dir> <tokens-json> [port]");
    std::process::exit(2);
}
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = env::args().collect();
    match args.get(1).map(String::as_str) {
        Some("status") if args.len() == 2 => {
            println!("VSMedia Engine 0.1.0 — API v{}", vsmedia_api::API_VERSION);
            println!("Modules: core, pack, audio, image, font; IPC development v1");
        }
        Some("grant") if args.len() == 6 => {
            let db = SqliteAssetStore::open(&args[2])?;
            let (publish, read) = match args[5].as_str() {
                "read" => (false, true),
                "write" => (true, false),
                "both" => (true, true),
                _ => help(),
            };
            db.grant(&args[3], &args[4], publish, read)?;
            println!("Updated producer permissions");
        }
        Some("serve") if args.len() == 4 || args.len() == 5 => {
            let port: u16 = args.get(4).map_or(Ok(3059), |s| s.parse())?;
            if port == 0 { return Err("port 0 is not allowed".into()); }
            let db = Arc::new(SqliteAssetStore::open(&args[2])?);
            let tokens_file = Path::new(&args[3]);
            let metadata = fs::metadata(tokens_file)?;
            if !metadata.is_file() || metadata.len() > 16384 {
                return Err("token file must be a small regular file".into());
            }
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                if metadata.permissions().mode() & 0o077 != 0 {
                    return Err("token file must have permissions 0600 or stricter".into());
                }
            }
            let tokens: HashMap<String, String> = serde_json::from_slice(&fs::read(tokens_file)?)?;
            if tokens.is_empty() || tokens.len() > 256 ||
                tokens.iter().any(|(name, token)| {
                    vsmedia_api::AssetKey::new(name, "auth").is_err() ||
                    !(24..=256).contains(&token.len()) || !token.is_ascii()
                })
            {
                return Err("invalid token map".into());
            }
            transport::serve(port, db, tokens).await?;
        }
        _ => help(),
    }
    Ok(())
}

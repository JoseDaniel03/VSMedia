//! Minimal development entrypoint. This does not expose IPC, audio streaming or a Minecraft plugin.
fn main() {
    let args: Vec<_> = std::env::args().collect();
    match args.get(1).map(String::as_str) {
        Some("status") => {
            println!("VSMedia Engine 0.1.0 — API v{}", vsmedia_api::API_VERSION);
            println!("Modules: core, pack, audio, image, font");
            println!("Mode: scaffolding; no server transport active");
        }
        _ => {
            eprintln!("Usage: vsmedia-engine status");
            std::process::exit(2);
        }
    }
}

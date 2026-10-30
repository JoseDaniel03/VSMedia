mod app;
mod media;
mod recording;
mod export;
mod api_contract;
mod teleprompter;
mod studio_api;
mod mix_timeline;
mod waveform;
mod gain_editor;
mod mix_history;
mod mix_batch;

fn main() {
    console_error_panic_hook::set_once();
    leptos::mount::mount_to_body(app::App);
}

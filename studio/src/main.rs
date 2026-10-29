mod app;
mod media;
mod recording;
mod export;
mod api_contract;
mod teleprompter;
mod studio_api;

fn main() {
    console_error_panic_hook::set_once();
    leptos::mount::mount_to_body(app::App);
}

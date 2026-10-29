//! Grabación de micrófono mediante MediaRecorder, sin backend ni JS de aplicación.
use crate::media::{AudioAsset,add_blob,uid};
use leptos::prelude::*;
use wasm_bindgen::{JsCast, JsValue, closure::Closure};
use wasm_bindgen_futures::{spawn_local, JsFuture};
use web_sys::{BlobEvent, MediaRecorder, MediaStream, MediaStreamConstraints, MediaStreamTrack};
use std::cell::RefCell;

struct Session {
    recorder: MediaRecorder,
    stream: MediaStream,
    _on_data: Closure<dyn FnMut(BlobEvent)>,
}
thread_local! {static SESSION: RefCell<Option<Session>> = const { RefCell::new(None) };}

pub fn start(assets: RwSignal<Vec<AudioAsset>>, status: RwSignal<String>, active: RwSignal<bool>, on_complete: impl Fn(String) + 'static) {
    if active.get_untracked() {return;}
    status.set("Solicitando permiso para usar el micrófono…".to_owned());
    spawn_local(async move {
        let result: Result<(),String> = async {
            let window = web_sys::window().ok_or("No hay ventana")?;
            let devices = window.navigator().media_devices().map_err(|_|"El navegador no permite acceder al micrófono")?;
            let constraints = MediaStreamConstraints::new();
            constraints.set_audio(&JsValue::TRUE);
            let promise = devices.get_user_media_with_constraints(&constraints).map_err(|_|"No se pudo solicitar el micrófono")?;
            let stream_val = JsFuture::from(promise).await.map_err(|_|"Permiso de micrófono rechazado o dispositivo no disponible")?;
            let stream: MediaStream = stream_val.dyn_into().map_err(|_|"Micrófono no válido")?;
            let recorder = MediaRecorder::new_with_media_stream(&stream).map_err(|_|"MediaRecorder no compatible con este navegador")?;
            let mime = recorder.mime_type();
            let extension = if mime.contains("mp4") { "m4a" } else if mime.contains("ogg") { "ogg" } else { "webm" };
            let callback = Closure::<dyn FnMut(BlobEvent)>::new(move |evt: BlobEvent| {
                let blob = evt.data();
                if blob.size() <= 0. {status.set("La grabación llegó vacía; intentá nuevamente.".to_owned());active.set(false);return;}
                let name = format!("Grabacion-{}.{}",uid("mic"),extension);
                if let Some(id) = add_blob(blob,name,assets,status) {on_complete(id);}
                active.set(false);
                status.set("Grabación terminada e incorporada a la biblioteca.".to_owned());
            });
            recorder.set_ondataavailable(Some(callback.as_ref().unchecked_ref()));
            recorder.start().map_err(|_|"No se pudo iniciar la captura")?;
            SESSION.with(|cell| *cell.borrow_mut()=Some(Session{recorder,stream,_on_data:callback}));
            Ok(())
        }.await;
        match result {
            Ok(()) => {active.set(true);status.set("Grabando micrófono. Pulsá Detener grabación para terminar.".to_owned());}
            Err(err) => {active.set(false); status.set(err);}
        }
    });
}

pub fn stop(_active: RwSignal<bool>, status: RwSignal<String>) {
    SESSION.with(|cell| {
        if let Some(session)=cell.borrow().as_ref() {
            if session.recorder.stop().is_ok() {
                let tracks=session.stream.get_tracks();
                for index in 0..tracks.length() {
                    if let Ok(track)=tracks.get(index).dyn_into::<MediaStreamTrack>() {track.stop();}
                }
                status.set("Procesando grabación…".to_owned());
            }
        }
    });
}

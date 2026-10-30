//! Local, bounded waveform thumbnails for the FullCanvas timeline.
//! Decoding happens only after an explicit "Ondas" action, never on import.
//! Peaks are cached in memory; raw PCM is never stored in VSMedia state or CAS.
use crate::media::{self, AudioAsset, Track};
use leptos::prelude::*;
use js_sys::ArrayBuffer;
use std::collections::{HashMap, HashSet};
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::{spawn_local, JsFuture};
use web_sys::{AudioBuffer, AudioContext};

const PEAK_BUCKETS: usize = 768;
const MAX_DECODED_FRAMES: usize = 10_000_000;
const MAX_FILE_BYTES: f64 = 16.0 * 1024.0 * 1024.0;

#[derive(Clone)]
pub struct Waveform {
    duration: f64,
    peaks: Vec<f32>,
}
impl Waveform {
    /// A compact symmetric envelope for the visible region of this asset.
    /// Output has fixed complexity irrespective of audio duration and zoom.
    pub fn path(&self, source_in:f64, duration:f64) -> String {
        const POINTS: usize = 120;
        if self.peaks.is_empty() || self.duration <= 0. || !duration.is_finite() {
            return String::new();
        }
        let mut upper = Vec::with_capacity(POINTS);
        let mut lower = Vec::with_capacity(POINTS);
        for index in 0..POINTS {
            let time = source_in + duration * index as f64 / (POINTS - 1) as f64;
            let amplitude = if time.is_finite() && (0.0..self.duration).contains(&time) {
                let bucket = ((time / self.duration) * self.peaks.len() as f64) as usize;
                self.peaks[bucket.min(self.peaks.len()-1)].clamp(0., 1.)
            } else { 0. };
            let x = index as f32 * 120.0 / (POINTS - 1) as f32;
            upper.push((x, 12.0 - amplitude * 10.0));
            lower.push((x, 12.0 + amplitude * 10.0));
        }
        let mut path = String::with_capacity(POINTS * 28);
        for (i,(x,y)) in upper.into_iter().enumerate() {
            path.push_str(&format!("{} {:.2} {:.2}", if i==0 {"M"}else{"L"},x,y));
        }
        for (x,y) in lower.into_iter().rev() {
            path.push_str(&format!(" L {:.2} {:.2}",x,y));
        }
        path.push_str(" Z");
        path
    }
}

/// Extract extrema in bounded buckets, rather than duplicating entire PCM
/// channels into WASM memory. The browser releases the decoded AudioBuffer.
async fn decode(id:&str) -> Result<Waveform,String> {
    let blob=media::get_blob(id).ok_or("El audio ya no está disponible en memoria")?;
    if blob.size()<=0. || blob.size()>MAX_FILE_BYTES {
        return Err("Solo se analizan audios de hasta 16 MiB".into());
    }
    let context=AudioContext::new().map_err(|_|"No se pudo iniciar Web Audio")?;
    let result=async {
        let value=JsFuture::from(blob.array_buffer()).await
            .map_err(|_|"No se pudo leer el audio local")?;
        let bytes:ArrayBuffer=value.dyn_into()
            .map_err(|_|"El archivo no contiene bytes válidos")?;
        let decoded=JsFuture::from(context.decode_audio_data(&bytes)
            .map_err(|_|"El navegador no pudo decodificar el archivo")?).await
            .map_err(|_|"El navegador no admite el códec de este audio")?;
        let buffer:AudioBuffer=decoded.dyn_into()
            .map_err(|_|"La respuesta de Web Audio es inválida")?;
        let frames=buffer.length() as usize;
        let duration=buffer.duration();
        if frames==0 || frames>MAX_DECODED_FRAMES || !duration.is_finite() || duration<=0. {
            return Err("Audio demasiado largo para esta vista previa local".into());
        }
        let stereo=buffer.number_of_channels()>1;
        let count=frames.min(PEAK_BUCKETS);
        let mut peaks=Vec::with_capacity(count);
        for bucket in 0..count {
            let start=bucket*frames/count;
            let end=(bucket+1)*frames/count;
            // Read only ~13k frames per bucket. Do not copy full PCM into WASM.
            let mut left=vec![0.0f32;end-start];
            buffer.copy_from_channel_with_start_in_channel(&mut left,0,start as u32)
                .map_err(|_|"No se pudo leer el canal izquierdo")?;
            let mut amplitude=left.iter().fold(0.0f32,|max,s|max.max(s.abs()));
            if stereo {
                let mut right=vec![0.0f32;end-start];
                buffer.copy_from_channel_with_start_in_channel(&mut right,1,start as u32)
                    .map_err(|_|"No se pudo leer el canal derecho")?;
                amplitude=right.iter().fold(amplitude,|max,s|max.max(s.abs()));
            }
            peaks.push(amplitude.min(1.));
        }
        Ok(Waveform {duration,peaks})
    }.await;
    let _=context.close();
    result
}

/// Called only by a user button. Decode each referenced source at most once,
/// sequentially, to avoid concurrent large AudioBuffer allocations.
pub fn load_for_mix(
    assets:RwSignal<Vec<AudioAsset>>, tracks:RwSignal<Vec<Track>>,
    cache:RwSignal<HashMap<String, Waveform>>, status:RwSignal<String>,
    busy:RwSignal<bool>,
) {
    if busy.get_untracked() {return;}
    let referenced:HashSet<String>=tracks.get_untracked().iter()
        .flat_map(|track|track.regions.iter().map(|region|region.asset_id.clone()))
        .collect();
    let known:HashSet<String>=assets.get_untracked().iter().map(|item|item.id.clone()).collect();
    let to_decode:Vec<String>=referenced.into_iter().filter(|id|
        known.contains(id) && !cache.get_untracked().contains_key(id)).collect();
    if to_decode.is_empty() {
        status.set("Las ondas disponibles ya están en caché local.".into());
        return;
    }
    busy.set(true);
    status.set(format!("Analizando {} fuentes de audio localmente…",to_decode.len()));
    spawn_local(async move {
        let mut ok=0usize;
        let mut skipped=0usize;
        for id in to_decode {
            match decode(&id).await {
                Ok(waveform)=>{ cache.update(|all|{all.insert(id,waveform);});ok+=1; }
                Err(_)=>skipped+=1,
            }
        }
        busy.set(false);
        status.set(format!("Formas de onda listas: {ok}. No compatibles o demasiado largas: {skipped}. Sin subir archivos al servidor."));
    });
}

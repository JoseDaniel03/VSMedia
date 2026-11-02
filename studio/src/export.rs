//! Exportación WAV real en Rust/WASM a partir del decodificador nativo de Web Audio.
//! Operaciones aplicadas a muestras PCM: recorte, ganancia, fades y reversa.
use crate::media::{self, AudioAsset, Version, download_blob};
use js_sys::{Array, ArrayBuffer, Uint8Array};
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::{spawn_local, JsFuture};
use leptos::prelude::*;
use web_sys::{AudioContext, Blob};

pub fn export_wav(asset: AudioAsset, version: Version, status: RwSignal<String>) {
    status.set("Decodificando audio para exportación WAV…".to_owned());
    spawn_local(async move {
        let result = render_wav(asset.clone(), version.clone()).await;
        match result {
            Ok(blob) => {
                let stem = asset.name.rsplit_once('.').map(|(stem,_)|stem).unwrap_or(&asset.name);
                let name = format!("{}-{}.wav",stem,version.label.trim().replace(' ',"-"));
                match download_blob(&blob,&name) {
                    Ok(()) => status.set(format!("Exportación WAV preparada: {}",name)),
                    Err(e) => status.set(e),
                }
            }
            Err(e) => status.set(format!("Error al exportar: {}",e)),
        }
    });
}

async fn render_wav(asset: AudioAsset, v: Version) -> Result<Blob,String> {
    let context = AudioContext::new().map_err(|_|"El navegador no admite AudioContext")?;
    let source=media::get_blob(&asset.id).ok_or("El archivo ya no está en memoria")?;
    let bytes = JsFuture::from(source.array_buffer()).await.map_err(|_|"No se pudo leer el archivo")?;
    let raw: ArrayBuffer = bytes.dyn_into().map_err(|_|"Contenido de audio inválido")?;
    let promise = context.decode_audio_data(&raw).map_err(|_|"Formato no reconocido por Web Audio")?;
    let decoded = JsFuture::from(promise).await.map_err(|_|"No se pudo decodificar este formato de audio")?;
    let buffer: web_sys::AudioBuffer = decoded.dyn_into().map_err(|_|"Datos de audio inválidos")?;
    let channels = buffer.number_of_channels().max(1).min(2) as usize;
    let sample_rate = buffer.sample_rate();
    let length = buffer.length() as usize;
    let start = ((v.start.max(0.) * sample_rate as f64) as usize).min(length);
    let requested_end = if v.end <= 0. { length } else { (v.end * sample_rate as f64) as usize };
    let end = requested_end.min(length);
    if end <= start { return Err("El recorte termina antes de comenzar".into()); }
    let samples = end-start;
    if samples > 100_000_000 {return Err("Archivo demasiado grande para exportar en el navegador".into());}
    let mut audio = Vec::<Vec<f32>>::new();
    for c in 0..channels {
        let pcm = buffer.get_channel_data(c as u32).map_err(|_|"No se pudo leer el canal PCM")?;
        if pcm.len() != length {return Err("Cantidad de muestras inesperada".into());}
        audio.push(pcm);
    }
    let capacity = 44usize.saturating_add(samples.saturating_mul(channels).saturating_mul(2));
    if capacity > u32::MAX as usize { return Err("WAV superior al límite del formato".into()); }
    let mut out = Vec::with_capacity(capacity);
    out.extend_from_slice(b"RIFF");out.extend_from_slice(&((capacity-8) as u32).to_le_bytes());out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes());out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&(channels as u16).to_le_bytes());
    out.extend_from_slice(&(sample_rate as u32).to_le_bytes());
    out.extend_from_slice(&((sample_rate as u32)*channels as u32*2).to_le_bytes());
    out.extend_from_slice(&((channels*2) as u16).to_le_bytes());out.extend_from_slice(&16u16.to_le_bytes());
    out.extend_from_slice(b"data");out.extend_from_slice(&((samples*channels*2) as u32).to_le_bytes());
    let fade_in = (v.fade_in.max(0.) * sample_rate as f64) as usize;
    let fade_out = (v.fade_out.max(0.) * sample_rate as f64) as usize;
    for i in 0..samples {
        let source = if v.reversed { end-1-i } else { start+i };
        let in_factor = if fade_in==0 {1.} else {(i as f64/fade_in as f64).min(1.)};
        let out_factor = if fade_out==0 {1.} else {((samples-1-i) as f64/fade_out as f64).min(1.)};
        for ch in audio.iter().take(channels) {
            let s = (ch[source] as f64 * v.gain * in_factor.min(out_factor)).clamp(-1.,1.);
            out.extend_from_slice(&((s * 32767.) as i16).to_le_bytes());
        }
    }
    let array = Array::new();
    let bytes = Uint8Array::from(&out[..]);
    array.push(&bytes);
    let blob = Blob::new_with_u8_array_sequence(&array).map_err(|_|"No se pudo ensamblar el WAV")?;
    let _ = context.close();
    Ok(blob)
}

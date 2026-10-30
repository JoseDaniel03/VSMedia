//! Interactive, non-destructive VSMix timeline. All edits stay in the local
//! Track/MixRegion model; the existing draft render and .vsaudio path are reused.
use crate::media::{AudioAsset, MixFadeCurve, MixRegion, Track};
use crate::waveform::{self, Waveform};
use std::collections::HashMap;
use leptos::prelude::*;
use wasm_bindgen::JsCast;

const MIN_DURATION: f64 = 0.1;
const MAX_SECONDS: f64 = 3600.0;

#[derive(Clone, Copy, PartialEq, Eq)]
enum EditMode { Move, TrimStart, TrimEnd, FadeIn, FadeOut }

#[derive(Clone)]
struct PointerEdit {
    track_id: String,
    region_id: String,
    pointer_id: i32,
    origin_x: i32,
    delta_pixels: f64,
    mode: EditMode,
}

fn snap(seconds: f64) -> f64 {
    (seconds * 10.0).round() / 10.0
}
fn fit_fades(region: &mut MixRegion) {
    let total = region.fade_in + region.fade_out;
    if total > region.duration && total > 0.0 {
        let factor = region.duration / total;
        region.fade_in *= factor;
        region.fade_out *= factor;
    }
}
/// Shared by the preview geometry and final commit: no cumulative pointer drift.
fn transform(region: &mut MixRegion, mode: EditMode, delta: f64) {
    if !delta.is_finite() { return; }
    match mode {
        EditMode::Move => {
            region.start = (region.start + delta).clamp(0.0, MAX_SECONDS - region.duration);
        }
        EditMode::TrimStart => {
            let change = delta.clamp(
                (-region.source_in).max(-region.start),
                region.duration - MIN_DURATION,
            );
            region.start += change;
            region.source_in += change;
            region.duration -= change;
            fit_fades(region);
        }
        EditMode::TrimEnd => {
            region.duration = (region.duration + delta)
                .clamp(MIN_DURATION, MAX_SECONDS - region.start);
            fit_fades(region);
        }
        EditMode::FadeIn => {
            region.fade_in=(region.fade_in+delta)
                .clamp(0.0,(region.duration-region.fade_out).max(0.0));
        }
        EditMode::FadeOut => {
            region.fade_out=(region.fade_out-delta)
                .clamp(0.0,(region.duration-region.fade_in).max(0.0));
        }
    }
}
/// The curve preview uses the same sin(pi/2 * progress) equation as DSP.
fn envelope_path(region:&MixRegion)->String {
    if !region.duration.is_finite() || region.duration<=0.0 {
        return String::new();
    }
    let mut result=String::with_capacity(1000);
    for step in 0..=60 {
        let x=step as f64 * 2.0;
        let seconds=region.duration*step as f64/60.0;
        let enter=if region.fade_in>0.0 {seconds/region.fade_in}else{1.0};
        let exit=if region.fade_out>0.0 {
            (region.duration-seconds)/region.fade_out
        }else{1.0};
        let level=region.fade_curve.amplitude(enter)*region.fade_curve.amplitude(exit);
        let y=21.0-level*18.0;
        result.push_str(&format!("{} {:.2} {:.2} ",
            if step==0 {"M"}else{"L"},x,y));
    }
    result
}
fn preview_region(
    tracks:RwSignal<Vec<Track>>,pointer:RwSignal<Option<PointerEdit>>,
    zoom:RwSignal<f64>,track_id:&str,region_id:&str
)->Option<MixRegion> {
    let mut region=tracks.with(|all|all.iter()
        .find(|track|track.id==track_id)
        .and_then(|track|track.regions.iter().find(|region|region.id==region_id))
        .cloned())?;
    pointer.with(|state|{
        if let Some(drag)=state {
            if drag.track_id==track_id && drag.region_id==region_id {
                transform(&mut region,drag.mode,snap(drag.delta_pixels/zoom.get()));
            }
        }
    });
    Some(region)
}
fn apply_pointer_edit(
    tracks: &mut Vec<Track>, edit: &PointerEdit, seconds: f64, target: Option<&str>
) -> Result<(), &'static str> {
    let source = tracks.iter().position(|t| t.id == edit.track_id)
        .ok_or("La pista original ya no existe")?;
    let region_index = tracks[source].regions.iter()
        .position(|r| r.id == edit.region_id).ok_or("La región ya no existe")?;
    if edit.mode == EditMode::Move {
        let destination = target.and_then(|id| tracks.iter().position(|t| t.id == id))
            .unwrap_or(source);
        if destination != source && tracks[destination].regions.len() >= 32 {
            return Err("La pista de destino ya tiene 32 regiones");
        }
        let mut region = tracks[source].regions[region_index].clone();
        transform(&mut region, edit.mode, seconds);
        if destination == source {
            tracks[source].regions[region_index] = region;
        } else {
            tracks[source].regions.remove(region_index);
            if tracks[source].regions.is_empty() { tracks[source].asset_id.clear(); }
            tracks[destination].regions.push(region);
        }
    } else {
        transform(&mut tracks[source].regions[region_index], edit.mode, seconds);
    }
    Ok(())
}
/// Use the existing native linear fades, preserving the .vsaudio format.
fn apply_crossfades(tracks:&mut [Track],curve:MixFadeCurve) -> (usize,usize) {
    let mut applied=0usize;
    let mut skipped=0usize;
    for track in tracks {
        let mut order:Vec<usize>=(0..track.regions.len()).collect();
        order.sort_by(|&left,&right|
            track.regions[left].start.total_cmp(&track.regions[right].start));
        for (position,pair) in order.windows(2).enumerate() {
            let a=pair[0];
            let b=pair[1];
            let previous=&track.regions[a];
            let next=&track.regions[b];
            let first_end=previous.start+previous.duration;
            let next_end=next.start+next.duration;
            let overlap=first_end-next.start;
            if !(overlap>0.) {continue;}
            let has_third=(position>0 &&
                track.regions[order[position-1]].start
                    +track.regions[order[position-1]].duration>next.start)
                || order.get(position+2).is_some_and(|&third|
                    track.regions[third].start<first_end);
            if next_end<first_end || has_third || !overlap.is_finite()
                || previous.fade_in+overlap>previous.duration
                || next.fade_out+overlap>next.duration {
                skipped+=1;
                continue;
            }
            track.regions[a].fade_out=overlap;
            track.regions[a].fade_curve=curve;
            track.regions[b].fade_in=overlap;
            track.regions[b].fade_curve=curve;
            applied+=1;
        }
    }
    (applied,skipped)
}
fn pointer_start(
    event: &web_sys::PointerEvent,
    edit: RwSignal<Option<PointerEdit>>,
    track_id: String, region_id: String, mode: EditMode
) {
    if event.button() != 0 { return; }
    event.prevent_default();
    if let Some(element) = event.current_target()
        .and_then(|target| target.dyn_into::<web_sys::Element>().ok()) {
        let _ = element.set_pointer_capture(event.pointer_id());
    }
    edit.set(Some(PointerEdit {
        track_id, region_id, pointer_id: event.pointer_id(),
        origin_x: event.client_x(), delta_pixels: 0.0, mode,
    }));
}
fn target_lane(event: &web_sys::PointerEvent) -> Option<String> {
    let document = web_sys::window()?.document()?;
    document.element_from_point(event.client_x() as f32, event.client_y() as f32)?
        .closest("[data-mix-track]").ok().flatten()?
        .get_attribute("data-mix-track")
}
fn ruler_width(tracks: RwSignal<Vec<Track>>, zoom: RwSignal<f64>) -> f64 {
    let end = tracks.with(|all|all.iter()
        .flat_map(|t| t.regions.iter().map(|r| r.start + r.duration))
        .fold(30.0f64, f64::max).min(MAX_SECONDS));
    (end + 10.0) * zoom.get()
}

#[component]
fn TimelineClip(
    track_id: String, region_id: String, tracks: RwSignal<Vec<Track>>,
    assets: RwSignal<Vec<AudioAsset>>, zoom: RwSignal<f64>,
    pointer: RwSignal<Option<PointerEdit>>,
    waveforms: RwSignal<HashMap<String,Waveform>>,
) -> impl IntoView {
    let move_track = track_id.clone();
    let move_region = region_id.clone();
    let left_track = track_id.clone();
    let left_region = region_id.clone();
    let right_track = track_id.clone();
    let right_region = region_id.clone();
    let display_track = track_id.clone();
    let display_region = region_id.clone();
    let label_track = track_id.clone();
    let label_region = region_id.clone();
    let wave_track=track_id.clone();
    let wave_region=region_id.clone();
    let fade_in_track=track_id.clone();
    let fade_in_region=region_id.clone();
    let fade_out_track=track_id.clone();
    let fade_out_region=region_id.clone();
    let envelope_track=track_id.clone();
    let envelope_region=region_id.clone();
    let grip_in_track=track_id.clone();
    let grip_in_region=region_id.clone();
    let grip_out_track=track_id.clone();
    let grip_out_region=region_id.clone();
    let grip_in_start_track=track_id.clone();
    let grip_in_start_region=region_id.clone();
    let grip_out_start_track=track_id.clone();
    let grip_out_start_region=region_id.clone();
    view! {
        <div class="mix-timeline-clip"
            style=move || {
                let Some(region)=preview_region(tracks,pointer,zoom,&display_track,&display_region)
                    else {return "display:none".to_owned();};
                format!("left:{:.2}px;width:{:.2}px",
                    region.start * zoom.get(), (region.duration * zoom.get()).max(10.0))
            }
            on:pointerdown=move |event| pointer_start(
                &event, pointer, move_track.clone(), move_region.clone(), EditMode::Move)
            title="Arrastrá para mover · usá los bordes para recortar">
            <span class="mix-timeline-handle left" title="Ajustar comienzo"
                on:pointerdown=move |event| {
                    event.stop_propagation();
                    pointer_start(&event, pointer, left_track.clone(), left_region.clone(), EditMode::TrimStart);
                }></span>
            <svg class="mix-waveform" viewBox="0 0 120 24" preserveAspectRatio="none" aria-hidden="true">
                <path d=move || {
                    tracks.with(|all|all.iter().find(|t|t.id==wave_track)
                        .and_then(|t|t.regions.iter().find(|r|r.id==wave_region))
                        .and_then(|r|waveforms.with(|cached|cached.get(&r.asset_id)
                            .map(|wave|wave.path(r.source_in,r.duration))))
                        .unwrap_or_default())
                }/>
            </svg>
            <svg class="mix-fade-envelope" viewBox="0 0 120 24"
                preserveAspectRatio="none" aria-hidden="true">
                <path d=move ||preview_region(tracks,pointer,zoom,&envelope_track,&envelope_region)
                    .map(|region|envelope_path(&region)).unwrap_or_default()/>
            </svg>
            <span class="mix-fade-grip begin" title="Arrastrá para ajustar fade in"
                style=move ||preview_region(tracks,pointer,zoom,&grip_in_track,&grip_in_region)
                    .map(|r|format!("left:{:.2}%",100.0*r.fade_in/r.duration.max(0.1)))
                    .unwrap_or_default()
                on:pointerdown=move |event|{
                    event.stop_propagation();
                    pointer_start(&event,pointer,grip_in_start_track.clone(),
                        grip_in_start_region.clone(),EditMode::FadeIn);
                }></span>
            <span class="mix-fade-grip end" title="Arrastrá para ajustar fade out"
                style=move ||preview_region(tracks,pointer,zoom,&grip_out_track,&grip_out_region)
                    .map(|r|format!("left:{:.2}%",100.0*(1.0-r.fade_out/r.duration.max(0.1))))
                    .unwrap_or_default()
                on:pointerdown=move |event|{
                    event.stop_propagation();
                    pointer_start(&event,pointer,grip_out_start_track.clone(),
                        grip_out_start_region.clone(),EditMode::FadeOut);
                }></span>
            <span class="mix-fade-shade mix-fade-enter" style=move ||{
                tracks.with(|all|all.iter().find(|t|t.id==fade_in_track)
                    .and_then(|t|t.regions.iter().find(|r|r.id==fade_in_region))
                    .map(|r|format!("width:{:.2}%",100.*r.fade_in/r.duration.max(0.1)))
                    .unwrap_or_default())
            }></span>
            <span class="mix-fade-shade mix-fade-exit" style=move ||{
                tracks.with(|all|all.iter().find(|t|t.id==fade_out_track)
                    .and_then(|t|t.regions.iter().find(|r|r.id==fade_out_region))
                    .map(|r|format!("width:{:.2}%",100.*r.fade_out/r.duration.max(0.1)))
                    .unwrap_or_default())
            }></span>
            <span class="mix-timeline-clip-name">{move || {
                tracks.with(|all| all.iter().find(|t|t.id==label_track)
                    .and_then(|t|t.regions.iter().find(|r|r.id==label_region))
                    .and_then(|r| assets.with(|list|list.iter()
                        .find(|a|a.id==r.asset_id).map(|a|a.name.clone())))
                    .unwrap_or_else(||"Clip sin fuente".into()))
            }}</span>
            <span class="mix-timeline-handle right" title="Ajustar final"
                on:pointerdown=move |event| {
                    event.stop_propagation();
                    pointer_start(&event, pointer, right_track.clone(), right_region.clone(), EditMode::TrimEnd);
                }></span>
        </div>
    }
}

#[component]
pub fn MixTimeline(
    tracks: RwSignal<Vec<Track>>, assets: RwSignal<Vec<AudioAsset>>,
    status: RwSignal<String>,
) -> impl IntoView {
    let zoom = RwSignal::new(24.0f64);
    let pointer = RwSignal::new(None::<PointerEdit>);
    let waveforms = RwSignal::new(HashMap::<String,Waveform>::new());
    let waves_busy = RwSignal::new(false);
    let crossfade_curve=RwSignal::new(MixFadeCurve::EqualPower);
    let track_ids = move || tracks.get().into_iter().map(|track| track.id).collect::<Vec<String>>();
    let ruler_ticks = move || {
        let duration = ruler_width(tracks, zoom) / zoom.get();
        (0..=((duration / 10.0).ceil() as u32).min(361)).collect::<Vec<u32>>()
    };
    view! {
        <section class="mix-timeline-shell" aria-label="Línea de tiempo interactiva">
            <div class="mix-timeline-toolbar">
                <strong>"LÍNEA DE TIEMPO"</strong>
                <small>"Movimiento, recorte y fades sin alterar el origen"</small>
                <button type="button" class="mix-timeline-action"
                    title="Analiza únicamente los audios utilizados, sin subir archivos"
                    disabled=move || waves_busy.get()
                    on:click=move |_| waveform::load_for_mix(assets,tracks,waveforms,status,waves_busy)>
                    {move ||if waves_busy.get(){"Analizando…"}else{"▥ Ondas"}}
                </button>
                <select class="mix-curve-select" aria-label="Curva para crossfades"
                    title="Elegí el tipo de transición"
                    prop:value=move ||crossfade_curve.get().as_str()
                    on:change=move |event|{
                        let value=event_target_value(&event);
                        if let Some(curve)=MixFadeCurve::from_str(&value){
                            crossfade_curve.set(curve);
                        }
                    }>
                    <option value="equal_power">"Potencia constante"</option>
                    <option value="linear">"Lineal"</option>
                </select>
                <button type="button" class="mix-timeline-action"
                    title="Aplica el tipo elegido a pares superpuestos sin guardar proyecto"
                    on:click=move |_|{
                        let mut changed=(0,0);
                        let curve=crossfade_curve.get_untracked();
                        tracks.update(|list|changed=apply_crossfades(list,curve));
                        status.set(format!("Crossfades {}: {} aplicados; {} omitidos. Previsualizá con ▶ Motor (borrador).",
                            if curve==MixFadeCurve::EqualPower {"equal-power"} else {"lineales"},
                            changed.0,changed.1));
                    }>"⇄ Crossfade"</button>
                <button type="button" title="Alejar" on:click=move |_| zoom.update(|value| *value = (*value / 1.5).max(8.0))>"−"</button>
                <span>{move || format!("{:.0} px/s", zoom.get())}</span>
                <button type="button" title="Acercar" on:click=move |_| zoom.update(|value| *value = (*value * 1.5).min(72.0))>"＋"</button>
            </div>
            <div class="mix-timeline-scroll"
                on:pointermove=move |event| pointer.update(|active| {
                    if let Some(current) = active {
                        if current.pointer_id == event.pointer_id() {
                            current.delta_pixels = (event.client_x() - current.origin_x) as f64;
                        }
                    }
                })
                on:pointerup=move |event| {
                    let Some(active) = pointer.get_untracked() else { return; };
                    if active.pointer_id != event.pointer_id() { return; }
                    let delta = snap((event.client_x() - active.origin_x) as f64 / zoom.get_untracked());
                    let destination = if active.mode == EditMode::Move {
                        target_lane(&event)
                    } else { None };
                    let mut result = Ok(());
                    tracks.update(|all| {
                        result = apply_pointer_edit(all, &active, delta, destination.as_deref());
                    });
                    match result {
                        Ok(()) => status.set("Región actualizada en el borrador. Usá ▶ Motor (borrador) para escucharla.".into()),
                        Err(reason) => status.set(reason.into()),
                    }
                    pointer.set(None);
                }
                on:pointercancel=move |_| pointer.set(None)>
                <div class="mix-timeline-ruler" style=move || format!("width:{:.1}px", ruler_width(tracks,zoom)+120.0)>
                    <div class="mix-timeline-gutter">"SEG"</div>
                    <div class="mix-timeline-ticks">
                        <For each=ruler_ticks key=|second| *second children=move |step| {
                            view! {<span style=move ||format!("left:{:.1}px", f64::from(step)*10.0*zoom.get())>{format!("{}s",step*10)}</span>}
                        }/>
                    </div>
                </div>
                <For each=track_ids key=|id| id.clone() children=move |track_id| {
                    let name_id=track_id.clone();
                    let list_id=track_id.clone();
                    let clip_track_id=track_id.clone();
                    let region_ids = move || tracks.get().iter().find(|t|t.id == list_id)
                        .map(|t|t.regions.iter().map(|r|r.id.clone()).collect::<Vec<String>>())
                        .unwrap_or_default();
                    view! {
                        <div class="mix-timeline-lane" data-mix-track=track_id.clone()
                            style=move || format!("width:{:.1}px",ruler_width(tracks,zoom)+120.0)>
                            <div class="mix-timeline-gutter">{move ||
                                tracks.get().iter().find(|t| t.id == name_id)
                                    .map(|t|t.name.clone()).unwrap_or_default()
                            }</div>
                            <div class="mix-timeline-canvas" style=move ||
                                format!("background-size:{:.1}px 100%", zoom.get()*5.0)>
                                <For each=region_ids key=|id| id.clone() children=move |region_id| {
                                    view! {<TimelineClip track_id=clip_track_id.clone()
                                        region_id=region_id tracks=tracks assets=assets
                                        zoom=zoom pointer=pointer waveforms=waveforms/>}
                                }/>
                            </div>
                        </div>
                    }
                }/>
            </div>
        </section>
    }
}

//! Non-destructive, local-time gain automation for FullCanvas VSMix.
//! Points are expressed as linear gain; conversion to the native dB envelope
//! happens in studio_api::compose_mix during explicit draft render or save.
use crate::media::{MixGainPoint, MixRegion, Track};
use leptos::prelude::*;

const MAX_POINTS:usize=256;
const STEP:f64=0.001;

fn round_ms(value:f64)->f64 {(value*1000.0).round()/1000.0}

/// Mirror the native DSP interpolation in LINEAR amplitude, not in decibels.
pub fn sample(points:&[MixGainPoint],at:f64)->f64 {
    if points.is_empty(){return 1.0;}
    if points.len()==1 || at<=points[0].at {return points[0].gain;}
    for pair in points.windows(2) {
        if at<=pair[1].at {
            let fraction=((at-pair[0].at)/(pair[1].at-pair[0].at)).clamp(0.0,1.0);
            return pair[0].gain+(pair[1].gain-pair[0].gain)*fraction;
        }
    }
    points.last().map_or(1.0,|point|point.gain)
}

/// Fixed 61-segment approximation. The renderer receives original keyframes.
pub fn path(region:&MixRegion)->String {
    if region.duration<=0.0 || !region.duration.is_finite(){return String::new();}
    let mut out=String::with_capacity(900);
    for step in 0..=60 {
        let x=120.0*step as f64/60.0;
        let time=region.duration*step as f64/60.0;
        let gain=sample(&region.gain_points,time);
        let y=21.0-18.0*(gain/2.0);
        out.push_str(&format!("{} {:.2} {:.2} ",
            if step==0 {"M"}else{"L"},x,y));
    }
    out
}
pub fn adjust_point(
    region:&mut MixRegion,index:usize,at:Option<f64>,gain:Option<f64>
) {
    if index>=region.gain_points.len(){return;}
    if let Some(at)=at.filter(|v|v.is_finite()) {
        let low=if index==0 {0.0}else{round_ms(region.gain_points[index-1].at)+STEP};
        let high=if index+1==region.gain_points.len() {
            round_ms(region.duration)
        }else{round_ms(region.gain_points[index+1].at)-STEP};
        if low<=high {
            region.gain_points[index].at=round_ms(at.clamp(low,high));
        }
    }
    if let Some(gain)=gain.filter(|v|v.is_finite()){
        region.gain_points[index].gain=gain.clamp(0.0,2.0);
    }
}
pub fn add_point(region:&mut MixRegion)->bool {
    if region.gain_points.len()>=MAX_POINTS || !region.duration.is_finite()
        || region.duration<0.1 {return false;}
    let end=round_ms(region.duration);
    if region.gain_points.is_empty() {
        region.gain_points=vec![
            MixGainPoint{at:0.0,gain:1.0},
            MixGainPoint{at:round_ms(end/2.0),gain:1.0},
            MixGainPoint{at:end,gain:1.0},
        ];
        return true;
    }
    // Fill the widest gap, including first and last boundaries. This allows
    // repeated clicks without colliding with another keyframe after ms rounding.
    let mut boundaries=Vec::with_capacity(region.gain_points.len()+2);
    boundaries.push(0.0);
    boundaries.extend(region.gain_points.iter().map(|p|p.at));
    boundaries.push(end);
    let mut best=(0.0,0.0);
    for pair in boundaries.windows(2) {
        if pair[1]-pair[0]>best.1-best.0 {
            best=(pair[0],pair[1]);
        }
    }
    let at=round_ms((best.0+best.1)/2.0);
    if at<=best.0+STEP/2.0 || at>=best.1-STEP/2.0 {
        return false;
    }
    let gain=sample(&region.gain_points,at);
    region.gain_points.push(MixGainPoint{at,gain});
    region.gain_points.sort_by(|a,b|a.at.total_cmp(&b.at));
    true
}
pub fn trim_start(region:&mut MixRegion,change:f64) {
    if change>=0.0 {
        region.gain_points.retain(|point|point.at+1e-9>=change);
    }
    for point in &mut region.gain_points {
        point.at=round_ms((point.at-change).clamp(0.0,region.duration));
    }
}
pub fn trim_end(region:&mut MixRegion) {
    region.gain_points.retain(|point|point.at<=region.duration+1e-9);
}
fn point_value(tracks:RwSignal<Vec<Track>>,track_id:&str,region_id:&str,index:usize,
    read:impl FnOnce(&MixGainPoint)->String)->String {
    tracks.with(|list|list.iter().find(|t|t.id==track_id)
        .and_then(|t|t.regions.iter().find(|r|r.id==region_id))
        .and_then(|r|r.gain_points.get(index))
        .map(read).unwrap_or_default())
}
fn edit_point(tracks:RwSignal<Vec<Track>>,track_id:&str,region_id:&str,index:usize,
    at:Option<f64>,gain:Option<f64>) {
    tracks.update(|list|{
        if let Some(region)=list.iter_mut().find(|t|t.id==track_id)
            .and_then(|t|t.regions.iter_mut().find(|r|r.id==region_id)) {
            adjust_point(region,index,at,gain);
        }
    });
}
#[component]
fn GainPointRow(
    tracks:RwSignal<Vec<Track>>,track_id:String,region_id:String,index:usize
)->impl IntoView {
    let time_track=track_id.clone();let time_region=region_id.clone();
    let gain_track=track_id.clone();let gain_region=region_id.clone();
    let change_time_track=track_id.clone();let change_time_region=region_id.clone();
    let change_gain_track=track_id.clone();let change_gain_region=region_id.clone();
    let remove_track=track_id;let remove_region=region_id;
    view! {
        <div class="mix-automation-row">
            <label>"Tiempo (s)"
                <input type="number" min="0" max="3600" step="0.001"
                    aria-label=format!("Tiempo punto {}",index+1)
                    prop:value=move ||point_value(tracks,&time_track,&time_region,index,
                        |point|point.at.to_string())
                    on:change=move |ev|{
                        if let Ok(value)=event_target_value(&ev).parse::<f64>() {
                            edit_point(tracks,&change_time_track,&change_time_region,index,Some(value),None);
                        }
                    }/>
            </label>
            <label>"Ganancia (%)"
                <input type="number" min="0" max="200" step="1"
                    aria-label=format!("Ganancia punto {}",index+1)
                    prop:value=move ||point_value(tracks,&gain_track,&gain_region,index,
                        |point|(point.gain*100.).round().to_string())
                    on:change=move |ev|{
                        if let Ok(value)=event_target_value(&ev).parse::<f64>() {
                            edit_point(tracks,&change_gain_track,&change_gain_region,index,None,Some(value/100.));
                        }
                    }/>
            </label>
            <button type="button" class="text-icon" title="Eliminar punto de volumen"
                on:click=move |_|tracks.update(|list|{
                    if let Some(region)=list.iter_mut().find(|t|t.id==remove_track)
                        .and_then(|t|t.regions.iter_mut().find(|r|r.id==remove_region)) {
                        if index<region.gain_points.len(){region.gain_points.remove(index);}
                    }
                })>"×"</button>
        </div>
    }
}
#[component]
pub fn GainEditor(tracks:RwSignal<Vec<Track>>,track_id:String,region_id:String)->impl IntoView {
    let count_track=track_id.clone();let count_region=region_id.clone();
    let add_track=track_id.clone();let add_region=region_id.clone();
    let reset_track=track_id.clone();let reset_region=region_id.clone();
    let rows_track=track_id.clone();let rows_region=region_id.clone();
    view! {
        <div class="mix-automation-editor">
            <div class="mix-automation-heading">
                <strong>"VOLUMEN / AUTOMATIZACIÓN"</strong>
                <small>{move ||tracks.with(|list|list.iter().find(|t|t.id==count_track)
                    .and_then(|t|t.regions.iter().find(|r|r.id==count_region))
                    .map(|r|format!("{} puntos",r.gain_points.len()))
                    .unwrap_or_default())}</small>
                <button type="button" class="action-secondary"
                    title="Agrega puntos de volumen no destructivos"
                    on:click=move |_|tracks.update(|list|{
                        if let Some(r)=list.iter_mut().find(|t|t.id==add_track)
                            .and_then(|t|t.regions.iter_mut().find(|r|r.id==add_region)){
                            add_point(r);
                        }
                    })>"＋ Punto"</button>
                <button type="button" class="action-secondary" title="Restablecer volumen constante"
                    on:click=move |_|tracks.update(|list|{
                        if let Some(r)=list.iter_mut().find(|t|t.id==reset_track)
                            .and_then(|t|t.regions.iter_mut().find(|r|r.id==reset_region)){
                            r.gain_points.clear();
                        }
                    })>"Restablecer"</button>
            </div>
            <div class="mix-automation-points">
                <For each=move ||tracks.with(|list|list.iter().find(|t|t.id==rows_track)
                    .and_then(|t|t.regions.iter().find(|r|r.id==rows_region))
                    .map(|r|(0..r.gain_points.len()).collect::<Vec<usize>>())
                    .unwrap_or_default())
                    key=|index|*index children=move |index|{
                        view! {<GainPointRow tracks=tracks track_id=track_id.clone()
                            region_id=region_id.clone() index=index/>}
                    }/>
            </div>
        </div>
    }
}

//! Atomic multi-region operations. Every operation checks limits before
//! mutating track contents, so invalid batches cannot partially edit a mix.
use crate::media::{self,Track};
use std::collections::HashSet;

const MAX_DURATION:f64=3600.0;
const MAX_REGIONS_PER_TRACK:usize=32;
const MAX_REGIONS:usize=256;

pub fn ids(tracks:&[Track])->HashSet<String> {
    tracks.iter().flat_map(|track|track.regions.iter().map(|r|r.id.clone())).collect()
}
pub fn count_selected(tracks:&[Track],selected:&HashSet<String>)->usize {
    tracks.iter().flat_map(|t|t.regions.iter())
        .filter(|r|selected.contains(&r.id)).count()
}
/// A uniform displacement is clamped to the intersection of ALL selected
/// clips' allowed ranges; their relative positions are preserved.
pub fn nudge(tracks:&mut [Track],selected:&HashSet<String>,offset:f64)->usize {
    if !offset.is_finite(){return 0;}
    let mut lower=f64::NEG_INFINITY;
    let mut upper=f64::INFINITY;
    let mut count=0;
    for region in tracks.iter().flat_map(|t|t.regions.iter()) {
        if selected.contains(&region.id) {
            lower=lower.max(-region.start);
            upper=upper.min(MAX_DURATION-region.start-region.duration);
            count+=1;
        }
    }
    if count==0 || lower>upper {return 0;}
    let delta=offset.clamp(lower,upper);
    if delta.abs()<0.0000001{return 0;}
    for region in tracks.iter_mut().flat_map(|t|t.regions.iter_mut()) {
        if selected.contains(&region.id) {region.start+=delta;}
    }
    count
}
pub fn delete(tracks:&mut [Track],selected:&HashSet<String>)->usize {
    let mut removed=0;
    for track in tracks {
        let before=track.regions.len();
        track.regions.retain(|r|!selected.contains(&r.id));
        removed+=before-track.regions.len();
        if track.regions.is_empty(){track.asset_id.clear();}
    }
    removed
}
/// Duplicate selected clips by +250ms on their original tracks. The IDs
/// are regenerated; source files and volume automation are NOT duplicated.
pub fn duplicate(tracks:&mut [Track],selected:&HashSet<String>)
    ->Result<Vec<String>,String>
{
    let mut count=0usize;
    for track in tracks.iter() {
        let copies=track.regions.iter().filter(|r|selected.contains(&r.id)).count();
        if track.regions.len()+copies>MAX_REGIONS_PER_TRACK {
            return Err("La pista supera 32 regiones al duplicar".into());
        }
        if track.regions.iter().filter(|r|selected.contains(&r.id))
            .any(|r|r.start+r.duration+0.25>MAX_DURATION) {
            return Err("Una copia supera el límite de 3600 segundos".into());
        }
        count+=copies;
    }
    if count==0 {return Ok(Vec::new());}
    if tracks.iter().map(|t|t.regions.len()).sum::<usize>()+count>MAX_REGIONS {
        return Err("La mezcla supera 256 regiones".into());
    }
    let mut created=Vec::with_capacity(count);
    for track in tracks {
        let original=track.regions.iter().filter(|r|selected.contains(&r.id))
            .cloned().collect::<Vec<_>>();
        for mut copy in original {
            copy.id=media::uid("region");
            copy.start+=0.25;
            created.push(copy.id.clone());
            track.regions.push(copy);
        }
    }
    Ok(created)
}
/// Move a selected group while preserving source timings and project identity.
/// Destination is optional: without a target lane, keep original lanes.
/// All batch changes commit together only after every capacity check succeeds.
pub fn move_group(
    tracks:&mut Vec<Track>, selected:&HashSet<String>, offset:f64,
    destination:Option<&str>
)->Result<usize,String> {
    if !offset.is_finite(){return Err("Desplazamiento inválido".into());}
    let valid=ids(tracks);
    let chosen=selected.intersection(&valid).cloned().collect::<HashSet<_>>();
    if chosen.is_empty(){return Ok(0);}
    let dest=if let Some(name)=destination {
        Some(tracks.iter().position(|t|t.id==name).ok_or("Pista inexistente")?)
    } else {None};
    if let Some(target)=dest {
        let incoming=tracks.iter().enumerate()
            .filter(|(idx,_)|*idx!=target)
            .flat_map(|(_,t)|t.regions.iter())
            .filter(|r|chosen.contains(&r.id)).count();
        let removed=tracks[target].regions.iter()
            .filter(|r|chosen.contains(&r.id)).count();
        if tracks[target].regions.len()-removed+chosen.len()>MAX_REGIONS_PER_TRACK {
            return Err("La pista de destino supera 32 regiones".into());
        }
        // incoming is intentionally counted independently for clarity.
        if incoming+removed!=chosen.len(){return Err("Selección inválida".into());}
    }
    let moved=nudge(tracks,&chosen,offset);
    // A zero horizontal displacement can still represent a lane transfer.
    if let Some(target)=dest {
        let mut transferred=Vec::new();
        for (index,track) in tracks.iter_mut().enumerate() {
            if index==target {continue;}
            let mut left=Vec::with_capacity(track.regions.len());
            for region in track.regions.drain(..) {
                if chosen.contains(&region.id) {transferred.push(region);}
                else {left.push(region);}
            }
            track.regions=left;
            if track.regions.is_empty(){track.asset_id.clear();}
        }
        tracks[target].regions.extend(transferred);
        Ok(chosen.len())
    } else {Ok(moved)}
}


/// In-memory clipboard retains the source's local media IDs and all region
/// editing fields (gain envelopes, pan, fades, source offset). No PCM copied.
#[derive(Clone)]
pub struct MixClipboard {
    pub items:Vec<ClipboardItem>,
    pub anchor_start:f64,
    pub anchor_lane:usize,
}
#[derive(Clone)]
pub struct ClipboardItem {
    pub source_lane:usize,
    pub region:media::MixRegion,
}
pub fn copy(tracks:&[Track],selected:&HashSet<String>)->Option<MixClipboard>{
    let mut items=Vec::new();
    for (lane,track) in tracks.iter().enumerate(){
        for region in &track.regions {
            if selected.contains(&region.id) {
                items.push(ClipboardItem{source_lane:lane,region:region.clone()});
            }
        }
    }
    if items.is_empty(){return None;}
    let anchor_start=items.iter().map(|item|item.region.start)
        .min_by(|a,b|a.total_cmp(b))?;
    let anchor_lane=items.iter().map(|item|item.source_lane).min()?;
    Some(MixClipboard{items,anchor_start,anchor_lane})
}

/// Paste a clipboard group into a target track at the requested timeline
/// cursor, preserving source-lane offsets. Ripple insertion shifts complete
/// later regions in each destination lane, rejecting clips straddling cursor.
pub fn paste(
    tracks:&mut Vec<Track>,clipboard:&MixClipboard,target_lane:&str,
    at:f64,ripple:bool
)->Result<Vec<String>,String>{
    if !at.is_finite() || !(0.0..=MAX_DURATION).contains(&at){
        return Err("Posición de pegado fuera de la línea de tiempo".into());
    }
    let dest=tracks.iter().position(|t|t.id==target_lane)
        .ok_or("Seleccioná una pista de destino".to_owned())?;
    if clipboard.items.is_empty(){return Ok(Vec::new());}
    let count=tracks.iter().map(|t|t.regions.len()).sum::<usize>();
    if count+clipboard.items.len()>MAX_REGIONS{
        return Err("El pegado supera las 256 regiones de VSMix".into());
    }
    let mut incoming=vec![Vec::<media::MixRegion>::new();tracks.len()];
    for item in &clipboard.items {
        let offset=item.source_lane.checked_sub(clipboard.anchor_lane)
            .ok_or("Portapapeles de pistas inválido")?;
        let target=dest.checked_add(offset)
            .filter(|index|*index<tracks.len())
            .ok_or("No existen suficientes pistas de destino")?;
        let mut region=item.region.clone();
        region.start=at+(region.start-clipboard.anchor_start);
        if !region.start.is_finite() || !region.duration.is_finite()
            || region.start<0. || region.duration<=0.
            || region.start+region.duration>MAX_DURATION{
            return Err("Una región pegada supera el límite de 3600 segundos".into());
        }
        incoming[target].push(region);
    }
    for (index,regions) in incoming.iter().enumerate(){
        if tracks[index].regions.len()+regions.len()>MAX_REGIONS_PER_TRACK{
            return Err("El pegado supera 32 regiones en una pista".into());
        }
        if ripple && !regions.is_empty(){
            let shift=regions.iter().map(|r|r.start+r.duration)
                .fold(at,f64::max)-at;
            for old in &tracks[index].regions{
                if old.start<at && old.start+old.duration>at {
                    return Err("Ripple no puede cortar una región existente".into());
                }
                if old.start>=at && old.start+old.duration+shift>MAX_DURATION{
                    return Err("Ripple supera los 3600 segundos".into());
                }
            }
        }
    }
    // Preflight succeeded: only now mutate and generate new stable IDs.
    let mut created=Vec::with_capacity(clipboard.items.len());
    for (index,regions) in incoming.iter_mut().enumerate(){
        if regions.is_empty(){continue;}
        if ripple{
            let shift=regions.iter().map(|r|r.start+r.duration)
                .fold(at,f64::max)-at;
            for old in &mut tracks[index].regions{
                if old.start>=at{old.start+=shift;}
            }
        }
        for mut region in regions.drain(..){
            region.id=media::uid("region");
            created.push(region.id.clone());
            tracks[index].regions.push(region);
        }
    }
    Ok(created)
}

/// Remove selected regions and close their occupied time ranges. To avoid
/// silently chopping or resampling another source, a region intersecting
/// a removed interval causes a total rollback rather than a partial edit.
pub fn ripple_delete(tracks:&mut [Track],selected:&HashSet<String>)
    ->Result<usize,String>{
    let mut all_intervals=Vec::with_capacity(tracks.len());
    let mut total=0;
    for track in tracks.iter(){
        let mut intervals=track.regions.iter()
            .filter(|r|selected.contains(&r.id))
            .map(|r|(r.start,r.start+r.duration))
            .collect::<Vec<_>>();
        total+=intervals.len();
        intervals.sort_by(|a,b|a.0.total_cmp(&b.0));
        let mut joined=Vec::<(f64,f64)>::new();
        for (begin,end) in intervals{
            if !begin.is_finite() || !end.is_finite() || begin<0. || end>MAX_DURATION{
                return Err("Región ripple con tiempos inválidos".into());
            }
            if let Some(last)=joined.last_mut(){
                if begin<=last.1 {last.1=last.1.max(end);continue;}
            }
            joined.push((begin,end));
        }
        for region in track.regions.iter().filter(|r|!selected.contains(&r.id)){
            if joined.iter().any(|(from,to)|
                region.start<*to && region.start+region.duration>*from){
                return Err("Ripple se cruza con otra región; separalas antes de borrar".into());
            }
        }
        all_intervals.push(joined);
    }
    if total==0{return Ok(0);}
    for (track,intervals) in tracks.iter_mut().zip(all_intervals.iter()){
        track.regions.retain(|r|!selected.contains(&r.id));
        for region in &mut track.regions{
            let shift=intervals.iter().filter(|(_,end)|*end<=region.start)
                .map(|(begin,end)|end-begin).sum::<f64>();
            region.start-=shift;
        }
        if track.regions.is_empty(){track.asset_id.clear();}
    }
    Ok(total)
}

/// Align a selected region's start/end with the nearest unselected region
/// edge, playhead, or timeline origin inside a 9px threshold. The full group
/// receives one delta, so its relative timing is never distorted.
pub fn snap_move_delta(tracks:&[Track],selected:&HashSet<String>,cursor:f64,
    offset:f64,px_per_second:f64)->f64{
    if !offset.is_finite() || !cursor.is_finite()
        || !px_per_second.is_finite() || px_per_second<=0.{
        return offset;
    }
    let threshold=9.0/px_per_second;
    let mut edges=vec![0.0,cursor];
    let mut moving=Vec::new();
    for region in tracks.iter().flat_map(|t|t.regions.iter()){
        if selected.contains(&region.id){
            moving.push(region.start+offset);
            moving.push(region.start+region.duration+offset);
        }else{
            edges.push(region.start);
            edges.push(region.start+region.duration);
        }
    }
    let mut best=(threshold,f64::NAN);
    for from in moving{
        for target in &edges {
            let dist=(target-from).abs();
            if dist<best.0 {best=(dist,target-from);}
        }
    }
    if best.1.is_finite(){((offset+best.1)*1000.0).round()/1000.0}
    else{offset}
}

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

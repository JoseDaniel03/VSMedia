//! Native headless unit tests compile the EXACT production algorithms,
//! using minimal serializable region/track fixtures (no DOM or WAV needed).
#![allow(dead_code)]
mod media {
    use serde::{Serialize,Deserialize};
    use std::sync::atomic::{AtomicUsize,Ordering};
    #[derive(Clone,Debug,Serialize,Deserialize,PartialEq)]
    pub struct Region {pub id:String,pub asset_id:String,pub start:f64,pub duration:f64}
    #[derive(Clone,Debug,Serialize,Deserialize,PartialEq)]
    pub struct Track {pub id:String,pub asset_id:String,pub regions:Vec<Region>}
    pub type MixRegion=Region;
    pub fn uid(prefix:&str)->String {
        static SEQ:AtomicUsize=AtomicUsize::new(0);
        format!("{prefix}-{}",SEQ.fetch_add(1,Ordering::Relaxed))
    }
}
#[path="../../src/mix_history.rs"]
mod mix_history;
#[path="../../src/mix_batch.rs"]
mod mix_batch;

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;
    fn region(id:&str,start:f64)->media::Region {
        media::Region{id:id.into(),asset_id:"source-1".into(),start,duration:1.0}
    }
    fn track(id:&str,regions:Vec<media::Region>)->media::Track {
        media::Track{id:id.into(),asset_id:"source-1".into(),regions}
    }
    #[test]
    fn undo_redo_and_branching_preserve_exact_sessions() {
        let mut history=mix_history::MixHistory::default();
        let mut tracks=vec![track("a",vec![region("r",1.0)])];
        history.observe(&tracks);
        tracks[0].regions[0].start=2.0;
        history.observe(&tracks);
        assert!(history.can_undo());
        assert_eq!(history.undo().unwrap()[0].regions[0].start,1.0);
        assert!(history.can_redo());
        assert_eq!(history.redo().unwrap()[0].regions[0].start,2.0);
        history.undo().unwrap();
        tracks[0].regions[0].start=3.0;
        history.observe(&tracks);
        assert!(!history.can_redo());
        assert_eq!(history.size(),2);
    }
    #[test]
    fn history_deduplicates_and_is_bounded() {
        let mut history=mix_history::MixHistory::default();
        for n in 0..100 {
            let t=vec![track("a",vec![region("r",n as f64)])];
            history.observe(&t);
            history.observe(&t);
        }
        assert_eq!(history.size(),80);
        assert_eq!(history.undo().unwrap()[0].regions[0].start,98.0);
    }
    #[test]
    fn uniform_group_nudge_stops_at_nearest_boundary() {
        let mut tracks=vec![track("a",vec![region("r1",0.2),region("r2",5.0)])];
        let ids=HashSet::from(["r1".into(),"r2".into()]);
        assert_eq!(mix_batch::nudge(&mut tracks,&ids,-2.0),2);
        assert!((tracks[0].regions[0].start-0.0).abs()<1e-9);
        assert!((tracks[0].regions[1].start-4.8).abs()<1e-9);
    }
    #[test]
    fn duplicate_preserves_sources_and_produces_unique_ids() {
        let mut tracks=vec![track("a",vec![region("r1",3.0),region("r2",8.0)])];
        let ids=HashSet::from(["r1".into()]);
        let created=mix_batch::duplicate(&mut tracks,&ids).unwrap();
        assert_eq!(created.len(),1);
        assert_ne!(created[0],"r1");
        assert_eq!(tracks[0].regions[2].asset_id,"source-1");
        assert!((tracks[0].regions[2].start-3.25).abs()<1e-9);
        let remove=created.into_iter().collect::<HashSet<_>>();
        assert_eq!(mix_batch::delete(&mut tracks,&remove),1);
        assert_eq!(tracks[0].regions.len(),2);
    }
    #[test]
    fn lane_transfer_is_atomic_on_capacity_failure() {
        let mut tracks=vec![
            track("from",vec![region("picked",1.0)]),
            track("full",(0..32).map(|i|region(&format!("f{i}"),i as f64)).collect()),
        ];
        let before=tracks.clone();
        let selected=HashSet::from(["picked".into()]);
        assert!(mix_batch::move_group(&mut tracks,&selected,1.0,Some("full")).is_err());
        assert_eq!(tracks,before);
    }
    #[test]
    fn duplicate_rejects_full_track_without_changing_any_clip() {
        let mut tracks=vec![track("a",(0..32)
            .map(|i|region(&format!("r{i}"),i as f64)).collect())];
        let original=tracks.clone();
        let ids=HashSet::from(["r0".to_string()]);
        assert!(mix_batch::duplicate(&mut tracks,&ids).is_err());
        assert_eq!(tracks,original);
    }
    #[test]
    fn oversized_snapshot_pauses_history_until_smaller_session() {
        let mut history=mix_history::MixHistory::default();
        history.observe(&[track("a",vec![region("r",0.0)])]);
        let huge=track(&"a".repeat(2*1024*1024+100),vec![]);
        history.observe(&[huge]);
        assert!(history.oversized);
        assert_eq!(history.size(),0);
        history.observe(&[track("a",vec![region("r",0.0)])]);
        assert!(!history.oversized);
        assert_eq!(history.size(),1);
    }
    #[test]
    fn horizontal_multi_lane_group_move_preserves_track_assignments() {
        let mut tracks=vec![
            track("a",vec![region("a1",0.5)]),
            track("b",vec![region("b1",10.0)]),
        ];
        let selected=HashSet::from(["a1".into(),"b1".into()]);
        assert_eq!(mix_batch::move_group(&mut tracks,&selected,0.2,None).unwrap(),2);
        assert_eq!(tracks[0].regions[0].id,"a1");
        assert_eq!(tracks[1].regions[0].id,"b1");
        assert!((tracks[0].regions[0].start-0.7).abs()<1e-9);
        assert!((tracks[1].regions[0].start-10.2).abs()<1e-9);
    }
    #[test]
    fn lane_transfer_preserves_region_identity_and_relations() {
        let mut tracks=vec![
            track("from",vec![region("a",3.0),region("b",4.0)]),
            track("to",vec![]),
        ];
        let selected=HashSet::from(["a".into(),"b".into()]);
        assert_eq!(mix_batch::move_group(&mut tracks,&selected,0.5,Some("to")).unwrap(),2);
        assert!(tracks[0].regions.is_empty());
        assert_eq!(tracks[0].asset_id,"");
        assert_eq!(tracks[1].regions.len(),2);
        assert_eq!(tracks[1].regions[0].id,"a");
        assert_eq!(tracks[1].regions[1].id,"b");
        assert_eq!(tracks[1].regions[0].start,3.5);
        assert_eq!(tracks[1].regions[1].start,4.5);
    }
    #[test]
    fn clipboard_paste_across_lanes_preserves_offsets_and_sources(){
        let mut tracks=vec![
            track("source-a",vec![region("first",1.0)]),
            track("source-b",vec![region("second",1.5)]),
            track("dest-a",vec![]),
            track("dest-b",vec![]),
        ];
        let chosen=HashSet::from(["first".into(),"second".into()]);
        let copied=mix_batch::copy(&tracks,&chosen).unwrap();
        let made=mix_batch::paste(&mut tracks,&copied,"dest-a",5.0,false).unwrap();
        assert_eq!(made.len(),2);
        assert_ne!(made[0],"first");
        assert_ne!(made[1],"second");
        assert_eq!(tracks[2].regions[0].start,5.0);
        assert_eq!(tracks[3].regions[0].start,5.5);
        assert_eq!(tracks[2].regions[0].asset_id,"source-1");
        assert_eq!(tracks[0].regions.len(),1);
    }
    #[test]
    fn clipboard_preflight_rolls_back_when_target_has_no_space(){
        let mut tracks=vec![
            track("source",vec![region("original",1.0)]),
            track("full",(0..32).map(|i|region(&format!("r{i}"),i as f64)).collect()),
        ];
        let clipboard=mix_batch::copy(&tracks,&HashSet::from(["original".to_owned()])).unwrap();
        let before=tracks.clone();
        assert!(mix_batch::paste(&mut tracks,&clipboard,"full",10.0,false).is_err());
        assert_eq!(tracks,before);
    }
    #[test]
    fn ripple_paste_shifts_later_clips_and_rolls_back_on_collision(){
        let mut tracks=vec![
            track("source",vec![region("copy",1.0)]),
            track("dest",vec![region("later",6.0)]),
        ];
        let copied=mix_batch::copy(&tracks,&HashSet::from(["copy".to_owned()])).unwrap();
        mix_batch::paste(&mut tracks,&copied,"dest",5.0,true).unwrap();
        assert_eq!(tracks[1].regions[0].start,7.0);
        assert_eq!(tracks[1].regions[1].start,5.0);
        let before=tracks.clone();
        assert!(mix_batch::paste(&mut tracks,&copied,"dest",5.5,true).is_err());
        assert_eq!(tracks,before);
    }
    #[test]
    fn ripple_delete_closes_multiple_gaps_without_touching_source(){
        let mut tracks=vec![track("voice",vec![
            region("a",0.0),region("b",1.0),region("c",2.0),region("d",3.0)
        ])];
        let selected=HashSet::from(["b".into(),"d".into()]);
        assert_eq!(mix_batch::ripple_delete(&mut tracks,&selected).unwrap(),2);
        assert_eq!(tracks[0].regions.len(),2);
        assert_eq!(tracks[0].regions[0].start,0.0);
        assert_eq!(tracks[0].regions[1].start,1.0);
    }
    #[test]
    fn ripple_delete_rejects_overlapping_untouched_regions_atomically(){
        let mut tracks=vec![track("voice",vec![
            region("a",0.0),region("overlay",0.5),region("later",2.0)
        ])];
        let before=tracks.clone();
        assert!(mix_batch::ripple_delete(&mut tracks,&HashSet::from(["a".into()])).is_err());
        assert_eq!(tracks,before);
    }
    #[test]
    fn snapping_uses_playhead_and_other_region_edges_not_self(){
        let tracks=vec![track("voice",vec![
            region("moving",1.0),region("target",4.0)
        ])];
        let selected=HashSet::from(["moving".into()]);
        let to_clip=mix_batch::snap_move_delta(&tracks,&selected,0.0,1.96,24.0);
        assert!((to_clip-2.0).abs()<1e-9); // moved clip end aligns with 4
        let to_cursor=mix_batch::snap_move_delta(&tracks,&selected,6.0,5.08,24.0);
        assert!((to_cursor-5.0).abs()<1e-9); // clip start aligns with cursor
    }

}

//! Bounded undo/redo for the *full* local multitrack mix. Tracks, regions,
//! fades, gain automation and ordering share one transaction history.
//! Only editing state is captured: PCM, Blob URLs and CAS sources are excluded.
use crate::media::Track;

const MAX_STATES:usize=80;
const MAX_HISTORY_BYTES:usize=12*1024*1024;
const MAX_SNAPSHOT_BYTES:usize=2*1024*1024;

#[derive(Clone)]
struct Snapshot {
    tracks: Vec<Track>,
    fingerprint: String,
}
#[derive(Default,Clone)]
pub struct MixHistory {
    states:Vec<Snapshot>,
    cursor:usize,
    pub oversized:bool,
}
impl MixHistory {
    pub fn observe(&mut self,tracks:Vec<Track>) {
        let Ok(fingerprint)=serde_json::to_string(&tracks) else {return;};
        if fingerprint.len()>MAX_SNAPSHOT_BYTES {
            self.oversized=true;
            // Do not retain a misleading older undo history when a document
            // grows beyond the safe per-snapshot cap.
            self.states.clear();
            self.cursor=0;
            return;
        }
        if self.states.get(self.cursor)
            .is_some_and(|current|current.fingerprint==fingerprint) {
            return;
        }
        if self.states.is_empty() {
            self.states.push(Snapshot{tracks,fingerprint});
            self.cursor=0;
            self.oversized=false;
            return;
        }
        self.states.truncate(self.cursor+1);
        self.states.push(Snapshot{tracks,fingerprint});
        let mut bytes=self.states.iter().map(|state|state.fingerprint.len()).sum::<usize>();
        while self.states.len()>1 && (self.states.len()>MAX_STATES ||
                bytes>MAX_HISTORY_BYTES) {
            let removed=self.states.remove(0);
            bytes-=removed.fingerprint.len();
        }
        self.cursor=self.states.len()-1;
        self.oversized=false;
    }
    pub fn undo(&mut self)->Option<Vec<Track>> {
        if self.cursor==0 || self.states.is_empty(){return None;}
        self.cursor-=1;
        Some(self.states[self.cursor].tracks.clone())
    }
    pub fn redo(&mut self)->Option<Vec<Track>> {
        if self.cursor+1>=self.states.len(){return None;}
        self.cursor+=1;
        Some(self.states[self.cursor].tracks.clone())
    }
    pub fn can_undo(&self)->bool {self.cursor>0}
    pub fn can_redo(&self)->bool {self.cursor+1<self.states.len()}
    pub fn size(&self)->usize {self.states.len()}
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn empty_history_does_not_undo() {
        let mut h=MixHistory::default();
        assert!(h.undo().is_none());
        assert!(h.redo().is_none());
        h.observe(vec![]);
        assert!(!h.can_undo());
        assert!(!h.can_redo());
    }
}

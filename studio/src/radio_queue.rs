//! Bounded, predictable radio playlist navigation; identical logic in WASM
//! and in native regression tests. Audio bytes are never stored in playlists.
use std::collections::HashSet;
pub const MAX_PLAYLISTS:usize=32;
pub const MAX_ITEMS:usize=512;
pub const MAX_NAME_CHARS:usize=80;
pub fn next_playable(items:&[String],available:&HashSet<String>,
    current:usize,wrap:bool)->Option<usize>{
    if items.is_empty(){return None;}
    // No implicit wrap when finishing a playlist.
    if current+1<items.len(){
        if let Some(index)=((current+1)..items.len())
            .find(|&i|available.contains(&items[i])){return Some(index);}
    }
    if wrap {
        return (0..=current.min(items.len()-1))
            .find(|&i|available.contains(&items[i]));
    }
    None
}
pub fn first_playable(items:&[String],available:&HashSet<String>)->Option<usize>{
    items.iter().position(|id|available.contains(id))
}
pub fn previous_playable(items:&[String],available:&HashSet<String>,
    current:usize)->Option<usize>{
    (0..current.min(items.len())).rev()
        .find(|&i|available.contains(&items[i]))
}
pub fn valid_name(name:&str)->bool{
    let n=name.trim();
    !n.is_empty() && n.chars().count()<=MAX_NAME_CHARS && !n.chars().any(char::is_control)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn skips_missing_media_without_looping_forever(){
        let ids=vec!["lost".into(),"good".into(),"gone".into()];
        let available=HashSet::from(["good".into()]);
        assert_eq!(first_playable(&ids,&available),Some(1));
        assert_eq!(next_playable(&ids,&available,1,false),None);
        assert_eq!(next_playable(&ids,&available,1,true),Some(1));
        assert_eq!(next_playable(&ids,&available,0,false),Some(1));
        assert_eq!(previous_playable(&ids,&available,2),Some(1));
        assert_eq!(first_playable(&ids,&HashSet::new()),None);
    }
    #[test]
    fn playlist_names_are_bounded_and_safe(){
        assert!(valid_name("Radio Virgoland"));
        assert!(!valid_name("   "));
        assert!(!valid_name("name\u{0000}"));
        assert!(!valid_name(&"a".repeat(MAX_NAME_CHARS+1)));
    }
}

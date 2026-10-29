//! VSMedia native, editable project manifests.
//!
//! A native format is a versioned JSON document referencing immutable assets
//! by namespace/key and SHA-256. It is NOT a new audio/image codec, and binary
//! media is deliberately not embedded in the manifest.
use std::collections::{BTreeMap, BTreeSet};
use serde::{Deserialize, Serialize};
use vsmedia_api::{AssetKey, AssetKind, AssetRecord, AssetRepository, MediaError, MediaResult};
use vsmedia_audio::AudioProject;

pub const MAGIC: &str = "VSMEDIA";
pub const SCHEMA_VERSION: u32 = 1;
pub const MAX_DOCUMENT_BYTES: usize = 2 * 1024 * 1024;
const MAX_ITEMS: usize = 4096;
const MAX_NODES: usize = 1024;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NativeKind { Audio, Image, Stream, Pack, Project }
impl NativeKind {
    pub const fn extension(self) -> &'static str {
        match self {
            Self::Audio => "vsaudio",
            Self::Image => "vsimage",
            Self::Stream => "vsstream",
            Self::Pack => "vspack",
            Self::Project => "vsproject",
        }
    }
    pub fn from_extension(ext: &str) -> Option<Self> {
        match ext.trim_start_matches('.').to_ascii_lowercase().as_str() {
            "vsaudio" => Some(Self::Audio),
            "vsimage" => Some(Self::Image),
            "vsstream" => Some(Self::Stream),
            "vspack" => Some(Self::Pack),
            "vsproject" => Some(Self::Project),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MediaCodec {
    Wav, Flac, Mp3, OggVorbis, Opus, Webm,
    Png, Jpeg, Webp, Apng, Gif,
}
impl MediaCodec {
    fn audio(self) -> bool {
        matches!(self, Self::Wav | Self::Flac | Self::Mp3 | Self::OggVorbis | Self::Opus | Self::Webm)
    }
    fn image(self) -> bool {
        matches!(self, Self::Png | Self::Jpeg | Self::Webp | Self::Apng | Self::Gif | Self::Webm)
    }
}
/// A manifest reference pins a blob revision by its actual content SHA-256.
/// Same physical asset may be used by multiple VSMedia projects without copying.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AssetRef {
    pub key: AssetKey,
    pub sha256: String,
    pub codec: MediaCodec,
}
impl AssetRef {
    fn validate(&self) -> MediaResult<()> {
        self.key.validated()?;
        validate_hash(&self.sha256)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VSAudio {
    /// Existing engine timeline format; edits are non-destructive.
    pub timeline: AudioProject,
    pub sources: Vec<AssetRef>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ImageLayer {
    pub id: String,
    pub source: AssetRef,
    pub visible: bool,
    pub opacity: f32,
    pub x: i32,
    pub y: i32,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VSImage {
    pub name: String,
    pub canvas_width: u32,
    pub canvas_height: u32,
    pub layers: Vec<ImageLayer>,
    /// Animation playback rate; absent for a still composition.
    pub fps: Option<u16>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VSStream {
    pub name: String,
    pub channel: String,
    /// Music/radio playlist assets; URLs/credentials are never embedded.
    pub playlist: Vec<AssetRef>,
    pub loop_playlist: bool,
    pub spatial: bool,
    pub buffer_ms: u32,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PackEntry {
    pub source: AssetRef,
    /// Root-relative Resource Pack path; validated through vsmedia-pack.
    pub target: String,
    pub priority: i32,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VSPack {
    pub name: String,
    pub entries: Vec<PackEntry>,
}
/// References to other native-format documents, including nested projects.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DocumentLink {
    pub key: AssetKey,
    pub kind: NativeKind,
    pub sha256: String,
}
impl DocumentLink {
    fn validate(&self) -> MediaResult<()> {
        self.key.validated()?;
        validate_hash(&self.sha256)
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProjectNode {
    pub id: String,
    pub document: Option<DocumentLink>,
    /// IDs of other local project nodes on which this node depends.
    pub depends_on: Vec<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VSProject {
    pub name: String,
    pub nodes: Vec<ProjectNode>,
    pub entry_points: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", content = "data", rename_all = "snake_case")]
pub enum NativePayload {
    Audio(VSAudio), Image(VSImage), Stream(VSStream),
    Pack(VSPack), Project(VSProject),
}
impl NativePayload {
    pub const fn kind(&self) -> NativeKind {
        match self {
            Self::Audio(_) => NativeKind::Audio, Self::Image(_) => NativeKind::Image,
            Self::Stream(_) => NativeKind::Stream, Self::Pack(_) => NativeKind::Pack,
            Self::Project(_) => NativeKind::Project,
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NativeDocument {
    pub magic: String,
    pub version: u32,
    pub payload: NativePayload,
}
impl NativeDocument {
    pub fn new(payload: NativePayload) -> Self {
        Self { magic: MAGIC.into(), version: SCHEMA_VERSION, payload }
    }
    pub fn kind(&self) -> NativeKind { self.payload.kind() }
    pub fn validate(&self) -> MediaResult<()> {
        if self.magic != MAGIC || self.version != SCHEMA_VERSION {
            return Err(invalid("unsupported native format magic or version"));
        }
        match &self.payload {
            NativePayload::Audio(audio) => {
                audio.timeline.validate()?;
                if audio.sources.len() > MAX_ITEMS { return Err(invalid("too many audio sources")); }
                let mut resources = BTreeSet::new();
                for source in &audio.sources {
                    source.validate()?;
                    if !source.codec.audio() || !resources.insert(&source.key) {
                        return Err(invalid("invalid or duplicate audio source"));
                    }
                }
                for track in &audio.timeline.tracks {
                    for clip in &track.clips {
                        if !resources.contains(&clip.asset) {
                            return Err(invalid("timeline clip lacks a pinned source"));
                        }
                    }
                }
            }
            NativePayload::Image(image) => {
                validate_name(&image.name)?;
                if image.canvas_width == 0 || image.canvas_width > 4096 ||
                    image.canvas_height == 0 || image.canvas_height > 4096 ||
                    image.layers.len() > MAX_NODES ||
                    image.fps.is_some_and(|fps| fps == 0 || fps > 60) {
                    return Err(invalid("invalid image dimensions/layer count/fps"));
                }
                let mut ids = BTreeSet::new();
                for layer in &image.layers {
                    validate_id(&layer.id)?;
                    layer.source.validate()?;
                    if !ids.insert(&layer.id) || !layer.source.codec.image() ||
                        !layer.opacity.is_finite() || !(0.0..=1.0).contains(&layer.opacity) {
                        return Err(invalid("invalid image layer"));
                    }
                }
            }
            NativePayload::Stream(stream) => {
                validate_name(&stream.name)?;
                validate_id(&stream.channel)?;
                if stream.playlist.len() > MAX_ITEMS ||
                    !(40..=60_000).contains(&stream.buffer_ms) {
                    return Err(invalid("invalid stream configuration"));
                }
                for source in &stream.playlist {
                    source.validate()?;
                    if !source.codec.audio() { return Err(invalid("stream must reference audio")); }
                }
            }
            NativePayload::Pack(pack) => {
                validate_name(&pack.name)?;
                if pack.entries.len() > MAX_ITEMS { return Err(invalid("too many pack entries")); }
                for entry in &pack.entries {
                    entry.source.validate()?;
                    vsmedia_pack::validate_path(&entry.target)?;
                }
            }
            NativePayload::Project(project) => validate_project(project)?,
        }
        Ok(())
    }
}
pub fn encode(document: &NativeDocument) -> MediaResult<Vec<u8>> {
    document.validate()?;
    let json = serde_json::to_vec(document)
        .map_err(|e| invalid(&format!("document serialization: {e}")))?;
    if json.len() > MAX_DOCUMENT_BYTES { return Err(invalid("native document too large")); }
    Ok(json)
}
pub fn decode(bytes: &[u8], extension: &str) -> MediaResult<NativeDocument> {
    if bytes.is_empty() || bytes.len() > MAX_DOCUMENT_BYTES {
        return Err(invalid("native document exceeds size limit"));
    }
    let expected = NativeKind::from_extension(extension)
        .ok_or_else(|| invalid("unknown native document extension"))?;
    let document: NativeDocument = serde_json::from_slice(bytes)
        .map_err(|e| invalid(&format!("native document JSON: {e}")))?;
    document.validate()?;
    if document.kind() != expected {
        return Err(invalid("document type differs from filename extension"));
    }
    Ok(document)
}
/// Store in the existing Core, never in a parallel formats-specific database.
pub fn publish<R: AssetRepository>(
    repository: &R, key: AssetKey, document: &NativeDocument,
) -> MediaResult<AssetRecord> {
    key.validated()?;
    let bytes = encode(document)?;
    repository.publish(key, AssetKind::Other, &bytes)
}
pub fn load<R: AssetRepository>(
    repository: &R, key: &AssetKey, extension: &str,
) -> MediaResult<NativeDocument> {
    let record = repository.metadata(key)?;
    if record.kind != AssetKind::Other {
        return Err(invalid("asset type is not a native document"));
    }
    decode(&repository.read(key)?, extension)
}
/// Explicitly pin links to a particular catalog revision; fail if they drift.
pub fn verify_links<R: AssetRepository>(
    repository: &R, project: &VSProject,
) -> MediaResult<()> {
    validate_project(project)?;
    for node in &project.nodes {
        if let Some(link) = &node.document {
            let record = repository.metadata(&link.key)?;
            if record.kind != AssetKind::Other || record.sha256 != link.sha256 {
                return Err(MediaError::Conflict(format!("stale project document: {}", link.key.qualified())));
            }
            let contents = repository.read(&link.key)?;
            decode(&contents, link.kind.extension())?;
        }
    }
    Ok(())
}
fn validate_project(project: &VSProject) -> MediaResult<()> {
    validate_name(&project.name)?;
    if project.nodes.len() > MAX_NODES || project.entry_points.len() > MAX_NODES {
        return Err(invalid("too many project nodes"));
    }
    let mut graph: BTreeMap<&str, &ProjectNode> = BTreeMap::new();
    for node in &project.nodes {
        validate_id(&node.id)?;
        if node.depends_on.len() > MAX_NODES || graph.insert(&node.id, node).is_some() {
            return Err(invalid("duplicated project nodes or dependencies exceed limit"));
        }
        if let Some(link) = &node.document { link.validate()?; }
    }
    for entry in &project.entry_points {
        if !graph.contains_key(entry.as_str()) { return Err(invalid("unknown project entry point")); }
    }
    let mut color: BTreeMap<&str, u8> = BTreeMap::new();
    for id in graph.keys() { visit(id, &graph, &mut color)?; }
    Ok(())
}
fn visit<'a>(
    id: &'a str, graph: &BTreeMap<&'a str, &'a ProjectNode>,
    state: &mut BTreeMap<&'a str, u8>,
) -> MediaResult<()> {
    match state.get(id) {
        Some(1) => return Err(invalid("cyclic project dependencies")),
        Some(2) => return Ok(()),
        _ => {}
    }
    let node = graph.get(id).ok_or_else(|| invalid("missing project dependency"))?;
    state.insert(id, 1);
    let mut deps = BTreeSet::new();
    for dep in &node.depends_on {
        if !deps.insert(dep) { return Err(invalid("duplicate project edge")); }
        visit(dep, graph, state)?;
    }
    state.insert(id, 2);
    Ok(())
}
fn validate_hash(hash: &str) -> MediaResult<()> {
    if hash.len() != 64 || !hash.bytes().all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c)) {
        return Err(invalid("expected lowercase SHA-256 hex"));
    }
    Ok(())
}
fn validate_name(name: &str) -> MediaResult<()> {
    if name.trim().is_empty() || name.len() > 160 ||
        name.chars().any(|c| c.is_control()) {
        return Err(invalid("invalid document name"));
    }
    Ok(())
}
fn validate_id(id: &str) -> MediaResult<()> {
    if id.is_empty() || id.len() > 64 || !id.bytes().all(|c|
        c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'_' || c == b'-'
    ) { return Err(invalid("invalid project identifier")); }
    Ok(())
}
fn invalid(value: &str) -> MediaError { MediaError::InvalidInput(value.into()) }

#[cfg(test)]
mod tests {
    use super::*;

    fn source(name: &str, codec: MediaCodec) -> AssetRef {
        AssetRef { key: AssetKey::new("demo",name).unwrap(),
            sha256: "a".repeat(64), codec }
    }
    fn project() -> VSProject {
        VSProject {
            name: "Virgoland Cinematics".into(),
            nodes: vec![
                ProjectNode { id:"intro".into(),
                    document:Some(DocumentLink {
                        key:AssetKey::new("demo","intro-audio").unwrap(),
                        kind: NativeKind::Audio, sha256:"b".repeat(64)
                    }), depends_on:vec![] },
                ProjectNode { id:"screen".into(),
                    document:Some(DocumentLink {
                        key:AssetKey::new("demo","screen-image").unwrap(),
                        kind:NativeKind::Image,sha256:"c".repeat(64)
                    }), depends_on:vec!["intro".into()] },
            ], entry_points:vec!["screen".into()],
        }
    }
    #[test]
    fn all_five_formats_round_trip_and_reject_wrong_extensions() {
        let samples = vec![
            NativePayload::Audio(VSAudio {
                timeline:AudioProject { name:"intro".into(), tracks:vec![] }, sources:vec![]
            }),
            NativePayload::Image(VSImage {
                name:"Billboard".into(), canvas_width:128,canvas_height:256,
                layers:vec![ImageLayer { id:"layer_0".into(),source:source("banner",MediaCodec::Webp),
                    visible:true,opacity:0.8,x:0,y:0 }], fps:None
            }),
            NativePayload::Stream(VSStream {
                name:"Radio".into(),channel:"radio".into(),
                playlist:vec![source("station", MediaCodec::Webm)],
                loop_playlist:true,spatial:false,buffer_ms:250
            }),
            NativePayload::Pack(VSPack {
                name:"Global Pack".into(),entries:vec![PackEntry {
                    source:source("texture", MediaCodec::Png),
                    target:"assets/demo/textures/texture.png".into(),priority:1
                }]
            }),
            NativePayload::Project(project()),
        ];
        for payload in samples {
            let doc = NativeDocument::new(payload);
            let bytes = encode(&doc).unwrap();
            assert_eq!(decode(&bytes,doc.kind().extension()).unwrap().kind(),doc.kind());
            assert!(decode(&bytes,"wrong").is_err());
        }
    }
    #[test]
    fn project_cycles_missing_links_and_duplicates_are_rejected() {
        let mut p = project();
        p.nodes[0].depends_on.push("screen".into());
        assert!(NativeDocument::new(NativePayload::Project(p)).validate().is_err());
        let mut p = project();
        p.nodes[1].depends_on = vec!["missing".into()];
        assert!(NativeDocument::new(NativePayload::Project(p)).validate().is_err());
        let mut p = project();
        p.nodes[1].id = "intro".into();
        assert!(NativeDocument::new(NativePayload::Project(p)).validate().is_err());
    }
    #[test]
    fn media_refs_must_be_pinned_and_correctly_typed() {
        let mut bad=source("image",MediaCodec::Webp);
        bad.sha256="not-a-hash".into();
        let doc=NativeDocument::new(NativePayload::Image(VSImage {
            name:"bad".into(),canvas_width:128,canvas_height:128,
            layers:vec![ImageLayer {id:"layer".into(),source:bad,visible:true,
                opacity:1.0,x:0,y:0}],fps:None
        }));
        assert!(doc.validate().is_err());
    }
    #[test]
    fn rejects_future_version_and_type_confusion() {
        let mut doc=NativeDocument::new(NativePayload::Project(project()));
        doc.version=2;
        assert!(encode(&doc).is_err());
        doc.version=SCHEMA_VERSION;
        let bytes=encode(&doc).unwrap();
        assert!(decode(&bytes,".vsaudio").is_err());
    }
}

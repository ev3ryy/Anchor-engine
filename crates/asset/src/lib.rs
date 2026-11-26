mod convert;
mod cook;
mod document;
mod pak;
mod resolve;

use std::collections::BTreeMap;
use std::path::Path;

pub use convert::{document_to_scene, scene_to_document};
pub use cook::cook;
pub use document::{Document, Node};
pub use pak::{Entry, Kind, Pak, PakFile};
use scene::AssetId;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Error {
    Parse(String),
    Guid(String),
    UnknownAsset(String),
    Cycle(String),
    DuplicateId(u32),
    MissingParent(u32),
    MissingId(u32),
    NodeCycle(u32),
    Rotation(u32),
    PrefabRoot(String),
    PrefabChildren(u32),
    Cook(scene::cooked::Error),
    Pak(&'static str),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Parse(message) => write!(f, "parse asset: {message}"),
            Self::Guid(message) => write!(f, "asset guid: {message}"),
            Self::UnknownAsset(path) => write!(f, "unknown asset {path}"),
            Self::Cycle(path) => write!(f, "prefab cycle at {path}"),
            Self::DuplicateId(id) => write!(f, "duplicate node {id}"),
            Self::MissingParent(id) => write!(f, "missing parent {id}"),
            Self::MissingId(id) => write!(f, "missing override target {id}"),
            Self::NodeCycle(id) => write!(f, "parent cycle at node {id}"),
            Self::Rotation(id) => write!(f, "node {id} rotation is not normalized"),
            Self::PrefabRoot(path) => write!(f, "prefab {path} must have one root"),
            Self::PrefabChildren(id) => write!(f, "prefab instance {id} has children"),
            Self::Cook(err) => write!(f, "{err}"),
            Self::Pak(message) => write!(f, "pak: {message}"),
        }
    }
}

impl std::error::Error for Error {}

impl From<scene::cooked::Error> for Error {
    fn from(value: scene::cooked::Error) -> Self {
        Self::Cook(value)
    }
}

#[derive(Default)]
pub struct Assets {
    scenes: BTreeMap<String, Document>,
    ids: BTreeMap<String, AssetId>,
    paths: BTreeMap<AssetId, String>,
}

impl Assets {
    pub fn insert_scene(
        &mut self,
        path: impl Into<String>,
        document: Document,
    ) -> Result<(), Error> {
        let path = path.into();
        let id = self.bind(&path, &document.guid)?;
        self.ids.insert(path.clone(), id);
        self.scenes.insert(path, document);
        Ok(())
    }

    pub fn insert_material(&mut self, path: impl Into<String>, guid: &str) -> Result<(), Error> {
        let path = path.into();
        let id = self.bind(&path, guid)?;
        self.ids.insert(path, id);
        Ok(())
    }

    pub fn scene(&self, path: &str) -> Option<&Document> {
        self.scenes.get(path)
    }

    pub fn material(&self, reference: &str) -> Result<AssetId, Error> {
        if let Some(id) = AssetId::parse(reference) {
            return Ok(id);
        }
        self.ids
            .get(reference)
            .copied()
            .ok_or_else(|| Error::UnknownAsset(reference.to_string()))
    }

    pub fn path(&self, id: AssetId) -> Option<String> {
        self.paths.get(&id).cloned()
    }

    fn bind(&mut self, path: &str, guid: &str) -> Result<AssetId, Error> {
        let id = AssetId::parse(guid).ok_or_else(|| Error::Guid(guid.to_string()))?;
        self.paths.insert(id, path.to_string());
        Ok(id)
    }
}

pub fn parse_document(text: &str) -> Result<Document, Error> {
    document::parse(text).map_err(|err| Error::Parse(err.to_string()))
}

pub fn format_document(document: &Document) -> Result<String, Error> {
    document::format(document).map_err(|err| Error::Parse(err.to_string()))
}

pub fn write_pak(entries: &[Entry<'_>]) -> Result<scene::cooked::AlignedBytes, Error> {
    pak::write(entries)
}

pub fn open_pak(path: &Path) -> Result<PakFile, Error> {
    PakFile::open(path)
}

#[cfg(test)]
mod tests;

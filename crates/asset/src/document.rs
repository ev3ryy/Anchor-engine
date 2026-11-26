use serde::{Deserialize, Serialize};

use scene::Primitive;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Document {
    pub guid: String,
    pub nodes: Vec<Node>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Node {
    pub id: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    pub transform: TransformValue,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mesh: Option<MeshValue>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub camera: Option<CameraValue>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub light: Option<LightValue>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prefab: Option<PrefabInstance>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TransformValue {
    pub translation: [f32; 3],
    pub rotation: [f32; 4],
    pub scale: [f32; 3],
}

impl Default for TransformValue {
    fn default() -> Self {
        Self {
            translation: [0.0, 0.0, 0.0],
            rotation: [0.0, 0.0, 0.0, 1.0],
            scale: [1.0, 1.0, 1.0],
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PrimitiveValue {
    #[default]
    Cube,
    Sphere,
    Cylinder,
    Capsule,
    Plane,
    Cone,
    Wedge,
}

impl PrimitiveValue {
    pub fn to_primitive(self) -> Primitive {
        match self {
            Self::Cube => Primitive::Cube,
            Self::Sphere => Primitive::Sphere,
            Self::Cylinder => Primitive::Cylinder,
            Self::Capsule => Primitive::Capsule,
            Self::Plane => Primitive::Plane,
            Self::Cone => Primitive::Cone,
            Self::Wedge => Primitive::Wedge,
        }
    }

    pub fn from_primitive(primitive: Primitive) -> Self {
        match primitive {
            Primitive::Cube => Self::Cube,
            Primitive::Sphere => Self::Sphere,
            Primitive::Cylinder => Self::Cylinder,
            Primitive::Capsule => Self::Capsule,
            Primitive::Plane => Self::Plane,
            Primitive::Cone => Self::Cone,
            Primitive::Wedge => Self::Wedge,
        }
    }
}

fn is_cube(primitive: &PrimitiveValue) -> bool {
    *primitive == PrimitiveValue::Cube
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MeshValue {
    #[serde(default, skip_serializing_if = "is_cube")]
    pub primitive: PrimitiveValue,
    pub material: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CameraValue {
    pub fov_y: f32,
    pub near: f32,
    pub far: f32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct LightValue {
    pub color: [f32; 3],
    pub intensity: f32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PrefabInstance {
    pub source: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub overrides: Vec<Override>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Override {
    pub id: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub transform: Option<TransformValue>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mesh: Option<MeshValue>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub camera: Option<CameraValue>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub light: Option<LightValue>,
}

pub fn parse(text: &str) -> Result<Document, ron::error::SpannedError> {
    ron::from_str(text)
}

pub fn format(document: &Document) -> Result<String, ron::Error> {
    ron::ser::to_string_pretty(document, ron::ser::PrettyConfig::default())
}

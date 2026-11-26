use std::io::Write;

use scene::cooked::CookedScene;
use scene::{AssetId, Camera, Light, MeshRenderer, Model, Name, Primitive, Quat, Scene, Vec3};

use crate::cook;
use crate::document::{MeshValue, Node, Override, PrefabInstance, PrimitiveValue, TransformValue};
use crate::pak::Pak;
use crate::{
    Assets, Document, Entry, Error, Kind, document_to_scene, format_document, parse_document,
    scene_to_document, write_pak,
};

const SCENE_GUID: &str = "11111111-1111-4111-8111-111111111111";
const MATERIAL_GUID: &str = "22222222-2222-4222-8222-222222222222";
const PREFAB_GUID: &str = "33333333-3333-4333-8333-333333333333";

fn material() -> AssetId {
    AssetId::parse(MATERIAL_GUID).unwrap()
}

fn transform(translation: [f32; 3]) -> TransformValue {
    TransformValue {
        translation,
        ..TransformValue::default()
    }
}

fn plain(id: u32, parent: Option<u32>, name: &str, translation: [f32; 3]) -> Node {
    Node {
        id,
        parent,
        name: Some(name.to_string()),
        transform: transform(translation),
        mesh: None,
        camera: None,
        light: None,
        prefab: None,
    }
}

fn assets_with_material() -> Assets {
    let mut assets = Assets::default();
    assets
        .insert_material("materials/checker.mat", MATERIAL_GUID)
        .unwrap();
    assets
}

#[test]
fn document_scene_roundtrip_keeps_material() {
    let mut scene = Scene::new();
    let root = scene.spawn();
    scene.insert(root, Name::new("Root")).unwrap();
    scene
        .insert(
            root,
            Light::Directional {
                color: Vec3::ONE,
                intensity: 3.0,
            },
        )
        .unwrap();
    let child = scene.spawn_child(root).unwrap();
    scene.insert(child, Name::new("Child")).unwrap();
    scene.set_local_position(child, Vec3::X).unwrap();
    scene
        .set_local_rotation(child, Quat::from_rotation_y(0.25))
        .unwrap();
    scene
        .insert(
            child,
            MeshRenderer {
                model: Model::Primitive(Primitive::Cube),
                material: material(),
            },
        )
        .unwrap();
    scene
        .insert(
            child,
            Camera {
                fov_y: 1.2,
                near: 0.1,
                far: 50.0,
            },
        )
        .unwrap();
    scene.update();

    let assets = assets_with_material();
    let document = scene_to_document(&scene, SCENE_GUID, &assets).unwrap();
    let child_node = document
        .nodes
        .iter()
        .find(|node| node.name.as_deref() == Some("Child"))
        .unwrap();
    assert_eq!(
        child_node.mesh.as_ref().unwrap().material,
        "materials/checker.mat"
    );
    let text = format_document(&document).unwrap();
    assert_eq!(parse_document(&text).unwrap(), document);

    let loaded = document_to_scene(&document, &assets).unwrap();
    assert_same(&scene, &loaded);
}

#[test]
fn omitted_primitive_is_a_cube_and_named_primitives_roundtrip() {
    let omitted: MeshValue = ron::from_str("(material: \"mat\")").unwrap();
    assert_eq!(omitted.primitive, PrimitiveValue::Cube);
    let named: MeshValue = ron::from_str("(primitive: wedge, material: \"mat\")").unwrap();
    assert_eq!(named.primitive, PrimitiveValue::Wedge);

    let mut scene = Scene::new();
    let sphere = scene.spawn();
    scene.insert(sphere, Name::new("Sphere")).unwrap();
    scene
        .insert(
            sphere,
            MeshRenderer {
                model: Model::Primitive(Primitive::Sphere),
                material: material(),
            },
        )
        .unwrap();
    let wedge = scene.spawn_child(sphere).unwrap();
    scene.insert(wedge, Name::new("Wedge")).unwrap();
    scene
        .insert(
            wedge,
            MeshRenderer {
                model: Model::Primitive(Primitive::Wedge),
                material: material(),
            },
        )
        .unwrap();
    scene.update();
    let assets = assets_with_material();
    let document = scene_to_document(&scene, SCENE_GUID, &assets).unwrap();
    let text = format_document(&document).unwrap();
    assert!(text.contains("sphere"));
    assert!(text.contains("wedge"));
    let loaded = document_to_scene(&parse_document(&text).unwrap(), &assets).unwrap();
    assert_same(&scene, &loaded);
    let cooked = cook::cook(&document, &assets, Some("shapes.scene")).unwrap();
    let instantiated = CookedScene::validate(cooked.as_slice())
        .unwrap()
        .instantiate();
    assert_same(&scene, &instantiated);
}

#[test]
fn missing_path_roundtrips_as_guid() {
    let mut scene = Scene::new();
    let root = scene.spawn();
    scene
        .insert(
            root,
            MeshRenderer {
                model: Model::Primitive(Primitive::Cube),
                material: material(),
            },
        )
        .unwrap();
    scene.update();
    let assets = Assets::default();
    let document = scene_to_document(&scene, SCENE_GUID, &assets).unwrap();
    assert_eq!(
        document.nodes[0].mesh.as_ref().unwrap().material,
        material().canonical()
    );
    let loaded = document_to_scene(&document, &assets).unwrap();
    let entity = loaded.roots().next().unwrap();
    assert_eq!(
        loaded
            .get::<MeshRenderer>(entity)
            .unwrap()
            .unwrap()
            .material,
        material()
    );
}

#[test]
fn prefab_override_replaces_nested_translation() {
    let mut assets = assets_with_material();
    assets
        .insert_scene(
            "crate.prefab",
            Document {
                guid: PREFAB_GUID.to_string(),
                nodes: vec![
                    Node {
                        mesh: Some(MeshValue {
                            primitive: PrimitiveValue::Cube,
                            material: "materials/checker.mat".to_string(),
                        }),
                        ..plain(1, None, "Crate", [0.0, 0.0, 0.0])
                    },
                    plain(2, Some(1), "Lid", [0.0, 1.0, 0.0]),
                ],
            },
        )
        .unwrap();
    let document = Document {
        guid: SCENE_GUID.to_string(),
        nodes: vec![
            plain(1, None, "Root", [0.0, 0.0, 0.0]),
            Node {
                id: 2,
                parent: Some(1),
                name: None,
                transform: TransformValue::default(),
                mesh: None,
                camera: None,
                light: None,
                prefab: Some(PrefabInstance {
                    source: "crate.prefab".to_string(),
                    overrides: vec![Override {
                        id: 2,
                        transform: Some(transform([5.0, 0.0, 0.0])),
                        name: None,
                        mesh: None,
                        camera: None,
                        light: None,
                    }],
                }),
            },
        ],
    };
    let scene = document_to_scene(&document, &assets).unwrap();
    let root = scene.roots().next().unwrap();
    assert_eq!(scene.get::<Name>(root).unwrap().unwrap().as_str(), "Root");
    let crate_entity = scene.children(root).unwrap().next().unwrap();
    assert_eq!(
        scene.get::<Name>(crate_entity).unwrap().unwrap().as_str(),
        "Crate"
    );
    assert_eq!(
        scene
            .get::<MeshRenderer>(crate_entity)
            .unwrap()
            .unwrap()
            .material,
        material()
    );
    let lid = scene.children(crate_entity).unwrap().next().unwrap();
    assert_eq!(
        scene.local(lid).unwrap().translation,
        Vec3::new(5.0, 0.0, 0.0)
    );
}

#[test]
fn prefab_cycle_and_unknown_material_fail() {
    let mut assets = Assets::default();
    let looping = Document {
        guid: PREFAB_GUID.to_string(),
        nodes: vec![Node {
            prefab: Some(PrefabInstance {
                source: "loop.prefab".to_string(),
                overrides: Vec::new(),
            }),
            ..plain(1, None, "Loop", [0.0, 0.0, 0.0])
        }],
    };
    assets.insert_scene("loop.prefab", looping.clone()).unwrap();
    assert!(matches!(
        cook::cook(&looping, &assets, Some("loop.prefab")),
        Err(Error::Cycle(_))
    ));

    let assets = assets_with_material();
    let document = Document {
        guid: SCENE_GUID.to_string(),
        nodes: vec![Node {
            mesh: Some(MeshValue {
                primitive: PrimitiveValue::Cube,
                material: "missing.mat".to_string(),
            }),
            ..plain(1, None, "Root", [0.0, 0.0, 0.0])
        }],
    };
    assert!(matches!(
        document_to_scene(&document, &assets),
        Err(Error::UnknownAsset(_))
    ));
}

#[test]
fn cook_is_stable_and_matches_scene_worlds() {
    let assets = assets_with_material();
    let document = Document {
        guid: SCENE_GUID.to_string(),
        nodes: vec![
            plain(1, None, "Root", [0.0, 2.0, 0.0]),
            Node {
                mesh: Some(MeshValue {
                    primitive: PrimitiveValue::Cube,
                    material: "materials/checker.mat".to_string(),
                }),
                ..plain(2, Some(1), "Child", [1.0, 0.0, 0.0])
            },
        ],
    };
    let first = cook::cook(&document, &assets, Some("demo.scene")).unwrap();
    let second = cook::cook(&document, &assets, Some("demo.scene")).unwrap();
    assert_eq!(first.as_slice(), second.as_slice());
    let cooked = CookedScene::validate(first.as_slice()).unwrap();
    let scene = document_to_scene(&document, &assets).unwrap();
    let root = scene.roots().next().unwrap();
    let child = scene.children(root).unwrap().next().unwrap();
    assert_eq!(cooked.worlds()[0], scene.world(root).unwrap());
    assert_eq!(cooked.worlds()[1], scene.world(child).unwrap());
    let loaded = cooked.instantiate();
    let loaded_child = loaded
        .children(loaded.roots().next().unwrap())
        .unwrap()
        .next()
        .unwrap();
    assert_eq!(
        loaded
            .get::<MeshRenderer>(loaded_child)
            .unwrap()
            .unwrap()
            .material,
        material()
    );
}

#[test]
fn pak_roundtrip_rejects_a_bad_checksum_and_offset() {
    let assets = assets_with_material();
    let document = Document {
        guid: SCENE_GUID.to_string(),
        nodes: vec![plain(1, None, "Root", [0.0, 0.0, 0.0])],
    };
    let blob = cook::cook(&document, &assets, None).unwrap();
    let id = AssetId::parse(SCENE_GUID).unwrap();
    let packed = write_pak(&[Entry {
        id,
        kind: Kind::Scene,
        bytes: blob.as_slice(),
    }])
    .unwrap();
    let pak = Pak::parse(packed.as_slice()).unwrap();
    let (kind, payload) = pak.get(id).unwrap();
    assert_eq!(kind, Kind::Scene);
    assert_eq!(payload, blob.as_slice());

    let mut corrupt = scene::cooked::AlignedBytes::from_slice(packed.as_slice());
    corrupt.as_mut_slice()[64] ^= 0xff;
    let pak = Pak::parse(corrupt.as_slice()).unwrap();
    assert!(matches!(pak.get(id), Err(Error::Pak("checksum"))));

    let mut shifted = scene::cooked::AlignedBytes::from_slice(packed.as_slice());
    let toc = u64::from_le_bytes(shifted.as_slice()[8..16].try_into().unwrap()) as usize;
    shifted.as_mut_slice()[toc + 24..toc + 32].copy_from_slice(&1u64.to_le_bytes());
    let pak = Pak::parse(shifted.as_slice()).unwrap();
    assert!(matches!(
        pak.get(id),
        Err(Error::Pak("payload offset is not aligned"))
    ));

    let path = std::env::temp_dir().join(format!("engine-pak-{}", std::process::id()));
    {
        let mut file = std::fs::File::create(&path).unwrap();
        file.write_all(packed.as_slice()).unwrap();
    }
    let file = crate::open_pak(&path).unwrap();
    let (kind, payload) = file.pak().unwrap().get(id).unwrap();
    assert_eq!(kind, Kind::Scene);
    assert_eq!(payload, blob.as_slice());
    drop(file);
    std::fs::remove_file(path).unwrap();
}

#[test]
fn parent_cycles_are_rejected() {
    let mutual = Document {
        guid: SCENE_GUID.to_string(),
        nodes: vec![
            plain(1, Some(2), "A", [0.0, 0.0, 0.0]),
            plain(2, Some(1), "B", [0.0, 0.0, 0.0]),
        ],
    };
    assert!(matches!(
        document_to_scene(&mutual, &Assets::default()),
        Err(Error::NodeCycle(_))
    ));
    let itself = Document {
        guid: SCENE_GUID.to_string(),
        nodes: vec![plain(7, Some(7), "Self", [0.0, 0.0, 0.0])],
    };
    assert!(matches!(
        document_to_scene(&itself, &Assets::default()),
        Err(Error::NodeCycle(7))
    ));
}

#[test]
fn unnormalized_rotation_is_rejected_before_cooking() {
    for rotation in [[2.0, 0.0, 0.0, 0.0], [0.0, 0.0, 0.0, 0.0]] {
        let document = Document {
            guid: SCENE_GUID.to_string(),
            nodes: vec![Node {
                transform: TransformValue {
                    rotation,
                    ..TransformValue::default()
                },
                ..plain(4, None, "Root", [0.0, 0.0, 0.0])
            }],
        };
        let assets = Assets::default();
        assert!(matches!(
            document_to_scene(&document, &assets),
            Err(Error::Rotation(4))
        ));
        assert!(matches!(
            cook::cook(&document, &assets, None),
            Err(Error::Rotation(4))
        ));
    }
    let identity = Document {
        guid: SCENE_GUID.to_string(),
        nodes: vec![plain(1, None, "Root", [0.0, 0.0, 0.0])],
    };
    assert!(cook::cook(&identity, &Assets::default(), None).is_ok());
}

#[test]
fn pak_rejects_a_duplicate_guid_and_finds_either_entry() {
    let assets = Assets::default();
    let blob = cook::cook(
        &Document {
            guid: SCENE_GUID.to_string(),
            nodes: vec![plain(1, None, "Root", [0.0, 0.0, 0.0])],
        },
        &assets,
        None,
    )
    .unwrap();
    let scene_id = AssetId::parse(SCENE_GUID).unwrap();
    let prefab_id = AssetId::parse(PREFAB_GUID).unwrap();
    assert!(matches!(
        write_pak(&[
            Entry {
                id: scene_id,
                kind: Kind::Scene,
                bytes: blob.as_slice(),
            },
            Entry {
                id: scene_id,
                kind: Kind::Prefab,
                bytes: blob.as_slice(),
            },
        ]),
        Err(Error::Pak("duplicate guid"))
    ));
    let packed = write_pak(&[
        Entry {
            id: prefab_id,
            kind: Kind::Prefab,
            bytes: blob.as_slice(),
        },
        Entry {
            id: scene_id,
            kind: Kind::Scene,
            bytes: blob.as_slice(),
        },
    ])
    .unwrap();
    let pak = Pak::parse(packed.as_slice()).unwrap();
    assert_eq!(pak.get(scene_id).unwrap().0, Kind::Scene);
    assert_eq!(pak.get(prefab_id).unwrap().0, Kind::Prefab);
}

#[test]
fn repeated_names_are_stored_once() {
    let assets = Assets::default();
    let shared = cook::cook(&named("AAAA", "AAAA"), &assets, None).unwrap();
    let distinct = cook::cook(&named("AAAA", "BBBB"), &assets, None).unwrap();
    assert!(shared.as_slice().len() < distinct.as_slice().len());
}

#[test]
fn scene_to_document_rejects_a_bad_guid() {
    let scene = Scene::new();
    assert!(matches!(
        scene_to_document(&scene, "not-a-guid", &Assets::default()),
        Err(Error::Guid(_))
    ));
    let document =
        scene_to_document(&scene, &SCENE_GUID.replace('-', ""), &Assets::default()).unwrap();
    assert_eq!(document.guid, SCENE_GUID);
}

fn named(left: &str, right: &str) -> Document {
    Document {
        guid: SCENE_GUID.to_string(),
        nodes: vec![
            plain(1, None, left, [0.0, 0.0, 0.0]),
            plain(2, Some(1), right, [0.0, 0.0, 0.0]),
        ],
    }
}

fn assert_same(left: &Scene, right: &Scene) {
    fn walk(left: &Scene, right: &Scene, a: scene::Entity, b: scene::Entity) {
        assert_eq!(left.local(a).unwrap(), right.local(b).unwrap());
        assert_eq!(
            left.get::<Name>(a).unwrap().map(Name::as_str),
            right.get::<Name>(b).unwrap().map(Name::as_str)
        );
        assert_eq!(
            left.get::<MeshRenderer>(a).unwrap().copied(),
            right.get::<MeshRenderer>(b).unwrap().copied()
        );
        assert_eq!(
            left.get::<Camera>(a).unwrap().copied(),
            right.get::<Camera>(b).unwrap().copied()
        );
        assert_eq!(
            left.get::<Light>(a).unwrap().copied(),
            right.get::<Light>(b).unwrap().copied()
        );
        let left_children: Vec<_> = left.children(a).unwrap().collect();
        let right_children: Vec<_> = right.children(b).unwrap().collect();
        assert_eq!(left_children.len(), right_children.len());
        for (left_child, right_child) in left_children.into_iter().zip(right_children) {
            walk(left, right, left_child, right_child);
        }
    }
    let left_roots: Vec<_> = left.roots().collect();
    let right_roots: Vec<_> = right.roots().collect();
    assert_eq!(left_roots.len(), right_roots.len());
    for (left_root, right_root) in left_roots.into_iter().zip(right_roots) {
        walk(left, right, left_root, right_root);
    }
}

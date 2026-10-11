//! People: rigged, animated CC0 characters (Quaternius "Ultimate Modular
//! Men/Women", `web/assets/people`) for the players and customers.
//!
//! Each character is a glTF scene under the body entity, with an animation
//! graph of its five clips (idle, walk, run, interact, wave). The body's
//! speed picks the clip; a customer sitting at the counter idles with the
//! thighs and shins bent onto the stool.

use std::time::Duration;

use bevy::animation::{AnimationPlayer, graph::AnimationGraphHandle, transition::AnimationTransitions};
use bevy::gltf::Gltf;
use bevy::prelude::*;

/// The models, by file name in `web/assets/people`.
pub const MODELS: [&str; 8] =
    ["casual_2", "w_casual", "casual_hoodie", "w_formal", "suit", "w_suit", "worker", "beach"];

/// The clips every model has, in this order.
const CLIPS: [&str; 5] = ["Idle", "Walk", "Run", "Interact", "Wave"];

/// Height of the models' feet below the body entity's center, and their
/// size: a 1.6 m player.
const FEET: f32 = -shared::bar::PLAYER_HEIGHT / 2.0;
const SCALE: f32 = 0.9;

struct Kind {
    gltf: Handle<Gltf>,
    scene: Handle<bevy::world_serialization::WorldAsset>,
    graph: Option<(Handle<AnimationGraph>, Vec<AnimationNodeIndex>)>,
}

/// The loaded models and their animation graphs.
#[derive(Resource)]
pub struct People(Vec<Kind>);

/// A character model under a body: which model, and its animation player
/// and leg bones once the scene is in.
#[derive(Component)]
pub struct Person {
    kind: usize,
    player: Option<Entity>,
    legs: Vec<(Entity, f32)>,
    clip: Option<usize>,
}

/// How a body moves, for its character (set by the body's systems).
#[derive(Component, Default, Clone, Copy)]
pub struct Motion {
    /// Metres per second along the ground.
    pub speed: f32,
    /// Sitting on a stool.
    pub seated: bool,
}

pub fn add(app: &mut App) {
    app.add_systems(Startup, load_people);
    app.add_systems(Update, (build_graphs, link_players, choose_clips).chain());
    app.add_systems(
        PostUpdate,
        sit_down.after(bevy::app::AnimationSystems).before(bevy::transform::TransformSystems::Propagate),
    );
}

fn load_people(mut commands: Commands, assets: Option<Res<AssetServer>>, nodraw: Option<Res<crate::online::NoDraw>>) {
    let Some(assets) = assets.filter(|_| nodraw.is_none()) else { return };
    let kinds = MODELS
        .iter()
        .map(|m| Kind {
            gltf: assets.load(format!("people/{m}.glb")),
            scene: assets.load(GltfAssetLabel::Scene(0).from_asset(format!("people/{m}.glb"))),
            graph: None,
        })
        .collect();
    commands.insert_resource(People(kinds));
}

/// Once a model's file is in, make its animation graph from the named clips.
fn build_graphs(people: Option<ResMut<People>>, gltfs: Res<Assets<Gltf>>, mut graphs: ResMut<Assets<AnimationGraph>>) {
    let Some(mut people) = people else { return };
    for kind in people.0.iter_mut().filter(|k| k.graph.is_none()) {
        let Some(gltf) = gltfs.get(&kind.gltf) else { continue };
        let clips: Vec<Handle<AnimationClip>> =
            CLIPS.iter().filter_map(|c| gltf.named_animations.get(*c).cloned()).collect();
        if clips.len() != CLIPS.len() {
            continue;
        }
        let (graph, nodes) = AnimationGraph::from_clips(clips);
        kind.graph = Some((graphs.add(graph), nodes));
    }
}

/// Give `body` a character: model `kind` (any number: it wraps).
pub fn dress(commands: &mut Commands, people: &People, body: Entity, kind: usize) {
    let kind = kind % people.0.len().max(1);
    let Some(k) = people.0.get(kind) else { return };
    commands.entity(body).with_child((
        Person { kind, player: None, legs: Vec::new(), clip: None },
        bevy::world_serialization::WorldAssetRoot(k.scene.clone()),
        // The models face +Z; a body at yaw 0 faces -Z.
        Transform::from_xyz(0.0, FEET, 0.0)
            .with_rotation(Quat::from_rotation_y(std::f32::consts::PI))
            .with_scale(Vec3::splat(SCALE)),
        Visibility::Inherited,
    ));
}

/// Find each character's animation player (it appears when the scene is
/// spawned, somewhere under the model) and its leg bones.
fn link_players(
    mut commands: Commands,
    people: Option<Res<People>>,
    added: Query<Entity, Added<AnimationPlayer>>,
    parents: Query<&ChildOf>,
    children: Query<&Children>,
    names: Query<&Name>,
    mut persons: Query<&mut Person>,
) {
    let Some(people) = people else { return };
    for player in &added {
        let mut at = player;
        let person = loop {
            if persons.contains(at) {
                break Some(at);
            }
            match parents.get(at) {
                Ok(p) => at = p.parent(),
                Err(_) => break None,
            }
        };
        let Some(root) = person else { continue };
        let Ok(mut p) = persons.get_mut(root) else { continue };
        p.player = Some(player);
        // The legs: thighs bend forward to sit, shins bend back down.
        for e in children.iter_descendants(root) {
            match names.get(e).map(|n| n.as_str()) {
                Ok("UpperLeg.L" | "UpperLeg.R") => p.legs.push((e, 1.45)),
                Ok("LowerLeg.L" | "LowerLeg.R") => p.legs.push((e, -1.45)),
                _ => {}
            }
        }
        if let Some((graph, _)) = &people.0[p.kind].graph {
            commands.entity(player).insert((AnimationGraphHandle(graph.clone()), AnimationTransitions::new()));
        }
    }
}

/// Idle, walk or run by the body's speed.
fn choose_clips(
    mut commands: Commands,
    people: Option<Res<People>>,
    cam: Query<&GlobalTransform, With<Camera3d>>,
    bodies: Query<&GlobalTransform, Without<Camera3d>>,
    mut persons: Query<(&mut Person, &ChildOf)>,
    motions: Query<&Motion>,
    mut players: Query<(&mut AnimationPlayer, Option<&mut AnimationTransitions>, Has<AnimationGraphHandle>)>,
) {
    let Some(people) = people else { return };
    let eye = cam.single().map(|c| c.translation()).ok();
    for (mut p, parent) in &mut persons {
        let Some(player) = p.player else { continue };
        // Far away the motion cannot be seen: stop animating (it costs CPU
        // every frame for every bone).
        let far = match (eye, bodies.get(parent.parent())) {
            (Some(e), Ok(b)) => b.translation().distance(e) > ANIMATE_WITHIN,
            _ => false,
        };
        let Some((graph, nodes)) = &people.0[p.kind].graph else { continue };
        let Ok((mut anim, transitions, has_graph)) = players.get_mut(player) else { continue };
        if !has_graph {
            commands.entity(player).insert((AnimationGraphHandle(graph.clone()), AnimationTransitions::new()));
            continue;
        }
        let Some(mut transitions) = transitions else { continue };
        if far != anim.all_paused() {
            if far {
                anim.pause_all();
            } else {
                anim.resume_all();
            }
        }
        let m = motions.get(parent.parent()).copied().unwrap_or_default();
        let want = if m.seated {
            0
        } else if m.speed > 3.6 {
            2
        } else if m.speed > 0.3 {
            1
        } else {
            0
        };
        if p.clip != Some(want) {
            p.clip = Some(want);
            transitions.play(&mut anim, nodes[want], Duration::from_millis(200)).repeat();
        }
    }
}

/// Characters farther than this from the camera (metres) stop animating.
const ANIMATE_WITHIN: f32 = 20.0;

/// How far a seated character's hips drop from standing to the stool seat.
const SIT_DROP: f32 = 0.3;

/// After the animation has posed the bones: a seated character drops onto
/// the stool, thighs forward and shins down.
fn sit_down(
    mut persons: Query<(&Person, &ChildOf, &mut Transform)>,
    motions: Query<&Motion>,
    mut bones: Query<&mut Transform, Without<Person>>,
) {
    for (p, parent, mut t) in &mut persons {
        let seated = motions.get(parent.parent()).is_ok_and(|m| m.seated);
        let y = if seated { FEET - SIT_DROP } else { FEET };
        if t.translation.y != y {
            t.translation.y = y;
        }
        if !seated {
            continue;
        }
        for (bone, angle) in &p.legs {
            if let Ok(mut t) = bones.get_mut(*bone) {
                t.rotation *= Quat::from_rotation_x(*angle);
            }
        }
    }
}

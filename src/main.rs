#![allow(private_interfaces)]

use std::f32::consts::PI;

use avian2d::prelude as p;
use bevy::app::PluginGroup as _;
use bevy::ecs::schedule::IntoScheduleConfigs as _;
use bevy::ecs::spawn::SpawnRelated as _;
use bevy::math::{Vec2, Vec3Swizzles as _, ivec2, vec2, vec3};
use bevy::prelude as b;
use bevy::utils::default;
use bevy_enhanced_input::prelude as bei;
use bevy_enhanced_input::prelude::InputContextAppExt as _;

// -------------------------------------------------------------------------------------------------

mod bullets_and_targets;
use bullets_and_targets::{Attackable, Gun, PlayerBullet};

mod rendering;
use crate::rendering::{PLAYFIELD_LAYERS, PlayfieldCamera, SCALING_MARGIN, UI_LAYERS, Zees};

// -------------------------------------------------------------------------------------------------

fn main() {
    b::App::new()
        .add_plugins(
            b::DefaultPlugins
                .set(bevy::audio::AudioPlugin {
                    default_spatial_scale: bevy::audio::SpatialScale::new_2d(0.001),
                    ..default()
                })
                .set(b::ImagePlugin::default_nearest())
                .set(b::WindowPlugin {
                    primary_window: Some(b::Window {
                        //TODO: positioning for development, not for release
                        position: b::WindowPosition::At(ivec2(3000, 0)),

                        resolution: {
                            let desired_scale = 2;
                            let cautionary_fudge_pixels = 2;
                            bevy::window::WindowResolution::new(
                                SCREEN_SIZE.x * desired_scale
                                    + SCALING_MARGIN
                                    + cautionary_fudge_pixels,
                                SCREEN_SIZE.y * desired_scale
                                    + SCALING_MARGIN
                                    + cautionary_fudge_pixels,
                            )
                        },
                        ..default()
                    }),
                    ..default()
                }),
        )
        .add_plugins(bevy_enhanced_input::EnhancedInputPlugin)
        .add_input_context::<Player>()
        .add_plugins(avian2d::PhysicsPlugins::default())
        //.add_plugins(avian2d::prelude::PhysicsDebugPlugin::default())
        .add_systems(
            b::Startup,
            (rendering::setup_camera_system, setup_gameplay, setup_ui).chain(),
        )
        .add_systems(b::Update, rendering::fit_canvas_to_window_system)
        .add_systems(b::FixedUpdate, apply_movement)
        .add_systems(
            b::FixedUpdate,
            // put these in *some* order for consistency
            (expire_lifetimes, bullets_and_targets::bullet_hit_system).chain(),
        )
        .add_systems(
            b::FixedUpdate,
            (
                quantity_behaviors,
                (
                    update_quantity_display_system_1,
                    update_quantity_display_system_2,
                ),
            )
                .chain(),
        )
        .add_systems(b::FixedUpdate, bullets_and_targets::gun_cooldown)
        .add_observer(bullets_and_targets::shoot)
        .run();
}

// -------------------------------------------------------------------------------------------------

/// Size of UI enclosing playfield, for pixel rendering
const SCREEN_SIZE: b::UVec2 = b::uvec2(640, 480);

/// Size of the playfield.
/// If you change this, the assets must be changed to match too
const PLAYFIELD_SIZE: b::UVec2 = b::uvec2(320, 460);

const PLAYFIELD_RECT: b::Rect = b::Rect {
    min: vec2(PLAYFIELD_SIZE.x as f32 / -2., PLAYFIELD_SIZE.y as f32 / -2.),
    max: vec2(PLAYFIELD_SIZE.x as f32 / 2., PLAYFIELD_SIZE.y as f32 / 2.),
};

// -------------------------------------------------------------------------------------------------

/// Player ship entity
#[derive(Debug, b::Component)]
#[require(b::Transform)]
struct Player;

/// Decremented by game time and despawns the entity when it is zero
#[derive(Debug, b::Component)]
struct Lifetime(f32);

// -------------------------------------------------------------------------------------------------
// Quantities

/// A value between 0 and 1 that is displayed to the player as a bar.
/// Other components on this entity define which quantity it is and how systems affect it.
#[derive(Debug, b::Component)]
struct Quantity {
    value: f32,
}

/// [`Quantity`] 1/3; affects shooting.
#[derive(Debug, b::Component)]
struct Coherence;

/// [`Quantity`] 2/3; maxing it is game over.
#[derive(Debug, b::Component)]
struct Fever;

/// [`Quantity`] 3/3; maxing it is a win.
#[derive(Debug, b::Component)]
struct Fervor;

/// Specifies a [`Quantity`] this entity should update its visual appearance (e.g. bar length) from.
#[derive(Debug, b::Component)]
struct UpdateFromQuantity(b::Entity);

// -------------------------------------------------------------------------------------------------

#[derive(Debug, bei::InputAction)]
#[action_output(b::Vec2)]
struct Move;

#[derive(Debug, bei::InputAction)]
#[action_output(bool)]
struct Shoot;

// -------------------------------------------------------------------------------------------------

fn setup_ui(
    mut commands: b::Commands,
    asset_server: b::Res<b::AssetServer>,
    coherence: b::Single<b::Entity, (b::With<Coherence>, b::Without<Fever>, b::Without<Fervor>)>,
    fever: b::Single<b::Entity, (b::With<Fever>, b::Without<Coherence>, b::Without<Fervor>)>,
    fervor: b::Single<b::Entity, (b::With<Fervor>, b::Without<Coherence>, b::Without<Fever>)>,
) {
    commands.spawn((
        b::Sprite::from_image(asset_server.load("playfield-frame.png")),
        b::Transform::from_xyz(0., 0., Zees::UiElement.z()),
        UI_LAYERS,
    ));

    let bar_fill_image = asset_server.load("bar-fill.png");

    commands.spawn(bar_bundle(
        &bar_fill_image,
        "Coherence",
        *coherence,
        vec2(PLAYFIELD_RECT.min.x - 20.0, PLAYFIELD_RECT.min.y),
    ));
    commands.spawn(bar_bundle(
        &bar_fill_image,
        "Fever",
        *fever,
        vec2(PLAYFIELD_RECT.min.x - 60.0, PLAYFIELD_RECT.min.y),
    ));
    commands.spawn(bar_bundle(
        &bar_fill_image,
        "Fervor",
        *fervor,
        vec2(PLAYFIELD_RECT.min.x - 100.0, PLAYFIELD_RECT.min.y),
    ));
}

/// Build the UI for a [`Quantity`] bar
fn bar_bundle(
    bar_fill_image: &b::Handle<b::Image>,
    label: &str,
    quantity_entity: b::Entity,
    position: Vec2,
) -> impl b::Bundle {
    (
        b::children![
            (
                b::Sprite::from_image(bar_fill_image.clone()),
                bevy::sprite::Anchor::CENTER_LEFT,
                UpdateFromQuantity(quantity_entity),
                UI_LAYERS,
            ),
            (
                b::Text2d::new(label),
                b::TextLayout::new_with_justify(b::Justify::Left),
                bevy::sprite::Anchor::CENTER_LEFT,
                b::Transform::from_translation(vec3(0.0, 20.0, 0.0)),
                UI_LAYERS,
            )
        ],
        b::Visibility::default(), // needed for hierarchy https://bevy.org/learn/errors/b0004/
        b::Transform {
            translation: position.extend(Zees::UiElement.z()),
            rotation: b::Quat::from_rotation_z(PI / 2.),
            ..default()
        },
    )
}

/// Spawn the entities that participate in gameplay rules.
fn setup_gameplay(mut commands: b::Commands, asset_server: b::Res<b::AssetServer>) {
    // player sprite
    let player_sprite_asset = asset_server.load("player.png");
    commands.spawn((
        Player,
        b::Transform::from_xyz(0., PLAYFIELD_RECT.min.y + 20.0, Zees::Player.z()),
        b::Sprite::from_image(player_sprite_asset.clone()),
        PLAYFIELD_LAYERS,
        bei::actions!(Player[
            (
                bei::Action::<Move>::new(),
                bei::DeadZone::default(),
                //bei::SmoothNudge::default(),
                bei::Bindings::spawn((
                    bei::Cardinal::wasd_keys(),
                    bei::Axial::left_stick(),
                )),
            ),
            (
                bei::Action::<Shoot>::new(),
                bei::bindings![b::KeyCode::Space, b::GamepadButton::South],
            ),
        ]),
        p::Collider::circle(8.),
        Gun { cooldown: 0.0 },
    ));

    commands.spawn((Coherence, Quantity { value: 0.5 }));
    commands.spawn((Fever, Quantity { value: 0.5 }));
    commands.spawn((Fervor, Quantity { value: 0.0 }));

    for x in (-100..100).step_by(32) {
        for y in [100, 120, 240] {
            commands.spawn((
                Attackable { health: 10 },
                b::Transform::from_xyz(x as f32, y as f32, 0.0),
                b::Sprite::from_image(player_sprite_asset.clone()), // TODO: enemy sprite
                PLAYFIELD_LAYERS,
                p::Collider::circle(8.),
                Gun { cooldown: 0.0 },
            ));
        }
    }
}

// -------------------------------------------------------------------------------------------------

fn apply_movement(
    action: b::Single<&bei::Action<Move>>,
    time: b::Res<b::Time>,
    player_query: b::Query<&mut b::Transform, b::With<Player>>,
) -> b::Result {
    let movement: b::Vec2 = ***action;
    let delta_position = movement * 150.0 * time.delta_secs(); // apply speed
    for mut transform in player_query {
        let new_position: b::Vec2 = (transform.translation.xy() + delta_position)
            .clamp(PLAYFIELD_RECT.min, PLAYFIELD_RECT.max);
        transform.translation.x = new_position.x;
        transform.translation.y = new_position.y;
    }
    Ok(())
}

// -------------------------------------------------------------------------------------------------

fn expire_lifetimes(
    mut commands: b::Commands,
    time: b::Res<b::Time>,
    query: b::Query<(b::Entity, &mut Lifetime, b::Has<PlayerBullet>)>,
    mut coherence: b::Single<
        &mut Quantity,
        (b::With<Coherence>, b::Without<Fever>, b::Without<Fervor>),
    >,
) {
    let delta = time.delta_secs();
    for (entity, mut lifetime, is_bullet) in query {
        let new_lifetime = lifetime.0 - delta;
        if new_lifetime > 0. {
            lifetime.0 = new_lifetime;
        } else {
            commands.entity(entity).despawn();

            // If this is a bullet, then if it expired, it is a miss; lose coherence.
            if is_bullet {
                coherence.value = (coherence.value - 0.01).clamp(0.0, 1.0);
            }
        }
    }
}

// -------------------------------------------------------------------------------------------------

#[expect(unused_variables)]
fn quantity_behaviors(
    coherence: b::Single<
        &mut Quantity,
        (b::With<Coherence>, b::Without<Fever>, b::Without<Fervor>),
    >,
    fever: b::Single<&mut Quantity, (b::With<Fever>, b::Without<Coherence>, b::Without<Fervor>)>,
    fervor: b::Single<&mut Quantity, (b::With<Fervor>, b::Without<Coherence>, b::Without<Fever>)>,
) -> b::Result {
    Ok(())
}

fn update_quantity_display_system_1(
    //coherence: b::Single<&Quantity, b::With<Coherence>>,
    fever: b::Single<&Quantity, b::With<Fever>>,
    //fervor: b::Single<&Quantity, b::With<Fervor>>,
    mut pixel_camera: b::Single<&mut b::Camera, b::With<PlayfieldCamera>>,
) -> b::Result {
    pixel_camera.clear_color = bevy::camera::ClearColorConfig::Custom(b::Color::oklch(
        fever.value * 0.05,
        fever.value,
        0.0,
    ));
    Ok(())
}

fn update_quantity_display_system_2(
    quantities: b::Query<&Quantity>,
    bars_to_update: b::Query<(&mut b::Transform, &UpdateFromQuantity)>,
) -> b::Result {
    for (mut bar_transform, ufq) in bars_to_update {
        let quantity = quantities.get(ufq.0)?.value;
        // TODO: establish a constant for bar height instead
        bar_transform.scale = vec3((SCREEN_SIZE.y as f32 - 20.0) / 16.0 * quantity, 1.0, 1.0);
    }
    Ok(())
}

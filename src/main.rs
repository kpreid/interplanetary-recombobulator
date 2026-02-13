#![allow(private_interfaces)]

use std::f32::consts::PI;

use avian2d::prelude::{self as p, PhysicsTime as _};
use bevy::app::PluginGroup as _;
use bevy::ecs::schedule::IntoScheduleConfigs;
use bevy::ecs::spawn::SpawnRelated as _;
use bevy::math::{Vec2, Vec3, Vec3Swizzles as _, ivec2, vec2, vec3};
use bevy::prelude as b;
use bevy::state::app::AppExtStates as _;
use bevy::utils::default;
use bevy_asset_loader::asset_collection::AssetCollection; // required by derive macro :(
use bevy_asset_loader::loading_state::LoadingStateAppExt as _;
use bevy_asset_loader::loading_state::config::ConfigureLoadingState as _;
use bevy_enhanced_input::prelude as bei;
use bevy_enhanced_input::prelude::InputContextAppExt as _;
use rand::RngExt as _;

// -------------------------------------------------------------------------------------------------

mod bullets_and_targets;
use bullets_and_targets::Gun;

mod enemy;

mod rendering;
use rendering::{PLAYFIELD_LAYERS, SCALING_MARGIN, UI_LAYERS, Zees};

mod quantity;
use quantity::{Coherence, Fervor, Fever, Quantity};

use crate::bullets_and_targets::{Attackable, Pattern};

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
        .init_state::<GameState>()
        .add_loading_state(
            bevy_asset_loader::loading_state::LoadingState::new(GameState::AssetLoading)
                .continue_to_state(GameState::NotStarted)
                .load_collection::<Preload>(),
        )
        .add_plugins(bevy_enhanced_input::EnhancedInputPlugin)
        .add_input_context::<Player>()
        .add_input_context::<NonGameInput>()
        .add_plugins(avian2d::PhysicsPlugins::default())
        //.add_plugins(avian2d::prelude::PhysicsDebugPlugin::default())
        .add_systems(
            b::Startup,
            (
                rendering::setup_camera_system,
                setup_non_game_input,
                setup_status_text,
                setup_permanent_gameplay,
            ),
        )
        .add_systems(b::OnExit(GameState::AssetLoading), setup_ui)
        .add_systems(b::OnExit(GameState::NotStarted), start_new_game)
        .add_systems(b::OnExit(GameState::GameOver), despawn_game)
        .add_systems(b::OnEnter(GameState::Playing), unpause)
        .add_systems(b::OnExit(GameState::Playing), pause)
        .add_observer(pause_unpause_observer)
        .add_systems(
            b::Update,
            (
                rendering::fit_canvas_to_window_system,
                update_status_text_system,
            ),
        )
        .add_systems(
            b::FixedUpdate,
            // These need to be ordered because which order they run in affects mechanics.
            // Besides the questions of bullet range and who wins, there is also an interaction
            // between expire_lifetimes and bullet_hit_system for which  it is expected that
            // expirations happen on the next frame and not the current one.
            (
                expire_lifetimes,
                apply_movement,
                pickup_system,
                bullets_and_targets::gun_cooldown,
                enemy::enemy_ship_ai,
                bullets_and_targets::fire_gun_system,
                bullets_and_targets::bullet_hit_system,
                bullets_and_targets::player_health_is_fever_system,
            )
                .chain()
                .run_if(b::in_state(GameState::Playing)),
        )
        .add_systems(b::Update, bullets_and_targets::hurt_animation_system)
        .add_systems(
            b::FixedUpdate,
            (
                quantity::quantity_behaviors_system.run_if(b::in_state(GameState::Playing)),
                (
                    quantity::update_quantity_display_system_1,
                    quantity::update_quantity_display_system_2,
                ),
            )
                .chain(),
        )
        .add_systems(
            b::FixedUpdate,
            (enemy::spawn_enemies_system, spawn_starfield_system)
                .run_if(b::in_state(GameState::Playing)),
        )
        .add_observer(bullets_and_targets::player_input_fire_gun)
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
///
/// Note that player-related entities are also identified by [`Team`].
#[derive(Debug, b::Component)]
#[require(b::Transform, p::CollidingEntities)]
struct Player;

/// Which side of the fight this entity belongs to.
/// Bullets and damageable entities need to be on a team.
#[derive(Clone, Copy, Debug, Eq, PartialEq, b::Component)]
enum Team {
    Player,
    Enemy,
}
impl Team {
    pub fn should_hurt(self, other_team: Team) -> bool {
        other_team != self
    }
}

#[derive(Debug, b::Component)]
struct StarfieldSpawner {
    /// set to true on the first frame only
    startup: bool,
    cooldown: f32,
}

/// Decremented by game time and despawns the entity when it is zero.
///
/// Note that this component is also treated slightly specially for bullets;
/// a value of zero is used to indicate that the
#[derive(Debug, b::Component)]
struct Lifetime(f32);

/// On colliding with [`Player`], has an effect and despawns the entity.
/// This is used for both pickups and colliding with enemies.
#[derive(Debug, b::Component)]
enum Pickup {
    /// Increase [`Fever`] by this amount, and depict it as a damaging hit.
    Damage(f32),
    /// Decrease [`Fever`] by this amount.
    Cool(f32),
    /// Increase [`Coherence`] by this amount.
    Cohere(f32),
}

#[derive(Clone, Eq, PartialEq, Debug, Hash, Default, b::States)]
enum GameState {
    #[default]
    AssetLoading,

    /// Game not started. Game entities do not exist.
    NotStarted,

    Playing,

    Paused,

    /// Game entities exist but are frozen.
    GameOver,
}

#[derive(Debug, b::Component)]
struct StatusText;

/// Assets that we use for things spawned after startup.
#[derive(b::Resource, bevy_asset_loader::asset_collection::AssetCollection)]
struct Preload {
    // Enemy assets
    #[asset(path = "player.png")] // TODO: enemy sprite
    enemy_sprite: b::Handle<b::Image>,
    #[asset(path = "enemy-bullet.png")]
    enemy_bullet_sprite: b::Handle<b::Image>,
    #[asset(path = "enemy-hurt.ogg")]
    enemy_hurt_sound: b::Handle<b::AudioSource>,
    #[asset(path = "enemy-kill.ogg")]
    enemy_kill_sound: b::Handle<b::AudioSource>,

    // Player assets
    #[asset(path = "player.png")]
    player_ship_sprite: b::Handle<b::Image>,
    #[asset(path = "player-bullet.png")]
    player_bullet_sprite: b::Handle<b::Image>,
    #[asset(path = "shoot.ogg")]
    shoot_sound: b::Handle<b::AudioSource>,

    // Pickups
    #[asset(path = "pickup-cool.png")]
    pickup_cool_sprite: b::Handle<b::Image>,
    #[asset(path = "pickup.ogg")]
    pickup_sound: b::Handle<b::AudioSource>,

    // UI
    #[asset(path = "bar-frame.png")]
    bar_frame_sprite: b::Handle<b::Image>,
    #[asset(path = "bar-fill-base.png")]
    bar_fill_base_sprite: b::Handle<b::Image>,
    #[asset(path = "bar-fill-temporary.png")]
    bar_fill_temporary_sprite: b::Handle<b::Image>,
    #[asset(path = "text-bar-coherence.png")]
    text_bar_coherence_sprite: b::Handle<b::Image>,
    #[asset(path = "text-bar-fever.png")]
    text_bar_fever_sprite: b::Handle<b::Image>,
    #[asset(path = "text-bar-fervor.png")]
    text_bar_fervor_sprite: b::Handle<b::Image>,

    // Misc
    #[asset(path = "star.png")]
    star_sprite: b::Handle<b::Image>,
    #[asset(path = "muzzle-flash.png")]
    muzzle_flash_sprite: b::Handle<b::Image>,
}

// -------------------------------------------------------------------------------------------------

/// Context entity for inputs that shouldn’t depend on gameplay state, such as the escape key.
#[derive(Debug, b::Component)]
struct NonGameInput;

#[derive(Debug, bei::InputAction)]
#[action_output(b::Vec2)]
struct Move;

#[derive(Debug, bei::InputAction)]
#[action_output(bool)]
struct Shoot;

#[derive(Debug, bei::InputAction)]
#[action_output(bool)]
struct Escape;

// -------------------------------------------------------------------------------------------------
// Startup systems (not all literally `Startup` schedule)

fn setup_non_game_input(mut commands: b::Commands) {
    commands.spawn((
        NonGameInput,
        bei::actions!(
            NonGameInput[(
                bei::Action::<Escape>::new(),
                bei::bindings![b::KeyCode::Escape, b::GamepadButton::Start],
            )]
        ),
    ));
}

/// Early startup doesn't need assets to be ready
fn setup_status_text(mut commands: b::Commands) {
    // Gameplay status text
    commands.spawn((
        StatusText,
        b::Text2d::new(""),
        bevy::text::TextBounds {
            // if we don’t set this, the text wraps undesirably, maybe because it gets changed?
            width: Some(PLAYFIELD_SIZE.x as f32),
            height: None,
        },
        b::TextLayout::new_with_justify(b::Justify::Center),
        bevy::sprite::Anchor::CENTER,
        b::Transform::from_translation(vec3(0.0, 20.0, 0.0)),
        UI_LAYERS,
    ));
}

fn setup_ui(
    mut commands: b::Commands,
    asset_server: b::Res<b::AssetServer>,
    assets: b::Res<Preload>,
    coherence: b::Single<b::Entity, (b::With<Coherence>, b::Without<Fever>, b::Without<Fervor>)>,
    fever: b::Single<b::Entity, (b::With<Fever>, b::Without<Coherence>, b::Without<Fervor>)>,
    fervor: b::Single<b::Entity, (b::With<Fervor>, b::Without<Coherence>, b::Without<Fever>)>,
) {
    commands.spawn((
        b::Sprite::from_image(asset_server.load("playfield-frame.png")),
        b::Transform::from_xyz(0., 0., Zees::UiFront.z()),
        UI_LAYERS,
    ));

    commands.spawn(bar_bundle(
        &assets,
        assets.text_bar_fever_sprite.clone(),
        *fever,
        vec2(PLAYFIELD_RECT.min.x - 20.0, PLAYFIELD_RECT.min.y),
    ));
    commands.spawn(bar_bundle(
        &assets,
        assets.text_bar_coherence_sprite.clone(),
        *coherence,
        vec2(PLAYFIELD_RECT.min.x - 60.0, PLAYFIELD_RECT.min.y),
    ));
    commands.spawn(bar_bundle(
        &assets,
        assets.text_bar_fervor_sprite.clone(),
        *fervor,
        vec2(PLAYFIELD_RECT.min.x - 100.0, PLAYFIELD_RECT.min.y),
    ));
}

/// Build the UI for a [`Quantity`] bar
fn bar_bundle(
    assets: &Preload,
    label: b::Handle<b::Image>,
    quantity_entity: b::Entity,
    position: Vec2,
) -> impl b::Bundle {
    (
        b::children![
            (
                b::Sprite::from_image(assets.bar_frame_sprite.clone()),
                b::Transform::from_translation(vec3(0.0, 0.0, Zees::UiFront.z())),
                bevy::sprite::Anchor::CENTER_LEFT,
                UI_LAYERS,
            ),
            (
                b::Sprite {
                    image: assets.bar_fill_base_sprite.clone(),
                    image_mode: b::SpriteImageMode::Tiled {
                        tile_x: true,
                        tile_y: true,
                        stretch_value: 1.0,
                    },
                    ..default()
                },
                b::Transform::from_translation(vec3(2.0, 0.0, Zees::UiBack.z())),
                bevy::sprite::Anchor::CENTER_LEFT,
                quantity::UpdateFromQuantity {
                    quantity_entity,
                    property: quantity::UpdateProperty::BaseValueToLength,
                },
                UI_LAYERS,
            ),
            (
                b::Sprite {
                    image: assets.bar_fill_temporary_sprite.clone(),
                    image_mode: b::SpriteImageMode::Tiled {
                        tile_x: true,
                        tile_y: true,
                        stretch_value: 1.0,
                    },
                    ..default()
                },
                b::Transform::from_translation(vec3(2.0, 0.0, Zees::UiMiddle.z())),
                bevy::sprite::Anchor::CENTER_LEFT,
                quantity::UpdateFromQuantity {
                    quantity_entity,
                    property: quantity::UpdateProperty::TemporaryValueToLength,
                },
                UI_LAYERS,
            ),
            (
                b::Sprite::from_image(label),
                //b::Text2d::new(label),
                //b::TextLayout::new_with_justify(b::Justify::Left),
                bevy::sprite::Anchor::CENTER_LEFT,
                b::Transform::from_translation(vec3(10.0, 13.0, Zees::UiFront2.z())),
                UI_LAYERS,
            )
        ],
        b::Visibility::default(), // needed for hierarchy https://bevy.org/learn/errors/b0004/
        b::Transform {
            translation: position.extend(0.0),
            rotation: b::Quat::from_rotation_z(PI / 2.),
            ..default()
        },
    )
}

/// Spawn the entities that participate in gameplay rules and which exist forever.
/// Also the input bindings that don’t relate to the player ship.
fn setup_permanent_gameplay(mut commands: b::Commands) {
    commands.spawn((Coherence, Quantity::new(Coherence::INITIAL)));
    commands.spawn((Fever, Quantity::new(Fever::INITIAL)));
    commands.spawn((Fervor, Quantity::new(Fervor::INITIAL)));

    commands.spawn(StarfieldSpawner {
        startup: true,
        cooldown: 0.0,
    });
}

fn start_new_game(
    mut commands: b::Commands,
    assets: b::Res<Preload>,
    mut coherence: b::Single<
        &mut Quantity,
        (b::With<Coherence>, b::Without<Fever>, b::Without<Fervor>),
    >,
    mut fever: b::Single<
        &mut Quantity,
        (b::With<Fever>, b::Without<Coherence>, b::Without<Fervor>),
    >,
    mut fervor: b::Single<
        &mut Quantity,
        (b::With<Fervor>, b::Without<Coherence>, b::Without<Fever>),
    >,
) {
    **coherence = Quantity::new(Coherence::INITIAL);
    **fever = Quantity::new(Fever::INITIAL);
    **fervor = Quantity::new(Fervor::INITIAL);

    commands.spawn((
        Player,
        Team::Player,
        bullets_and_targets::Attackable {
            // any health below the max translates into fever increase via player_health_is_fever_system()
            health: u8::MAX,
            hurt_animation_cooldown: 0.0,
            drops: false,
        },
        b::Transform::from_xyz(0., PLAYFIELD_RECT.min.y + 20.0, Zees::Player.z()),
        b::Sprite::from_image(assets.player_ship_sprite.clone()),
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
            )
        ]),
        p::Collider::circle(8.),
        Gun {
            cooldown: 0.0,
            base_cooldown: 0.5,
            trigger: false,
            pattern: Pattern::Coherent,
        },
    ));

    commands.spawn(enemy::EnemySpawner {
        cooldown: 0.0,
        spawn_pattern_state: 0,
    });
}

// Despawn everything [`start_new_game`] spawns
fn despawn_game(
    mut commands: b::Commands,
    things: b::Query<
        b::Entity,
        b::Or<(
            b::With<Team>,
            b::With<enemy::EnemySpawner>,
            b::With<Lifetime>,
        )>,
    >,
    // assets: b::Res<Preload>,
) {
    bevy::log::info!("despawn_game");
    for entity in things {
        commands.entity(entity).despawn();
    }
    // start_new_game(commands, assets)
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
// Other game behaviors use `run_if`; physics pausing needs explicit action

fn pause(mut time: b::ResMut<b::Time<p::Physics>>) {
    time.pause();
}

fn unpause(mut time: b::ResMut<b::Time<p::Physics>>) {
    time.unpause();
}

fn pause_unpause_observer(
    _event: b::On<bei::Start<Escape>>,
    state: b::ResMut<b::State<GameState>>,
    mut next_state: b::ResMut<b::NextState<GameState>>,
) {
    bevy::log::info!("pause_unpause");
    next_state.set_if_neq(match *state.get() {
        GameState::AssetLoading => return,
        GameState::Playing => GameState::Paused,
        GameState::Paused | GameState::NotStarted => GameState::Playing,
        GameState::GameOver => GameState::NotStarted,
    });
}

// -------------------------------------------------------------------------------------------------

fn expire_lifetimes(
    mut commands: b::Commands,
    time: b::Res<b::Time>,
    query: b::Query<(b::Entity, &mut Lifetime)>,
) {
    let delta = time.delta_secs();
    for (entity, mut lifetime) in query {
        let new_lifetime = lifetime.0 - delta;
        if new_lifetime > 0. {
            lifetime.0 = new_lifetime;
        } else {
            commands.entity(entity).despawn();
        }
    }
}

fn pickup_system(
    mut commands: b::Commands,
    player_query: b::Single<(&p::CollidingEntities, &mut Attackable), b::With<Player>>,
    pickups: b::Query<(&Pickup, &b::Transform)>,
    mut coherence: b::Single<
        &mut Quantity,
        (b::With<Coherence>, b::Without<Fever>, b::Without<Fervor>),
    >,
    mut fever: b::Single<
        &mut Quantity,
        (b::With<Fever>, b::Without<Coherence>, b::Without<Fervor>),
    >,
    assets: b::Res<crate::Preload>,
) -> b::Result {
    let (player_collisions, mut player_attackable) = player_query.into_inner();
    for &pickup_entity in &player_collisions.0 {
        let Ok((pickup, &pickup_transform)) = pickups.get(pickup_entity) else {
            // not a pickup
            continue;
        };

        let sound_asset;

        match *pickup {
            Pickup::Damage(amount) => {
                fever.adjust_permanent_including_temporary(amount);

                player_attackable.hurt_flash();

                sound_asset = Some(assets.enemy_hurt_sound.clone()); // TODO: separate player hurt 
            }
            Pickup::Cool(amount) => {
                fever.adjust_permanent_ignoring_temporary(-amount);
                sound_asset = Some(assets.pickup_sound.clone());
            }
            Pickup::Cohere(amount) => {
                coherence.adjust_permanent_ignoring_temporary(-amount);
                sound_asset = Some(assets.pickup_sound.clone());
            }
        }

        commands.entity(pickup_entity).despawn();

        if let Some(sound_asset) = sound_asset {
            commands.spawn((
                b::AudioPlayer::new(sound_asset),
                b::PlaybackSettings {
                    spatial: true,
                    volume: bevy::audio::Volume::Decibels(-10.),
                    ..b::PlaybackSettings::DESPAWN
                },
                pickup_transform,
            ));
        }
    }
    Ok(())
}

// -------------------------------------------------------------------------------------------------

fn spawn_starfield_system(
    mut commands: b::Commands,
    time: b::Res<b::Time>,
    spawners: b::Query<&mut StarfieldSpawner>,
    assets: b::Res<crate::Preload>,
) {
    let spawn_period = 0.02;

    let delta = time.delta_secs();
    for mut spawner in spawners {
        let StarfieldSpawner { startup, cooldown }: &mut StarfieldSpawner = &mut *spawner;
        if *startup {
            *startup = false;

            // TODO: would be cleaner to calculate the number to spawn based on velocity,
            // but that's harder
            for t in (0..1000).map(|i| i as f32 * spawn_period) {
                commands.spawn(star_bundle(&assets, t));
            }
        } else if *cooldown > 0.0 {
            *cooldown = (*cooldown - delta).max(0.0);
        } else {
            *cooldown = spawn_period;

            commands.spawn(star_bundle(&assets, 0.0));
        }
    }
}

fn star_bundle(assets: &Preload, fast_forward: f32) -> impl b::Bundle {
    let overflow_x = 30.0;

    let velocity = vec2(
        0.0, //rand::rng().random_range(-10.0..=10.0),
        -rand::rng().random_range(40.0..=120.0),
    );
    let x = rand::rng()
        .random_range(PLAYFIELD_RECT.min.x - overflow_x..=PLAYFIELD_RECT.max.x + overflow_x);
    let y = PLAYFIELD_RECT.max.y + 80. + rand::rng().random_range(0.0..=30.0); // start offscreen
    (
        b::Sprite::from_image(assets.star_sprite.clone()),
        b::Transform::from_translation(
            (vec2(x, y) + velocity * fast_forward).extend(Zees::Starfield.z()),
        )
        .with_rotation(b::Quat::from_rotation_z(-velocity.angle_to(Vec2::NEG_Y)))
        .with_scale(Vec3::splat(2.0)),
        PLAYFIELD_LAYERS,
        p::RigidBody::Kinematic,
        p::Collider::circle(1.0), // TODO: use a simple movement system w/o physics
        // Note: no collider because it doesn't interact with anything
        p::LinearVelocity(velocity),
        Lifetime(20.0), // TODO: would be more efficient to detect when the sprite is off the screen
    )
}

// -------------------------------------------------------------------------------------------------

fn update_status_text_system(
    state: b::Res<b::State<GameState>>,
    mut text: b::Single<&mut b::Text2d, b::With<StatusText>>,
) {
    let new_text = match *state.get() {
        GameState::AssetLoading => "Loading",
        GameState::NotStarted => "Ready", // TODO: make this blank once we have other menu UI
        GameState::GameOver => "Game Overheated",
        GameState::Playing => "",
        GameState::Paused => "Paused",
    };

    if ***text != new_text {
        text.clear();
        text.push_str(new_text);
    }
}

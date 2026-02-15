#![allow(private_interfaces)]

use std::f32::consts::PI;

use avian2d::prelude::{self as p, PhysicsTime as _};
use bevy::app::PluginGroup as _;
use bevy::ecs::change_detection::{DetectChanges, DetectChangesMut as _};
use bevy::ecs::schedule::IntoScheduleConfigs;
use bevy::ecs::spawn::SpawnRelated as _;
use bevy::math::{Vec2, Vec3, Vec3Swizzles as _, vec2, vec3};
use bevy::prelude as b;
use bevy::prelude::StateSet as _;
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

mod pickup;
use pickup::Pickup;

mod rendering;
use rendering::{PLAYFIELD_LAYERS, SCALING_MARGIN, UI_LAYERS, Zees};

mod quantity;
use quantity::{Coherence, Fervor, Fever, Quantity};

use crate::bullets_and_targets::Pattern;
use crate::quantity::UpdateFromQuantity;

// -------------------------------------------------------------------------------------------------

const GAME_NAME: &str = "Interplanetary Recombobulator";

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
        .add_sub_state::<WinOrGameOver>()
        .add_loading_state(
            bevy_asset_loader::loading_state::LoadingState::new(GameState::AssetLoading)
                .continue_to_state(GameState::Menu)
                .load_collection::<Preload>(),
        )
        .add_plugins(bevy_enhanced_input::EnhancedInputPlugin)
        .add_input_context::<Player>()
        .add_input_context::<NonGameInput>()
        .init_resource::<bevy::input_focus::InputFocus>()
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
        .add_systems(b::OnExit(GameState::Menu), start_new_game)
        .add_systems(b::OnExit(GameState::WinOrGameOver), despawn_game)
        .add_systems(b::OnEnter(GameState::Playing), unpause)
        .add_systems(b::OnExit(GameState::Playing), pause)
        .add_observer(pause_unpause_observer)
        .add_systems(
            b::Update,
            // UI systems
            (
                rendering::fit_canvas_to_window_system,
                update_status_text_system,
                button_system,
                set_ui_visibility_from_state,
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
                pickup::pickup_system,
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

const SCREEN_RECT: b::Rect = b::Rect {
    min: vec2(SCREEN_SIZE.x as f32 / -2., SCREEN_SIZE.y as f32 / -2.),
    max: vec2(SCREEN_SIZE.x as f32 / 2., SCREEN_SIZE.y as f32 / 2.),
};
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

#[derive(Clone, Eq, PartialEq, Debug, Hash, Default, b::States)]
enum GameState {
    #[default]
    AssetLoading,

    /// Game not started. Game entities do not exist.
    Menu,

    Playing,

    Paused,

    /// Game entities exist but are frozen.
    WinOrGameOver,
}

#[derive(Clone, Eq, PartialEq, Debug, Hash, Default, b::SubStates)]
#[source(GameState = GameState::WinOrGameOver)]
enum WinOrGameOver {
    #[default]
    GameOver,

    Win,
}

#[derive(Debug, b::Component)]
struct StatusText;

#[derive(Debug, b::Component)]
enum ButtonAction {
    SetState(GameState),
}

#[derive(Debug, b::Component)]
struct VisibleInState(GameState);

/// Entity that is the parent of all entities making up a given Quantity's bar display
#[derive(Debug, b::Component)]
struct BarParent<T>(T);

#[derive(Debug, b::Component)]
struct BarLabelSprite<T>(T);

/// Assets that we use for things spawned after startup.
#[derive(b::Resource, bevy_asset_loader::asset_collection::AssetCollection)]
struct Preload {
    // Enemy assets
    #[asset(path = "enemy.png")]
    enemy_sprite: b::Handle<b::Image>,
    #[asset(path = "enemy-bullet.png")]
    enemy_bullet_sprite: b::Handle<b::Image>,
    #[asset(path = "enemy-fragment.png")]
    enemy_fragment_sprite: b::Handle<b::Image>,
    #[asset(path = "enemy-hurt.ogg")]
    enemy_hurt_sound: b::Handle<b::AudioSource>,
    #[asset(path = "enemy-kill.ogg")]
    enemy_kill_sound: b::Handle<b::AudioSource>,

    // Player assets
    #[asset(path = "player-ship.png")]
    player_ship_sprite: b::Handle<b::Image>,
    #[asset(path = "player-ship-heat.png")]
    player_ship_heat_sprite: b::Handle<b::Image>,
    #[asset(path = "player-bullet.png")]
    player_bullet_sprite: b::Handle<b::Image>,
    #[asset(path = "shoot.ogg")]
    shoot_sound: b::Handle<b::AudioSource>,

    // Pickups
    #[asset(path = "pickup-cool.png")]
    pickup_cool_sprite: b::Handle<b::Image>,
    #[asset(path = "pickup-cohere.png")]
    pickup_cohere_sprite: b::Handle<b::Image>,
    #[asset(path = "pickup.ogg")]
    pickup_sound: b::Handle<b::AudioSource>,

    // UI
    #[asset(path = "Kenney Future.ttf")]
    ui_font: b::Handle<b::Font>,
    #[asset(path = "Kenney Mini Square.ttf")]
    small_prop_font: b::Handle<b::Font>,
    #[asset(path = "Kenney Mini Square Mono.ttf")]
    small_mono_font: b::Handle<b::Font>,
    #[asset(path = "playfield-frame.png")]
    playfield_frame_sprite: b::Handle<b::Image>,
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
    #[asset(path = "text-bar-fervor-inactive.png")]
    text_bar_fervor_inactive_sprite: b::Handle<b::Image>,

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

impl Preload {
    // these methods know what a good font size for pixel matching is
    fn small_prop_font(&self) -> b::TextFont {
        b::TextFont {
            font: self.small_prop_font.clone(),
            font_size: 8.0,
            font_smoothing: bevy::text::FontSmoothing::None,
            ..default()
        }
    }
    fn small_mono_font(&self) -> b::TextFont {
        b::TextFont {
            font: self.small_mono_font.clone(),
            font_size: 8.0,
            font_smoothing: bevy::text::FontSmoothing::None,
            ..default()
        }
    }
}

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

/// Early startup doesn't need assets to be ready ... except for the UI font which we will patch in
fn setup_status_text(mut commands: b::Commands) {
    // Gameplay status text; also used for loading
    commands.spawn((
        StatusText,
        b::Text2d::new(""),
        bevy::text::TextBounds {
            // if we don’t set this, the text wraps undesirably, maybe because it gets changed?
            width: Some(PLAYFIELD_SIZE.x as f32),
            height: None,
        },
        b::TextShadow {
            offset: vec2(1.0, 1.0),
            color: b::Color::BLACK,
        },
        b::TextLayout::new_with_justify(b::Justify::Center),
        bevy::sprite::Anchor::CENTER,
        b::Transform::from_translation(vec3(0.0, 100.0, 0.0)),
        UI_LAYERS,
    ));
}

fn setup_ui(
    mut commands: b::Commands,
    assets: b::Res<Preload>,
    coherence: b::Single<b::Entity, (b::With<Coherence>, b::Without<Fever>, b::Without<Fervor>)>,
    fever: b::Single<b::Entity, (b::With<Fever>, b::Without<Coherence>, b::Without<Fervor>)>,
    fervor: b::Single<b::Entity, (b::With<Fervor>, b::Without<Coherence>, b::Without<Fever>)>,
    status_text: b::Single<b::Entity, b::With<StatusText>>,
) {
    commands.entity(*status_text).insert(b::TextFont {
        font: assets.ui_font.clone(),
        font_size: 26.0,
        font_smoothing: bevy::text::FontSmoothing::None,
        ..default()
    });

    commands.spawn((
        b::Sprite::from_image(assets.playfield_frame_sprite.clone()),
        b::Transform::from_xyz(0., 0., Zees::UiFront.z()),
        UI_LAYERS,
    ));

    commands.spawn(bar_bundle(
        Fever,
        &assets,
        assets.text_bar_fever_sprite.clone(),
        *fever,
        vec2(PLAYFIELD_RECT.min.x - 30.0, PLAYFIELD_RECT.min.y),
        b::Color::srgb_u8(0xFF, 0x42, 0x42),
    ));
    commands.spawn(bar_bundle(
        Coherence,
        &assets,
        assets.text_bar_coherence_sprite.clone(),
        *coherence,
        vec2(PLAYFIELD_RECT.max.x + 30.0, PLAYFIELD_RECT.min.y),
        b::Color::srgb_u8(0xAA, 0xFF, 0x33),
    ));
    commands.spawn(bar_bundle(
        Fervor,
        &assets,
        assets.text_bar_fervor_sprite.clone(),
        *fervor,
        vec2(PLAYFIELD_RECT.max.x + 70.0, PLAYFIELD_RECT.min.y),
        b::Color::srgb_u8(0x55, 0xAA, 0xFF),
    ));

    // New Game button
    commands.spawn((
        b::Node {
            width: b::percent(100),
            height: b::percent(100),
            align_items: b::AlignItems::Center,
            justify_content: b::JustifyContent::Center,
            ..default()
        },
        VisibleInState(GameState::Menu),
        b::children![button_bundle(
            &assets,
            "New Game",
            ButtonAction::SetState(GameState::Playing)
        )],
    ));

    // Back to Menu button for Game Over
    commands.spawn((
        b::Node {
            width: b::percent(100),
            height: b::percent(100),
            align_items: b::AlignItems::Center,
            justify_content: b::JustifyContent::Center,
            ..default()
        },
        VisibleInState(GameState::WinOrGameOver),
        b::children![button_bundle(
            &assets,
            "Menu",
            ButtonAction::SetState(GameState::Menu)
        )],
    ));

    // Unpause button
    commands.spawn((
        b::Node {
            width: b::percent(100),
            height: b::percent(100),
            align_items: b::AlignItems::Center,
            justify_content: b::JustifyContent::Center,
            ..default()
        },
        VisibleInState(GameState::Paused),
        b::children![button_bundle(
            &assets,
            "Resume",
            ButtonAction::SetState(GameState::Playing)
        )],
    ));

    // Help and credits text
    let text_margin = 6.0;
    commands.spawn((
        b::Text2d::new(indoc::indoc! {
            "
                Controls:
                WASD + Space
                ESC to pause
                or use gamepad
            ",
        }),
        assets.small_prop_font(),
        b::TextLayout::new_with_justify(b::Justify::Left),
        bevy::sprite::Anchor::TOP_LEFT,
        b::Transform::from_translation(vec3(
            SCREEN_RECT.min.x + text_margin,
            SCREEN_RECT.max.y - text_margin,
            Zees::UiMiddle.z(),
        )),
        UI_LAYERS,
    ));
    commands.spawn((
        b::Text2d::new(indoc::indoc! {
            "
                Code and art
                by kpreid
                switchb.org/kpreid
                github.com/kpreid
                
                Some fonts
                by Kenney 
                www.kenney.nl

                Made with Bevy
                for Bevy Jam #7
                bevy.org
            ",
        }),
        assets.small_prop_font(),
        b::TextLayout::new_with_justify(b::Justify::Left),
        bevy::sprite::Anchor::BOTTOM_LEFT,
        b::Transform::from_translation(vec3(
            SCREEN_RECT.min.x + text_margin,
            SCREEN_RECT.min.y + text_margin,
            Zees::UiMiddle.z(),
        )),
        UI_LAYERS,
    ));
}

fn button_bundle(assets: &Preload, label: &str, action: ButtonAction) -> impl b::Bundle {
    let text_bundle = (
        b::Text::new(label),
        b::TextFont {
            font: assets.ui_font.clone(),
            font_size: 27.0,
            ..default()
        },
        b::TextLayout::new_with_justify(b::Justify::Left),
        b::TextColor(b::Color::srgb(0.9, 0.9, 0.9)),
        b::TextShadow {
            offset: vec2(1.0, 1.0),
            color: b::Color::BLACK,
        },
    );
    (
        b::Button,
        b::Node {
            width: b::px(240),
            height: b::px(65),
            border: b::UiRect::all(b::px(5)),
            justify_content: b::JustifyContent::Center,
            align_items: b::AlignItems::Center,
            border_radius: b::BorderRadius::MAX,
            ..default()
        },
        action,
        b::BorderColor::all(b::Color::WHITE),
        b::BackgroundColor(NORMAL_BUTTON),
        b::children![text_bundle],
    )
}

/// Build the UI for a [`Quantity`] bar
fn bar_bundle<Marker: Copy + Send + Sync + 'static>(
    marker: Marker,
    assets: &Preload,
    label: b::Handle<b::Image>,
    quantity_entity: b::Entity,
    position: Vec2,
    tint: bevy::color::Color,
) -> impl b::Bundle {
    let percentage_position = vec3(130.0, 8.0, Zees::UiFront2.z());
    let percentage_font = assets.small_mono_font();

    (
        BarParent(marker),
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
                    // TODO: this mode doesn’t do quite what we want when the size is *less* than
                    // 1 repetition. Probably changes to bevy sprites would be needed to fix that.
                    image_mode: b::SpriteImageMode::Tiled {
                        tile_x: true,
                        tile_y: true,
                        stretch_value: 1.0,
                    },
                    color: tint,
                    ..default()
                },
                b::Transform::from_translation(vec3(2.0, 0.0, Zees::UiBack.z())),
                bevy::sprite::Anchor::CENTER_LEFT,
                quantity::UpdateFromQuantity {
                    quantity_entity,
                    property: quantity::UpdateProperty::BaseValue,
                    effect: quantity::UpdateEffect::BarLength,
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
                    color: tint,
                    ..default()
                },
                b::Transform::from_translation(vec3(2.0, 0.0, Zees::UiMiddle.z())),
                bevy::sprite::Anchor::CENTER_LEFT,
                quantity::UpdateFromQuantity {
                    quantity_entity,
                    property: quantity::UpdateProperty::TemporaryValue,
                    effect: quantity::UpdateEffect::BarLength,
                },
                UI_LAYERS,
            ),
            (
                // Fancy label sprite
                BarLabelSprite(marker),
                b::Sprite::from_image(label),
                bevy::sprite::Anchor::CENTER_LEFT,
                b::Transform::from_translation(vec3(10.0, 10.0, Zees::UiFront2.z())),
                UI_LAYERS,
            ),
            (
                // Text for base percentage
                b::Text2d::new(""),
                // b::TextLayout::new_with_justify(b::Justify::Right),
                bevy::sprite::Anchor::BOTTOM_RIGHT,
                percentage_font.clone(),
                quantity::UpdateFromQuantity {
                    quantity_entity,
                    property: quantity::UpdateProperty::BaseValue,
                    effect: quantity::UpdateEffect::TextPercentage
                },
                b::Transform::from_translation(percentage_position),
                UI_LAYERS,
            ),
            (
                // Text for temporary percentage
                b::Text2d::new(""),
                // b::TextLayout::new_with_justify(b::Justify::Right),
                bevy::sprite::Anchor::BOTTOM_RIGHT,
                percentage_font,
                quantity::UpdateFromQuantity {
                    quantity_entity,
                    property: quantity::UpdateProperty::TemporaryStack,
                    effect: quantity::UpdateEffect::TextPercentage
                },
                b::Transform::from_translation(percentage_position + vec3(30.0, 0.0, 0.0)),
                UI_LAYERS,
            )
        ],
        b::Visibility::Hidden,
        quantity::UpdateFromQuantity {
            quantity_entity,
            property: quantity::UpdateProperty::TemporaryValue,
            effect: quantity::UpdateEffect::VisibleIfEverNotZero,
        },
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
    fever_query: b::Single<
        (b::Entity, &mut Quantity),
        (b::With<Fever>, b::Without<Coherence>, b::Without<Fervor>),
    >,
    mut fervor: b::Single<
        &mut Quantity,
        (b::With<Fervor>, b::Without<Coherence>, b::Without<Fever>),
    >,

    bars_to_hide: b::Query<
        &mut b::Visibility,
        b::Or<(b::With<BarParent<Coherence>>, b::With<BarParent<Fervor>>)>,
    >,
) {
    let (fever_q_entity, mut fever) = fever_query.into_inner();
    **coherence = Quantity::new(Coherence::INITIAL);
    *fever = Quantity::new(Fever::INITIAL);
    **fervor = Quantity::new(Fervor::INITIAL);

    // Reset sticky visibility of bars
    for mut bar_vis in bars_to_hide {
        *bar_vis = b::Visibility::Hidden;
    }

    commands.spawn((
        Player,
        Team::Player,
        bullets_and_targets::Attackable {
            // any health below the max translates into fever increase via player_health_is_fever_system()
            health: u8::MAX,
            hurt_animation_cooldown: 0.0,
            destruction_particle: None, // TODO: add one
        },
        b::Transform::from_xyz(0., PLAYFIELD_RECT.min.y + 20.0, 0.0),
        PLAYFIELD_LAYERS,
        b::Visibility::Visible,
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
            base_cooldown: 0.25,
            trigger: false,
            pattern: Pattern::Coherent,
        },
        b::children![
            (
                b::Sprite::from_image(assets.player_ship_sprite.clone()),
                b::Transform::from_xyz(0., 0., Zees::Player.z()),
            ),
            (
                b::Sprite::from_image(assets.player_ship_heat_sprite.clone()),
                b::Transform::from_xyz(0., 0., Zees::AbovePlayer.z()),
                UpdateFromQuantity {
                    quantity_entity: fever_q_entity,
                    property: quantity::UpdateProperty::TemporaryValue,
                    effect: quantity::UpdateEffect::Opacity,
                },
            )
        ],
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
    let delta_position = movement * 200.0 * time.delta_secs(); // apply speed
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
    (*next_state).set_if_neq(match *state.get() {
        GameState::AssetLoading => return,
        GameState::Playing => GameState::Paused,
        GameState::Paused | GameState::Menu => GameState::Playing,
        GameState::WinOrGameOver => GameState::Menu,
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
        p::Collider::circle(1.0), // TODO: use a simple movement system w/o physics so as not to exercise collision
        p::LinearVelocity(velocity),
        Lifetime(20.0), // TODO: would be more efficient to detect when the sprite is off the screen
    )
}

// -------------------------------------------------------------------------------------------------

fn update_status_text_system(
    state: b::Res<b::State<GameState>>,
    wog_state: Option<b::Res<b::State<WinOrGameOver>>>,
    mut text: b::Single<&mut b::Text2d, b::With<StatusText>>,
) {
    let new_text = match *state.get() {
        GameState::AssetLoading => "Loading",
        GameState::Menu => GAME_NAME,
        GameState::WinOrGameOver => match *wog_state.unwrap().get() {
            WinOrGameOver::GameOver => "Game Overheated",
            WinOrGameOver::Win => "Win",
        },
        GameState::Playing => "",
        GameState::Paused => "Paused",
    };

    if ***text != new_text {
        text.clear();
        text.push_str(new_text);
    }
}

// based off of https://bevy.org/examples/ui-user-interface/button/
const NORMAL_BUTTON: b::Color = b::Color::srgb(0.15, 0.15, 0.15);
const HOVERED_BUTTON: b::Color = b::Color::srgb(0.5, 0.25, 0.25);
const PRESSED_BUTTON: b::Color = b::Color::srgb(0.75, 0.75, 0.35);

/// based off of https://bevy.org/examples/ui-user-interface/button/
fn button_system(
    //mut commands: b::Commands,
    mut input_focus: b::ResMut<bevy::input_focus::InputFocus>,
    mut interaction_query: b::Query<
        (
            b::Entity,
            Option<&ButtonAction>,
            &b::Interaction,
            &mut b::BackgroundColor,
            &mut b::Button,
        ),
        b::Changed<b::Interaction>,
    >,
    // state: b::Res<b::State<GameState>>,
    mut next_state: b::ResMut<b::NextState<GameState>>,
) {
    for (entity, action, interaction, mut color, mut button) in &mut interaction_query {
        match *interaction {
            b::Interaction::Pressed => {
                input_focus.set(entity);
                *color = PRESSED_BUTTON.into();
                button.set_changed();

                // TODO: would be better if this went through the same kind of path as key bindings
                match action {
                    Some(ButtonAction::SetState(state)) => {
                        next_state.set(state.clone());
                    }
                    None => b::warn!("Button {entity:?} has no action"),
                }
            }
            b::Interaction::Hovered => {
                input_focus.set(entity);
                *color = HOVERED_BUTTON.into();
                button.set_changed();
            }
            b::Interaction::None => {
                input_focus.clear();
                *color = NORMAL_BUTTON.into();
                button.set_changed();
            }
        }
    }
}

fn set_ui_visibility_from_state(
    entities: b::Query<(&mut b::Visibility, &VisibleInState)>,
    state: b::Res<b::State<GameState>>,
) {
    if !state.is_changed() {
        return;
    }
    // currently, just hides all buttons only in the new game state
    for (mut visibility, expected_state) in entities {
        *visibility = if expected_state.0 == **state {
            b::Visibility::Inherited
        } else {
            b::Visibility::Hidden
        };
    }
}

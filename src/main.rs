use std::f32::consts::PI;

use avian2d::prelude as p;
use bevy::app::PluginGroup as _;
use bevy::camera::visibility::RenderLayers;
use bevy::ecs::spawn::SpawnRelated as _;
use bevy::math::{Vec2, Vec3Swizzles as _, ivec2, vec2};
use bevy::prelude as b;
use bevy::render::render_resource::{
    Extent3d, TextureDescriptor, TextureDimension, TextureFormat, TextureUsages,
};
use bevy::utils::default;
use bevy_enhanced_input::prelude as bei;
use bevy_enhanced_input::prelude::InputContextAppExt as _;
use rand::RngExt;

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
        .add_plugins(avian2d::prelude::PhysicsDebugPlugin::default())
        .add_systems(b::Startup, (setup_camera, setup_gameplay))
        .add_systems(b::Update, fit_canvas_to_window)
        .add_systems(b::FixedUpdate, apply_movement)
        .add_systems(b::FixedUpdate, expire_lifetimes)
        .add_systems(b::FixedUpdate, gun_cooldown)
        .add_observer(shoot)
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

const SCALING_MARGIN: u32 = 10;

const PIXEL_LAYERS: RenderLayers = RenderLayers::layer(0);
const HIGH_RES_LAYERS: RenderLayers = RenderLayers::layer(1);

/// Z position values for sprites for when disambiguation is needed
enum Zees {
    Bullets = -1,
    Player = 0,
    Frame = 1,
}
impl Zees {
    fn z(self) -> f32 {
        self as i32 as f32
    }
}

// -------------------------------------------------------------------------------------------------

/// Player ship entity
#[derive(Debug, b::Component)]
#[require(b::Transform)]
struct Player;

#[derive(Debug, b::Component)]
struct PlayerBullet;

#[derive(Debug, b::Component)]
struct Gun {
    /// If positive, gun may not shoot.
    cooldown: f32,
}

/// Decremented by game time and despawns the entity when it is zero
#[derive(Debug, b::Component)]
struct Lifetime(f32);

// -------------------------------------------------------------------------------------------------
// Rendering-related components
// “Pixel perfect” setup per <https://github.com/bevyengine/bevy/blob/release-0.18.1/examples/2d/pixel_grid_snap.rs>

/// Low-resolution texture that contains the pixel-perfect world.
/// Canvas itself is rendered to the high-resolution world.
#[derive(b::Component)]
struct Canvas;

/// Camera that renders the pixel-perfect world to the [`Canvas`].
#[derive(b::Component)]
struct InGameCamera;

/// Camera that renders the [`Canvas`] (and other graphics on [`HIGH_RES_LAYERS`]) to the screen.
#[derive(b::Component)]
struct OuterCamera;

// -------------------------------------------------------------------------------------------------

#[derive(Debug, bei::InputAction)]
#[action_output(b::Vec2)]
struct Move;

#[derive(Debug, bei::InputAction)]
#[action_output(bool)]
struct Shoot;

// -------------------------------------------------------------------------------------------------

fn setup_camera(mut commands: b::Commands, mut images: b::ResMut<b::Assets<b::Image>>) {
    // “Pixel perfect” setup per <https://github.com/bevyengine/bevy/blob/release-0.18.1/examples/2d/pixel_grid_snap.rs>

    let canvas_size = Extent3d {
        width: SCREEN_SIZE.x,
        height: SCREEN_SIZE.y,
        ..default()
    };
    let mut canvas = b::Image {
        texture_descriptor: TextureDescriptor {
            label: Some("canvas"),
            size: canvas_size,
            dimension: TextureDimension::D2,
            format: TextureFormat::Bgra8UnormSrgb,
            mip_level_count: 1,
            sample_count: 1,
            usage: TextureUsages::TEXTURE_BINDING
                | TextureUsages::COPY_DST
                | TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        },
        ..default()
    };

    // Fill image.data with zeroes
    canvas.resize(canvas_size);

    let image_handle = images.add(canvas);

    // This camera renders whatever is on `PIXEL_PERFECT_LAYERS` to the canvas
    commands.spawn((
        b::Camera2d,
        b::Camera {
            // Render before the "main pass" camera
            order: -1,
            clear_color: b::ClearColorConfig::Custom(bevy::color::palettes::css::GRAY.into()),
            ..default()
        },
        bevy::camera::RenderTarget::Image(image_handle.clone().into()),
        b::Msaa::Off,
        InGameCamera,
        PIXEL_LAYERS,
    ));

    commands.spawn((b::Sprite::from_image(image_handle), Canvas, HIGH_RES_LAYERS));
    commands.spawn((b::Camera2d, b::Msaa::Off, OuterCamera, HIGH_RES_LAYERS));

    // Spatial audio listener (*not* attached to the player ship)
    commands.spawn((
        b::SpatialListener::new(2.0),
        // for some reason it seems we need to reverse left-right
        b::Transform::from_rotation(b::Quat::from_rotation_y(PI)),
    ));
}

/// Scales camera projection to fit the window (integer multiples only).
fn fit_canvas_to_window(
    mut resize_messages: b::MessageReader<bevy::window::WindowResized>,
    windows: b::Query<&b::Window>,
    mut projection: b::Single<&mut b::Projection, b::With<OuterCamera>>,
) -> b::Result {
    let b::Projection::Orthographic(projection) = &mut **projection else {
        return Err(b::BevyError::from("projection not orthographic"));
    };
    for window_resized in resize_messages.read() {
        // need physical size because that's what Camera2D relates to
        let window = windows.get(window_resized.window)?;
        let size = window.physical_size();
        let h_scale = (size.x - SCALING_MARGIN) / SCREEN_SIZE.x;
        let v_scale = (size.y - SCALING_MARGIN) / SCREEN_SIZE.y;
        projection.scale = window.scale_factor() / (h_scale.min(v_scale).max(1) as f32);
    }
    Ok(())
}

fn setup_gameplay(mut commands: b::Commands, asset_server: b::Res<b::AssetServer>) {
    // player sprite
    let player_sprite_asset = asset_server.load("player.png");
    commands.spawn((
        Player,
        b::Transform::from_xyz(0., PLAYFIELD_RECT.min.y + 20.0, Zees::Player.z()),
        b::Sprite::from_image(player_sprite_asset.clone()),
        PIXEL_LAYERS,
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

    commands.spawn((
        b::Sprite::from_image(asset_server.load("playfield-frame.png")),
        b::Transform::from_xyz(0., 0., Zees::Frame.z()),
    ));
}

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

fn shoot(
    _shoot: b::On<bei::Fire<Shoot>>,
    mut commands: b::Commands,
    gun_query: b::Query<(&b::Transform, &mut Gun)>,
    asset_server: b::Res<b::AssetServer>,
) -> b::Result {
    let (player_transform, mut gun) = gun_query.single_inner()?;

    let mut origin_of_bullets_transform: b::Transform = *player_transform;
    origin_of_bullets_transform.translation.z = Zees::Bullets.z();

    if gun.cooldown != 0.0 {
        return Ok(());
    }

    for bullet_angle_deg in (-15..=15).step_by(5) {
        let bullet_angle_rad = (bullet_angle_deg as f32).to_radians();

        let speed = rand::rng().random_range(500.0..=1000.0);
        commands.spawn((
            PlayerBullet,
            Lifetime(0.4),
            b::Sprite::from_image(asset_server.load("player-bullet.png")),
            PIXEL_LAYERS,
            p::RigidBody::Kinematic,
            p::LinearVelocity(Vec2::from_angle(bullet_angle_rad).rotate(vec2(0.0, speed))),
            p::Collider::rectangle(4., 8.),
            origin_of_bullets_transform
                * b::Transform::from_rotation(b::Quat::from_rotation_z(bullet_angle_rad)),
        ));
    }
    commands.spawn((
        b::AudioPlayer::new(asset_server.load("fire.ogg")),
        b::PlaybackSettings {
            spatial: true,
            volume: bevy::audio::Volume::Decibels(-10.),
            speed: rand::rng().random_range(0.5..=1.5),
            ..b::PlaybackSettings::DESPAWN
        },
        origin_of_bullets_transform,
    ));

    gun.cooldown = 0.25;

    Ok(())
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

fn gun_cooldown(time: b::Res<b::Time>, query: b::Query<&mut Gun>) {
    let delta = time.delta_secs();
    for mut gun in query {
        let new_cooldown = (gun.cooldown - delta).max(0.0);
        if new_cooldown != gun.cooldown {
            gun.cooldown = new_cooldown;
        }
    }
}

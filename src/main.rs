use std::f32::consts::PI;

use avian2d::prelude as p;
use bevy::app::PluginGroup as _;
use bevy::camera::visibility::RenderLayers;
use bevy::color::Alpha as _;
use bevy::ecs::schedule::IntoScheduleConfigs as _;
use bevy::ecs::spawn::SpawnRelated as _;
use bevy::math::{Vec2, Vec3Swizzles as _, ivec2, vec2, vec3};
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
        //.add_plugins(avian2d::prelude::PhysicsDebugPlugin::default())
        .add_systems(b::Startup, (setup_camera, setup_gameplay, setup_ui).chain())
        .add_systems(b::Update, fit_canvas_to_window)
        .add_systems(b::FixedUpdate, apply_movement)
        .add_systems(b::FixedUpdate, expire_lifetimes)
        .add_systems(b::FixedUpdate, gun_cooldown)
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
    UiElement = 1,
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
            ),
            (
                b::Text2d::new(label),
                b::TextLayout::new_with_justify(b::Justify::Left),
                bevy::sprite::Anchor::CENTER_LEFT,
                b::Transform::from_translation(vec3(0.0, 20.0, 0.0))
            )
        ],
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

    commands.spawn((Coherence, Quantity { value: 0.5 }));
    commands.spawn((Fever, Quantity { value: 0.5 }));
    commands.spawn((Fervor, Quantity { value: 0.0 }));
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

fn shoot(
    _shoot: b::On<bei::Fire<Shoot>>,
    mut commands: b::Commands,
    gun_query: b::Query<(&b::Transform, &mut Gun)>,
    coherence_query: b::Single<&Quantity, (b::With<Coherence>, b::Without<Fever>)>,
    mut fever_query: b::Single<&mut Quantity, b::With<Fever>>,
    asset_server: b::Res<b::AssetServer>,
) -> b::Result {
    let (player_transform, mut gun) = gun_query.single_inner()?;

    if gun.cooldown != 0.0 {
        return Ok(());
    }

    let mut origin_of_bullets_transform: b::Transform = *player_transform;
    origin_of_bullets_transform.translation.z = Zees::Bullets.z();

    let coherence = coherence_query.value;

    let bullet_scale = vec2(1.0, 1.0 + coherence.powi(2) * 10.0);
    let base_bullet_speed = 800.0 + coherence.powi(2) * 10.0;
    let bullet_angle_step_rad = (1.0 - coherence) * 5f32.to_radians();

    for bullet_angle_index in -3..=3 {
        let bullet_angle_rad = bullet_angle_index as f32 * bullet_angle_step_rad;

        let speed = rand::rng().random_range(0.5..=1.0) * base_bullet_speed;
        commands.spawn((
            PlayerBullet,
            Lifetime(0.4),
            b::Sprite::from_image(asset_server.load("player-bullet.png")),
            PIXEL_LAYERS,
            p::RigidBody::Kinematic,
            p::LinearVelocity(Vec2::from_angle(bullet_angle_rad).rotate(vec2(0.0, speed))),
            // constants are sprite size
            p::Collider::rectangle(4. * bullet_scale.x, 8. * bullet_scale.y),
            origin_of_bullets_transform
                * b::Transform {
                    rotation: b::Quat::from_rotation_z(bullet_angle_rad),
                    scale: bullet_scale.extend(1.0),
                    ..default()
                },
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

    // Side effects of firing besides a bullet.
    gun.cooldown = 0.25;
    fever_query.value = (fever_query.value + 0.1 * coherence).clamp(0.0, 1.0);

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

// -------------------------------------------------------------------------------------------------

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
    mut pixel_camera: b::Single<&mut b::Camera, b::With<InGameCamera>>,
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
        bar_transform.scale = vec3(10.0 * quantity, 1.0, 1.0);
    }
    Ok(())
}

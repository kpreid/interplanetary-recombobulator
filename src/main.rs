use avian2d::prelude as p;
use bevy::ecs::schedule::IntoScheduleConfigs as _;
use bevy::ecs::spawn::SpawnRelated as _;
use bevy::math::{ivec2, vec2};
use bevy::prelude as b;
use bevy::utils::default;
use bevy_enhanced_input::prelude as bei;
use bevy_enhanced_input::prelude::InputContextAppExt as _;

// -------------------------------------------------------------------------------------------------

fn main() {
    b::App::new()
        .add_plugins(b::DefaultPlugins)
        .add_plugins(bevy_enhanced_input::EnhancedInputPlugin)
        .add_input_context::<Player>()
        .add_plugins(avian2d::PhysicsPlugins::default())
        .add_plugins(avian2d::prelude::PhysicsDebugPlugin::default())
        //.add_plugins(player::player_plugin)
        .add_systems(b::Startup, setup)
        .add_systems(b::FixedUpdate, apply_movement)
        .add_systems(b::FixedUpdate, expire_lifetimes)
        .add_observer(shoot)
        .run();
}

// -------------------------------------------------------------------------------------------------

/// Player ship entity
#[derive(Debug, b::Component)]
#[require(b::Transform)]
struct Player;

#[derive(Debug, b::Component)]
struct PlayerBullet;

/// Decremented by game time and despawns the entity when it is zero
#[derive(Debug, b::Component)]
struct Lifetime(f32);

// -------------------------------------------------------------------------------------------------

#[derive(Debug, bei::InputAction)]
#[action_output(b::Vec2)]
struct Move;

#[derive(Debug, bei::InputAction)]
#[action_output(bool)]
struct Shoot;

// -------------------------------------------------------------------------------------------------

fn setup(
    mut commands: b::Commands,
    mut windows: b::Query<&mut b::Window>,
    asset_server: b::Res<b::AssetServer>,
) {
    eprintln!("setup");

    // reposition window for development
    windows.single_mut().unwrap().position = b::WindowPosition::At(ivec2(3000, 0));

    // camera
    commands.spawn(b::Camera2d::default());

    // player sprite
    let player_sprite_asset = asset_server.load("player.png");
    commands.spawn((
        Player,
        b::Transform::from_xyz(0., 0., 0.),
        b::Sprite::from_image(player_sprite_asset.clone()),
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
    ));

    // test second sprite
    // commands.spawn((
    //     b::Sprite::from_image(player_sprite_asset.clone()),
    //     b::Transform::from_xyz(10., 0., 0.),
    // ));
}

fn apply_movement(
    action: b::Single<&bei::Action<Move>>,
    player_query: b::Query<&mut b::Transform, b::With<Player>>,
) -> b::Result {
    let movement: b::Vec2 = ***action;
    let movement = movement * 10.0;
    for mut transform in player_query {
        transform.translation.x += movement.x;
        transform.translation.y += movement.y;
    }
    Ok(())
}

fn shoot(
    _shoot: b::On<bei::Fire<Shoot>>,
    mut commands: b::Commands,
    player_query: b::Query<&b::Transform, b::With<Player>>,
    asset_server: b::Res<b::AssetServer>,
) -> b::Result {
    let player_transform: b::Transform = *player_query.single()?;

    commands.spawn((
        PlayerBullet,
        Lifetime(0.2),
        b::Sprite::from_image(asset_server.load("player-bullet.png")),
        p::RigidBody::Kinematic,
        p::LinearVelocity(vec2(0.0, 800.0)),
        p::Collider::rectangle(4., 8.),
        player_transform,
    ));

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

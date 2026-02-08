use avian2d::prelude as p;
use bevy::ecs::schedule::IntoScheduleConfigs as _;
use bevy::ecs::spawn::SpawnRelated as _;
use bevy::math::{Vec3, ivec2, vec3};
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
        // .add_plugins(avian2d::PhysicsPlugins::default())
        //.add_plugins(avian2d::prelude::PhysicsDebugPlugin::default())
        //.add_plugins(player::player_plugin)
        .add_systems(b::Startup, setup)
        .add_systems(b::FixedUpdate, apply_movement)
        .run();
}

// -------------------------------------------------------------------------------------------------

#[derive(Debug, b::Component)]
#[require(b::Transform)]
struct Player;

#[derive(Debug, bei::InputAction)]
#[action_output(b::Vec2)]
struct Move;

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
            // (
            //     bei::Action::<Fire>::new(),
            //     bei::bindings![MouseButton::Left, GamepadButton::RightTrigger2],
            // ),
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

use bevy::ecs::schedule::IntoScheduleConfigs as _;
use bevy::math::{Vec3, ivec2, vec3};
use bevy::prelude as b;
use bevy::utils::default;

fn main() {
    b::App::new()
        .add_plugins(b::DefaultPlugins)
        .add_plugins(bevy_enhanced_input::EnhancedInputPlugin)
        .add_plugins(avian2d::PhysicsPlugins::default())
        //.add_plugins(avian2d::prelude::PhysicsDebugPlugin::default())
        //.add_plugins(player::player_plugin)
        .add_systems(b::Startup, setup)
        .run();
}

fn setup(
    mut commands: b::Commands,
    mut windows: b::Query<&mut b::Window>,
    asset_server: b::Res<b::AssetServer>,
) {
    eprintln!("setup");

    // reposition window for development
    windows.single_mut().unwrap().position = b::WindowPosition::At(ivec2(3000, 0));

    // camera
    commands.spawn((::Camera2d::default());

    // player sprite
    commands.spawn(b::Sprite::from_image(asset_server.load("player.png")));
}

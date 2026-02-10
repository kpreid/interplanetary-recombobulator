use std::f32::consts::PI;

use bevy::camera::visibility::RenderLayers;
use bevy::prelude as b;
use bevy::render::render_resource::{
    Extent3d, TextureDescriptor, TextureDimension, TextureFormat, TextureUsages,
};
use bevy::utils::default;

use crate::{PLAYFIELD_SIZE, SCREEN_SIZE};

// -------------------------------------------------------------------------------------------------

/// We pretend the window is this many pixels smaller when computing the scale factor,
/// to avoid the visual effect of sometimes just barely touching the window border
pub(crate) const SCALING_MARGIN: u32 = 10;

pub(crate) const PLAYFIELD_LAYERS: RenderLayers = RenderLayers::layer(0);
pub(crate) const UI_LAYERS: RenderLayers = RenderLayers::layer(1);
pub(crate) const HIGH_RES_LAYERS: RenderLayers = RenderLayers::layer(2);

/// Z position values for sprites for when disambiguation may be needed
pub(crate) enum Zees {
    Starfield = -3,
    Pickup = -2,
    Bullets = -1,
    Enemy = 0,
    Player = 1,
    UiElement = 2,
}
impl Zees {
    pub fn z(self) -> f32 {
        self as i32 as f32
    }
}

// -------------------------------------------------------------------------------------------------
// Rendering-related components
// “Pixel perfect” setup per <https://github.com/bevyengine/bevy/blob/release-0.18.1/examples/2d/pixel_grid_snap.rs>

/// Low-resolution texture that contains the pixel-perfect world.
/// Canvas itself is rendered to the high-resolution world.
#[derive(b::Component)]
struct Canvas;

/// Camera that renders the gameplay objects to the [`Canvas`].
/// Has a restricted viewport to crop objects.
#[derive(b::Component)]
pub(crate) struct PlayfieldCamera;

/// Camera that renders the UI objects to the [`Canvas`].
#[derive(b::Component)]
pub(crate) struct UiCamera;

/// Camera that renders the [`Canvas`] (and other graphics on [`HIGH_RES_LAYERS`]) to the screen.
#[derive(b::Component)]
pub(crate) struct OuterCamera;

// -------------------------------------------------------------------------------------------------

pub(crate) fn setup_camera_system(
    mut commands: b::Commands,
    mut images: b::ResMut<b::Assets<b::Image>>,
) {
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

    let pixel_camera_image_handle = images.add(canvas);

    commands.spawn((
        b::Camera2d,
        b::Camera {
            // Render before the "main pass" camera and before the UI too
            order: -2,
            clear_color: b::ClearColorConfig::Custom(bevy::color::palettes::css::GRAY.into()),
            viewport: Some(bevy::camera::Viewport {
                physical_position: (SCREEN_SIZE - PLAYFIELD_SIZE) / 2,
                physical_size: PLAYFIELD_SIZE,
                ..default()
            }),
            ..default()
        },
        bevy::camera::RenderTarget::Image(pixel_camera_image_handle.clone().into()),
        b::Msaa::Off,
        PlayfieldCamera,
        PLAYFIELD_LAYERS,
    ));

    commands.spawn((
        b::Camera2d,
        b::Camera {
            // Render before the "main pass" camera
            order: -1,
            clear_color: b::ClearColorConfig::None,
            ..default()
        },
        bevy::camera::RenderTarget::Image(pixel_camera_image_handle.clone().into()),
        b::Msaa::Off,
        UiCamera,
        UI_LAYERS,
    ));

    commands.spawn((
        b::Sprite::from_image(pixel_camera_image_handle),
        Canvas,
        HIGH_RES_LAYERS,
    ));
    commands.spawn((b::Camera2d, b::Msaa::Off, OuterCamera, HIGH_RES_LAYERS));

    // Spatial audio listener (*not* attached to the player ship)
    commands.spawn((
        b::SpatialListener::new(2.0),
        // for some reason it seems we need to reverse left-right
        b::Transform::from_rotation(b::Quat::from_rotation_y(PI)),
    ));
}

/// Scales camera projection to fit the window (integer multiples only).
pub(crate) fn fit_canvas_to_window_system(
    mut resize_messages: b::MessageReader<bevy::window::WindowResized>,
    windows: b::Query<&b::Window>,
    mut projection: b::Single<&mut b::Projection, b::With<OuterCamera>>,
) -> b::Result {
    let b::Projection::Orthographic(projection) = &mut **projection else {
        return Err(b::BevyError::from("projection not orthographic"));
    };
    for window_resized in resize_messages.read() {
        let window = windows.get(window_resized.window)?;
        let margin = if let bevy::window::WindowMode::Windowed = window.mode {
            SCALING_MARGIN
        } else {
            0
        };

        // compute scale factor in physical pixels
        let size = window.physical_size();
        let h_scale = (size.x - margin) / SCREEN_SIZE.x;
        let v_scale = (size.y - margin) / SCREEN_SIZE.y;

        projection.scale = window.scale_factor() / (h_scale.min(v_scale).max(1) as f32);
    }
    Ok(())
}

use bevy::math::vec3;
use bevy::prelude as b;

use crate::SCREEN_SIZE;
use crate::rendering::PlayfieldCamera;

// -------------------------------------------------------------------------------------------------

/// A value between 0 and 1 that is displayed to the player as a bar.
/// Other components on this entity define which quantity it is and how systems affect it.
#[derive(Debug, b::Component)]
pub(crate) struct Quantity {
    pub value: f32,
}

/// [`Quantity`] 1/3; affects shooting.
#[derive(Debug, b::Component)]
pub(crate) struct Coherence;

/// [`Quantity`] 2/3; maxing it is game over.
#[derive(Debug, b::Component)]
pub(crate) struct Fever;

/// [`Quantity`] 3/3; maxing it is a win.
#[derive(Debug, b::Component)]
pub(crate) struct Fervor;

/// Specifies a [`Quantity`] this entity should update its visual appearance (e.g. bar length) from.
#[derive(Debug, b::Component)]
pub(crate) struct UpdateFromQuantity(pub b::Entity);

// -------------------------------------------------------------------------------------------------

#[expect(unused_variables)]
pub(crate) fn quantity_behaviors_system(
    coherence: b::Single<
        &mut Quantity,
        (b::With<Coherence>, b::Without<Fever>, b::Without<Fervor>),
    >,
    fever: b::Single<&mut Quantity, (b::With<Fever>, b::Without<Coherence>, b::Without<Fervor>)>,
    fervor: b::Single<&mut Quantity, (b::With<Fervor>, b::Without<Coherence>, b::Without<Fever>)>,
) -> b::Result {
    // TODO: handle interactions between quantities
    Ok(())
}

pub(crate) fn update_quantity_display_system_1(
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

pub(crate) fn update_quantity_display_system_2(
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

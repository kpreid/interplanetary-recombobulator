use bevy::math::vec2;
use bevy::prelude as b;

use crate::GameState;
use crate::rendering::PlayfieldCamera;

// -------------------------------------------------------------------------------------------------

/// A value between 0 and 1 that is displayed to the player as a bar.
/// Other components on this entity define which quantity it is and how systems affect it.
#[derive(Debug, b::Component)]
pub(crate) struct Quantity {
    /// Base value of the quantity, persisting unless changed.
    value: f32,
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

// These constants are each the initial value of their corresponding `Quantity`
impl Coherence {
    pub const INITIAL: f32 = 1.0;
}
impl Fever {
    pub const INITIAL: f32 = 0.5;
}
impl Fervor {
    pub const INITIAL: f32 = 0.0;
}

// -------------------------------------------------------------------------------------------------

impl Quantity {
    pub fn new(value: f32) -> Self {
        Self { value }
    }

    pub fn adjust(&mut self, delta: f32) {
        self.set_clamped(self.value + delta);
    }

    pub fn set_clamped(&mut self, value: f32) {
        if value.is_nan() {
            if cfg!(debug_assertions) {
                panic!("NaN value");
            } else {
                return;
            }
        }
        self.value = value.clamp(0.0, 1.0);
    }

    /// Value which should apply to gameplay effects.
    pub fn effective_value(&self) -> f32 {
        self.value
    }
}

// -------------------------------------------------------------------------------------------------

#[expect(unused_variables)]
pub(crate) fn quantity_behaviors_system(
    coherence: b::Single<
        &mut Quantity,
        (b::With<Coherence>, b::Without<Fever>, b::Without<Fervor>),
    >,
    fever: b::Single<&mut Quantity, (b::With<Fever>, b::Without<Coherence>, b::Without<Fervor>)>,
    fervor: b::Single<&mut Quantity, (b::With<Fervor>, b::Without<Coherence>, b::Without<Fever>)>,
    mut next_state: b::ResMut<b::NextState<GameState>>,
) -> b::Result {
    // TODO: handle interactions between quantities

    if fever.value == 1.0 {
        next_state.set_if_neq(GameState::GameOver);
    }

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
    bars_to_update: b::Query<(&mut b::Sprite, &UpdateFromQuantity)>,
) -> b::Result {
    for (mut sprite, ufq) in bars_to_update {
        let quantity = quantities.get(ufq.0)?.value;
        sprite.custom_size = Some(vec2(459.0 * quantity, 16.0));
    }
    Ok(())
}

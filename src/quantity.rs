use core::fmt::Write as _;
use std::f64::consts::PI;

use bevy::ecs::change_detection::DetectChangesMut;
use bevy::math::vec2;
use bevy::prelude as b;

use crate::rendering::PlayfieldCamera;
use crate::{GameState, WinOrGameOver};

// -------------------------------------------------------------------------------------------------

/// A value between 0 and 1 that is displayed to the player as a bar.
/// Other components on this entity define which quantity it is and how systems affect it.
#[derive(Debug, b::Component)]
pub(crate) struct Quantity {
    /// Base value of the quantity, persisting unless changed.
    base: f32,

    /// An increase which becomes permanent if another increase is applied before this is removed.
    /// How removals happen depend on the specific quantity.
    temporary_stack: f32,
}

/// [`Quantity`] 1/3; affects shooting.
#[derive(Clone, Copy, Debug, b::Component)]
pub(crate) struct Coherence;

/// [`Quantity`] 2/3; maxing it is game over.
#[derive(Clone, Copy, Debug, b::Component)]
pub(crate) struct Fever;

/// [`Quantity`] 3/3; maxing it is a win.
#[derive(Clone, Copy, Debug, b::Component)]
pub(crate) struct Fervor;

/// Specifies a [`Quantity`] this entity should update its visual appearance (e.g. bar length) from.
/// Does not specify what type of update should be performed.
#[derive(Debug, b::Component)]
pub(crate) struct UpdateFromQuantity {
    pub quantity_entity: b::Entity,
    pub property: UpdateProperty,
    pub effect: UpdateEffect,
}

#[derive(Debug)]
pub(crate) enum UpdateProperty {
    BaseValue,
    TemporaryValue,
    TemporaryStack,
}
#[derive(Debug)]
pub(crate) enum UpdateEffect {
    BarLength,
    Opacity,
    PulsingOpacity,
    VisibleIfEverNotZero,
    TextPercentage,
}

// These constants are each the initial value of their corresponding `Quantity`
impl Coherence {
    pub const INITIAL: f32 = 0.0;
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
        Self {
            base: value,
            temporary_stack: 0.0,
        }
    }

    pub fn adjust_permanent_including_temporary(&mut self, delta: f32) {
        self.reset_to(self.effective_value() + delta);
    }

    pub fn adjust_permanent_clearing_temporary(&mut self, delta: f32) {
        self.reset_to(self.base + delta);
    }

    pub fn adjust_permanent_keeping_temporary(&mut self, delta: f32) {
        self.set_base_only(self.base + delta);
    }

    pub fn adjust_permanent_keeping_temporary_absolutely(&mut self, delta: f32) {
        self.set_base_only(self.base + delta);
        self.temporary_stack -= delta;
    }

    pub fn adjust_temporary_and_commit_previous_temporary(&mut self, delta: f32) {
        self.reset_to(self.effective_value());
        self.temporary_stack = delta;
    }

    pub fn adjust_temporary_stacking_with_previous(&mut self, delta: f32) {
        self.temporary_stack += delta;
    }

    /// Set the value and erase any temporary modifications
    pub fn reset_to(&mut self, value: f32) {
        self.set_base_only(value);
        self.temporary_stack = 0.0;
    }

    fn set_base_only(&mut self, value: f32) {
        if value.is_nan() {
            if cfg!(debug_assertions) {
                panic!("NaN value");
            } else {
                return;
            }
        }
        self.base = value.clamp(0.0, 1.0);
    }

    /// Value which should apply to gameplay effects (usually).
    pub fn effective_value(&self) -> f32 {
        self.unclamped_effective_value().clamp(0.0, 1.0)
    }

    pub fn unclamped_effective_value(&self) -> f32 {
        self.base + self.temporary_stack
    }

    pub fn temporary_stack(&self) -> f32 {
        self.temporary_stack
    }
}

// -------------------------------------------------------------------------------------------------

pub(crate) fn quantity_behaviors_system(
    time: b::Res<b::Time>,
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
    mut next_state: b::ResMut<b::NextState<GameState>>,
    mut next_wog_state: b::ResMut<b::NextState<WinOrGameOver>>,
) -> b::Result {
    // Win and lose conditions.
    // Side effects of these will be handled by OnEnter `crate::end_of_game_effects()`
    if fever.effective_value() == 1.0 {
        (*next_state).set_if_neq(GameState::WinOrGameOver);
        next_wog_state.set(WinOrGameOver::GameOver);
    } else if fervor.base >= 0.999 {
        (*next_state).set_if_neq(GameState::WinOrGameOver);
        next_wog_state.set(WinOrGameOver::Win);
    }

    // Loss of coherence becomes permanent if not removed
    {
        let coherence_change = coherence.temporary_stack * (2.0f32.powf(time.delta_secs()) - 1.0);
        coherence.adjust_permanent_keeping_temporary_absolutely(coherence_change);
    }

    // Excess fever goes away if not committed
    fever.temporary_stack *= 0.3f32.powf(time.delta_secs());

    // Fervor's permanent value moves towards its temporary value
    {
        let change = fervor.temporary_stack * 0.3f32 * time.delta_secs();
        fervor.adjust_permanent_keeping_temporary_absolutely(change);
    }

    // Temporary fervor goes down linearly until it hits a most-negative value of -0.1 or the base
    // value, whichever is higher.
    fervor.temporary_stack = (fervor.temporary_stack - 0.06f32 * time.delta_secs())
        .max(-0.1)
        .max(-fervor.base);

    Ok(())
}

/// Updates display in quantity-specific ways
pub(crate) fn update_quantity_display_system_1(
    assets: Option<b::Res<crate::MyAssets>>,
    fever: b::Single<&Quantity, b::With<Fever>>,
    coherence: b::Single<&Quantity, b::With<Coherence>>,
    // fervor: b::Single<&Quantity, b::With<Fervor>>,
    mut fervor_label_sprite: b::Single<&mut b::Sprite, b::With<crate::BarLabelSprite<Fervor>>>,
    mut pixel_camera: b::Single<&mut b::Camera, b::With<PlayfieldCamera>>,
) -> b::Result {
    pixel_camera.clear_color = bevy::camera::ClearColorConfig::Custom(b::Color::oklch(
        fever.effective_value() * 0.05,
        fever.effective_value(),
        0.0,
    ));

    if let Some(assets) = assets {
        let image = if fervor_is_active(&fever, &coherence) {
            &assets.text_bar_fervor_sprite
        } else {
            &assets.text_bar_fervor_inactive_sprite
        };
        if fervor_label_sprite.image != *image {
            fervor_label_sprite.image = image.clone();
        }
    }

    Ok(())
}

/// Updates sprites from quantities as specified by [`UpdateFromQuantity`] components.
/// The main job of this system is to update the bars.
pub(crate) fn update_quantity_display_system_2(
    time: b::Res<b::Time>,
    quantities: b::Query<&Quantity>,
    sprites_to_update: b::Query<(
        Option<&mut b::Sprite>,
        Option<&mut b::Visibility>,
        Option<&mut b::Text2d>,
        &UpdateFromQuantity,
    )>,
) -> b::Result {
    for (sprite, visibility, text, ufq) in sprites_to_update {
        let quantity: &Quantity = quantities.get(ufq.quantity_entity)?;
        let value = match ufq.property {
            UpdateProperty::BaseValue => quantity.base,
            UpdateProperty::TemporaryStack => quantity.temporary_stack,
            // allow temp values > 1.0 to spill over the bar frame
            UpdateProperty::TemporaryValue => quantity.unclamped_effective_value().max(0.0),
        };
        match ufq.effect {
            UpdateEffect::BarLength => {
                let length_scale = 459.0;
                let width = 16.0;
                sprite
                    .expect("need sprite component for BarLength")
                    .custom_size = Some(vec2(length_scale * value, width));
            }
            UpdateEffect::Opacity => {
                sprite.expect("need sprite component for Opacity").color =
                    b::Color::LinearRgba(b::LinearRgba::new(1.0, 1.0, 1.0, value));
            }
            UpdateEffect::PulsingOpacity => {
                let pulsing_value = (value
                    * (1.0 + (time.elapsed_secs_f64() * PI * 3.0).sin() as f32 * 0.4))
                    .clamp(0.0, 1.0);
                sprite.expect("need sprite component for Opacity").color =
                    b::Color::LinearRgba(b::LinearRgba::new(1.0, 1.0, 1.0, pulsing_value));
            }
            UpdateEffect::VisibleIfEverNotZero => {
                let mut visibility =
                    visibility.expect("need visibility component for VisibleIfNotZero");
                if value > 0.0 {
                    visibility.set_if_neq(b::Visibility::Visible);
                }
            }
            UpdateEffect::TextPercentage => {
                let text: &mut String =
                    &mut text.expect("need text component for TextPercentage").0;
                text.clear();
                if let UpdateProperty::TemporaryStack = ufq.property {
                    if value.abs() > 0.001 {
                        write!(text, "{:+2}%", (value * 100.0).round()).unwrap();
                    }
                } else {
                    write!(text, "{:2}%", (value * 100.0).round()).unwrap();
                }
            }
        }
    }
    Ok(())
}

pub(crate) fn fervor_is_active(fever: &Quantity, coherence: &Quantity) -> bool {
    coherence.effective_value() > fever.effective_value()
}

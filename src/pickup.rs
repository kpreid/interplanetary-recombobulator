use avian2d::prelude::{self as p};
use bevy::math::{Vec2, vec2};
use bevy::prelude as b;

use crate::bullets_and_targets::Hurt;
use crate::quantity::{Coherence, Fervor, Fever, Quantity};
use crate::rendering::{PLAYFIELD_LAYERS, Zees};
use crate::{Lifetime, Player};

// -------------------------------------------------------------------------------------------------

/// On colliding with [`Player`], has an effect and despawns the entity.
/// This is used for both pickups and colliding with enemies.
#[derive(Debug, b::Component)]
pub(crate) enum Pickup {
    /// Does nothing.
    /// Immediately vanishes.
    /// Used as a placeholder when a pickup bundle is required, but not wanted for gameplay.
    Null,

    /// Increase [`Fever`] by this amount, and depict it as a damaging hit.
    Damage(f32),
    /// Decrease [`Fever`] by this amount.
    Cool(f32),
    /// Increase [`Coherence`] by this amount.
    Cohere(f32),
}

/// Category of [`Pickup`] to spawn.
/// Determines the exact value and appearance using its internal logic.
#[derive(Clone, Copy, Debug)]
pub(crate) enum PickupSpawnType {
    /// Invisible and immediately vanishes.
    /// Used as a placeholder when a pickup bundle is required, but not wanted for gameplay.
    Null,

    Cool,

    Cohere,
}

// -------------------------------------------------------------------------------------------------

impl PickupSpawnType {
    pub(crate) fn pickup_bundle(&self, assets: &crate::MyAssets, position: Vec2) -> impl b::Bundle {
        let image = match self {
            PickupSpawnType::Null => &assets.pickup_cool_sprite,
            PickupSpawnType::Cool => &assets.pickup_cool_sprite,
            PickupSpawnType::Cohere => &assets.pickup_cohere_sprite,
        };

        let effect = match self {
            PickupSpawnType::Null => Pickup::Null,
            PickupSpawnType::Cool => Pickup::Cool(0.1),
            PickupSpawnType::Cohere => Pickup::Cohere(0.1),
        };

        // kludge to make null have no visible effect
        let visibility = match self {
            PickupSpawnType::Null => b::Visibility::Hidden,
            _ => b::Visibility::Visible,
        };

        // This bundle contains the parts of the pickup that exist while it is being carried
        // by an enemy. The parts for its independent existence will be added when it drops
        // from the enemy by after_drop_bundle().
        (
            b::Sprite::from_image(image.clone()),
            effect,
            visibility,
            b::Transform::from_translation(position.extend(Zees::Pickup.z())),
            PLAYFIELD_LAYERS,
        )
    }
}

/// Bundle of components to add to a [`Pickup`] entity when it stops being carried by an enemy
/// and starts existing on its own.
pub(crate) fn after_drop_bundle(pickup: &Pickup) -> impl b::Bundle {
    (
        Lifetime(match pickup {
            Pickup::Null => 0.0, // go away immediately
            _ => 20.0,           // TODO: bad substitute for "die when offscreen"
        }),
        p::RigidBody::Kinematic,
        p::Collider::circle(8.), // a bit oversized to make it easier to collect
        p::LinearVelocity(vec2(0.0, -100.0)),
        p::AngularVelocity(0.6),
    )
}

// -------------------------------------------------------------------------------------------------

pub(crate) fn pickup_system(
    mut commands: b::Commands,
    player_query: b::Single<(b::Entity, &p::CollidingEntities), b::With<Player>>,
    pickups: b::Query<(&Pickup, &b::Transform)>,
    mut coherence: b::Single<
        &mut Quantity,
        (b::With<Coherence>, b::Without<Fever>, b::Without<Fervor>),
    >,
    mut fever: b::Single<
        &mut Quantity,
        (b::With<Fever>, b::Without<Coherence>, b::Without<Fervor>),
    >,
    assets: b::Res<crate::MyAssets>,
) -> b::Result {
    let (player_entity, player_collisions) = player_query.into_inner();
    for &pickup_entity in &player_collisions.0 {
        let Ok((pickup, &pickup_transform)) = pickups.get(pickup_entity) else {
            // not a pickup
            continue;
        };

        let mut sound_asset = None;

        match *pickup {
            Pickup::Null => {}
            Pickup::Damage(amount) => {
                fever.adjust_permanent_including_temporary(amount);
                commands.trigger(Hurt(player_entity));
            }
            Pickup::Cool(amount) => {
                fever.adjust_permanent_clearing_temporary(-amount);
                sound_asset = Some(assets.pickup_sound.clone());
            }
            Pickup::Cohere(amount) => {
                coherence.adjust_permanent_clearing_temporary(amount);
                sound_asset = Some(assets.pickup_sound.clone());
            }
        }

        commands.entity(pickup_entity).despawn();

        if let Some(sound_asset) = sound_asset {
            commands.spawn((
                b::AudioPlayer::new(sound_asset),
                b::PlaybackSettings {
                    spatial: true,
                    volume: bevy::audio::Volume::Decibels(-10.),
                    ..b::PlaybackSettings::DESPAWN
                },
                pickup_transform,
            ));
        }
    }
    Ok(())
}

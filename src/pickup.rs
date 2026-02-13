use avian2d::prelude::{self as p};
use bevy::math::{Vec2, vec2};
use bevy::prelude as b;

use crate::bullets_and_targets::Attackable;
use crate::quantity::{Coherence, Fervor, Fever, Quantity};
use crate::rendering::{PLAYFIELD_LAYERS, Zees};
use crate::{Lifetime, Player};

// -------------------------------------------------------------------------------------------------

/// On colliding with [`Player`], has an effect and despawns the entity.
/// This is used for both pickups and colliding with enemies.
#[derive(Debug, b::Component)]
pub(crate) enum Pickup {
    /// Increase [`Fever`] by this amount, and depict it as a damaging hit.
    Damage(f32),
    /// Decrease [`Fever`] by this amount.
    Cool(f32),
    /// Increase [`Coherence`] by this amount.
    Cohere(f32),
}

// -------------------------------------------------------------------------------------------------

pub(crate) fn pickup_bundle(assets: &crate::Preload, position: Vec2) -> impl b::Bundle {
    (
        b::Sprite::from_image(assets.pickup_cool_sprite.clone()),
        Pickup::Cool(0.1),
        Lifetime(20.0), // TODO: bad substitute for "die when offscreen"
        b::Transform::from_translation(position.extend(Zees::Pickup.z())),
        PLAYFIELD_LAYERS,
        p::RigidBody::Kinematic,
        p::Collider::circle(5.),
        p::LinearVelocity(vec2(0.0, -70.0)),
        p::AngularVelocity(0.6),
    )
}

// -------------------------------------------------------------------------------------------------

pub(crate) fn pickup_system(
    mut commands: b::Commands,
    player_query: b::Single<(&p::CollidingEntities, &mut Attackable), b::With<Player>>,
    pickups: b::Query<(&Pickup, &b::Transform)>,
    mut coherence: b::Single<
        &mut Quantity,
        (b::With<Coherence>, b::Without<Fever>, b::Without<Fervor>),
    >,
    mut fever: b::Single<
        &mut Quantity,
        (b::With<Fever>, b::Without<Coherence>, b::Without<Fervor>),
    >,
    assets: b::Res<crate::Preload>,
) -> b::Result {
    let (player_collisions, mut player_attackable) = player_query.into_inner();
    for &pickup_entity in &player_collisions.0 {
        let Ok((pickup, &pickup_transform)) = pickups.get(pickup_entity) else {
            // not a pickup
            continue;
        };

        let sound_asset;

        match *pickup {
            Pickup::Damage(amount) => {
                fever.adjust_permanent_including_temporary(amount);

                player_attackable.hurt_flash();

                sound_asset = Some(assets.enemy_hurt_sound.clone()); // TODO: separate player hurt 
            }
            Pickup::Cool(amount) => {
                fever.adjust_permanent_ignoring_temporary(-amount);
                sound_asset = Some(assets.pickup_sound.clone());
            }
            Pickup::Cohere(amount) => {
                coherence.adjust_permanent_ignoring_temporary(-amount);
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

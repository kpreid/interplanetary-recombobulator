use avian2d::prelude as p;
use bevy::ecs::entity::EntityHashSet;
use bevy::math::{Vec2, vec2};
use bevy::prelude as b;
use bevy::utils::default;
use bevy_enhanced_input::prelude as bei;
use rand::RngExt;

use crate::{Coherence, Fever, Lifetime, PLAYFIELD_LAYERS, Player, Quantity, Shoot, Zees};

// -------------------------------------------------------------------------------------------------

#[derive(Debug, b::Component)]
pub(crate) struct PlayerBullet;

/// Something that dies if shot.
#[derive(Debug, b::Component)]
pub(crate) struct Attackable {
    pub health: u8,
}

/// This entity has a gun! It might be the player ship or an enemy ship.
#[derive(Debug, b::Component)]
pub(crate) struct Gun {
    /// If positive, gun may not shoot yet.
    pub cooldown: f32,
}

// -------------------------------------------------------------------------------------------------

/// Shoot the gun if the button is pressed.
/// Note that this is an input observer, not a system function
pub(crate) fn shoot(
    _shoot: b::On<bei::Fire<Shoot>>,
    mut commands: b::Commands,
    gun_query: b::Query<(&b::Transform, &mut Gun), b::With<Player>>,
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
    let base_bullet_speed = 400.0 + coherence.powi(2) * 10.0;
    let bullet_angle_step_rad = (1.0 - coherence) * 5f32.to_radians();

    for bullet_angle_index in -3..=3 {
        let bullet_angle_rad = bullet_angle_index as f32 * bullet_angle_step_rad;

        let speed = rand::rng().random_range(0.5..=1.0) * base_bullet_speed;
        commands.spawn((
            PlayerBullet,
            Lifetime(0.4),
            b::Sprite::from_image(asset_server.load("player-bullet.png")),
            PLAYFIELD_LAYERS,
            p::RigidBody::Kinematic,
            p::LinearVelocity(Vec2::from_angle(bullet_angle_rad).rotate(vec2(0.0, speed))),
            // constants are sprite size
            p::Collider::rectangle(4. * bullet_scale.x, 8. * bullet_scale.y),
            p::CollidingEntities::default(), // for dealing damage
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
    fever_query.adjust(0.1 * coherence);

    Ok(())
}

// -------------------------------------------------------------------------------------------------

pub(crate) fn gun_cooldown(time: b::Res<b::Time>, query: b::Query<&mut Gun>) {
    let delta = time.delta_secs();
    for mut gun in query {
        let new_cooldown = (gun.cooldown - delta).max(0.0);
        if new_cooldown != gun.cooldown {
            gun.cooldown = new_cooldown;
        }
    }
}

pub(crate) fn bullet_hit_system(
    mut commands: b::Commands,
    bullet_query: b::Query<(b::Entity, &p::CollidingEntities), b::With<PlayerBullet>>,
    mut target_query: b::Query<&mut Attackable>,
) -> b::Result {
    let mut killed = EntityHashSet::new();
    'bullet: for (bullet_entity, collisions) in bullet_query {
        'colliding: for &colliding_entity in &collisions.0 {
            if killed.contains(&colliding_entity) {
                // already killed but not yet despawned; skip
                continue 'colliding;
            }
            let Ok(mut attackable) = target_query.get_mut(colliding_entity) else {
                // collided but is not attackable
                continue 'colliding;
            };

            let new_health = attackable.health.saturating_sub(1);

            if new_health == 0 {
                commands.entity(colliding_entity).despawn();
                killed.insert(colliding_entity);
            } else {
                attackable.health = new_health;
            }
            commands.entity(bullet_entity).despawn();
            continue 'bullet; // each bullet hits only one entity
        }
    }
    Ok(())
}

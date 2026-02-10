use avian2d::prelude as p;
use bevy::ecs::entity::EntityHashSet;
use bevy::math::{Vec2, vec2, vec3};
use bevy::prelude as b;
use bevy::utils::default;
use bevy_enhanced_input::prelude as bei;
use rand::RngExt;

use crate::{Coherence, Fever, Lifetime, PLAYFIELD_LAYERS, Pickup, Player, Quantity, Shoot, Zees};

// -------------------------------------------------------------------------------------------------

#[derive(Debug, b::Component)]
pub(crate) struct PlayerBullet;

/// Something that dies if shot.
#[derive(Debug, b::Component)]
pub(crate) struct Attackable {
    /// Reduced by bullets, and when zero, this is despawned.
    pub health: u8,

    /// If successfully killed, spawns beneficial [`crate::Pickup`]s.
    /// (this may be more than a bool later)
    pub drops: bool,
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
    time: b::Res<b::Time>,
    gun_query: b::Query<(&b::Transform, &mut Gun), b::With<Player>>,
    coherence_query: b::Single<&Quantity, (b::With<Coherence>, b::Without<Fever>)>,
    mut fever_query: b::Single<&mut Quantity, b::With<Fever>>,
    assets: b::Res<crate::Preload>,
    images: b::Res<b::Assets<b::Image>>,
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

    let sprite_size = images
        .get(&assets.player_bullet_sprite)
        .ok_or_else(|| b::BevyError::from("asset not loaded"))?
        .size_f32();
    let bullet_box_size = sprite_size * bullet_scale;

    for bullet_angle_index in -3..=3 {
        let bullet_angle_rad = bullet_angle_index as f32 * bullet_angle_step_rad;

        let speed = rand::rng().random_range(0.5..=1.0) * base_bullet_speed;
        commands.spawn((
            PlayerBullet,
            Lifetime(0.4),
            b::Sprite::from_image(assets.player_bullet_sprite.clone()),
            PLAYFIELD_LAYERS,
            p::RigidBody::Kinematic,
            p::LinearVelocity(Vec2::from_angle(bullet_angle_rad).rotate(vec2(0.0, speed))),
            p::Collider::ellipse(sprite_size.x / 2., sprite_size.y / 2.),
            p::CollidingEntities::default(), // for dealing damage
            origin_of_bullets_transform
                * b::Transform::from_rotation(b::Quat::from_rotation_z(bullet_angle_rad))
                * b::Transform::from_translation(vec3(0.0, bullet_box_size.y / 2., 0.0))
                * b::Transform::from_scale(bullet_scale.extend(1.0)),
        ));
    }

    commands.spawn((
        b::AudioPlayer::new(assets.shoot_sound.clone()),
        b::PlaybackSettings {
            spatial: true,
            volume: bevy::audio::Volume::Decibels(-10.),
            speed: rand::rng().random_range(0.75..=1.25) + coherence.powi(2) * 2.0,
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
    bullet_query: b::Query<
        (b::Entity, &p::CollidingEntities, &mut Lifetime),
        b::With<PlayerBullet>,
    >,
    mut target_query: b::Query<(&mut Attackable, &b::Transform)>,
    assets: b::Res<crate::Preload>,
) -> b::Result {
    let mut killed = EntityHashSet::new();
    'bullet: for (bullet_entity, collisions, mut bullet_lifetime) in bullet_query {
        if bullet_lifetime.0 == 0.0 {
            // this bullet may have already hit something and is expiring
            continue 'bullet;
        }
        'colliding: for &colliding_entity in &collisions.0 {
            if killed.contains(&colliding_entity) {
                // already killed but not yet despawned; skip
                continue 'colliding;
            }
            let Ok((mut attackable, &attackable_transform)) =
                target_query.get_mut(colliding_entity)
            else {
                // collided but is not attackable
                continue 'colliding;
            };

            let new_health = attackable.health.saturating_sub(1);
            let is_killed = new_health == 0;

            if !is_killed {
                attackable.health = new_health;
            } else {
                killed.insert(colliding_entity);
                commands.entity(colliding_entity).despawn();

                // Spawn a pickup if we should
                if attackable.drops {
                    commands.spawn((
                        b::Sprite::from_image(assets.pickup_cool_sprite.clone()),
                        Pickup::Cool(0.1),
                        b::Transform::from_translation(
                            attackable_transform.translation.with_z(Zees::Pickup.z()),
                        ),
                        PLAYFIELD_LAYERS,
                        p::RigidBody::Kinematic,
                        p::Collider::circle(5.),
                        p::LinearVelocity(vec2(0.0, -70.0)),
                        p::AngularVelocity(0.6),
                    ));
                }
            }

            // Play death or hurt sound
            commands.spawn((
                b::AudioPlayer::new(
                    if is_killed {
                        &assets.enemy_kill_sound
                    } else {
                        &assets.enemy_hurt_sound
                    }
                    .clone(),
                ),
                b::PlaybackSettings {
                    spatial: true,
                    volume: bevy::audio::Volume::Decibels(-10.),
                    ..b::PlaybackSettings::DESPAWN
                },
                attackable_transform,
            ));

            // each bullet hits at most one entity and dies
            //commands.entity(bullet_entity).despawn();
            bullet_lifetime.0 = 0.0; // cause bullet to die on the next frame for visual purposes
            continue 'bullet; // don't hit anything else
        }
    }
    Ok(())
}

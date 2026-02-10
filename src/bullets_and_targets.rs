use std::f32::consts::PI;

use avian2d::prelude as p;
use bevy::ecs::entity::EntityHashSet;
use bevy::math::{Vec2, vec2, vec3};
use bevy::prelude as b;
use bevy_enhanced_input::prelude as bei;
use rand::RngExt;

use crate::{
    Coherence, Fever, Lifetime, PLAYFIELD_LAYERS, Pickup, Player, Quantity, Shoot, Team, Zees,
};

// -------------------------------------------------------------------------------------------------

/// Entity is a bullet and does bullet things such as hurting enemies.
#[derive(Debug, b::Component)]
#[require(p::CollidingEntities)]
pub(crate) struct Bullet;

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
    /// Gun will shoot next time [`fire_gun_system`] runs, if possible.
    pub trigger: bool,

    pub pattern: Pattern,

    /// If positive, gun may not shoot yet.
    pub cooldown: f32,

    /// Value `cooldown` is reset to after firing.
    pub base_cooldown: f32,
}

#[derive(Debug)]
pub(crate) enum Pattern {
    /// Fire a single, slow bullet.
    Single,
    /// Shotgun-to-laser depending on [`Coherence`].
    Coherent,
}

// -------------------------------------------------------------------------------------------------

/// Note that this is an input observer, not a system function
pub(crate) fn player_input_fire_gun(
    _shoot: b::On<bei::Fire<Shoot>>,
    gun_query: b::Query<&mut Gun, b::With<Player>>,
) {
    // fire button can be pressed this many seconds in advance
    const EARLY_TRIGGER_WINDOW: f32 = 0.05;

    for mut gun in gun_query {
        if gun.cooldown <= EARLY_TRIGGER_WINDOW {
            gun.trigger = true;
        } else {
            // ignore fire button when gun is not close enough to ready to fire
        }
    }
}

/// Spawn bullets if [`Gun::trigger`] is true.
pub(crate) fn fire_gun_system(
    mut commands: b::Commands,
    gun_query: b::Query<(&b::Transform, &mut Gun, &Team, b::Has<Player>)>,
    coherence_query: b::Single<&Quantity, (b::With<Coherence>, b::Without<Fever>)>,
    mut fever_query: b::Single<&mut Quantity, b::With<Fever>>,
    assets: b::Res<crate::Preload>,
    images: b::Res<b::Assets<b::Image>>,
) -> b::Result {
    for (gun_transform, mut gun, &team, is_player) in gun_query {
        if !gun.trigger || gun.cooldown != 0.0 {
            // Gun is not commanded to fire or is not ready to fire
            continue;
        }
        gun.trigger = false;

        let mut origin_of_bullets_transform: b::Transform = *gun_transform;
        origin_of_bullets_transform.translation.z = Zees::Bullets.z();

        let base_shooting_angle = match team {
            Team::Player => 0.0,
            Team::Enemy => PI,
        };

        // 1 + 2 * spread_count is the number of bullets
        let (coherence, spread_count): (f32, i32) = match gun.pattern {
            Pattern::Single => (0.0, 0),
            Pattern::Coherent => (coherence_query.value, 3),
        };

        let base_bullet_speed = 400.0 + coherence.powi(2) * 20000.0;
        let bullet_angle_step_rad = (1.0 - coherence) * 5f32.to_radians();
        // bullets scaled so that they overlap themselves from frame to frame,
        // for both reliable collisions and for good visuals.
        let bullet_scale = vec2(1.0, (base_bullet_speed * 0.003).max(1.0));

        let sprite_size = images
            .get(&assets.player_bullet_sprite)
            .ok_or_else(|| b::BevyError::from("asset not loaded"))?
            .size_f32();
        let bullet_box_size = sprite_size * bullet_scale;

        for bullet_angle_index in -spread_count..=spread_count {
            let bullet_angle_rad =
                base_shooting_angle + bullet_angle_index as f32 * bullet_angle_step_rad;
            let speed = rand::rng().random_range(0.5..=1.0) * base_bullet_speed;
            let bullet_transform = origin_of_bullets_transform
                * b::Transform::from_rotation(b::Quat::from_rotation_z(bullet_angle_rad))
                * b::Transform::from_translation(vec3(0.0, bullet_box_size.y / 2., 0.0))
                * b::Transform::from_scale(bullet_scale.extend(1.0));

            commands.spawn((
                Bullet,
                team,
                Lifetime(0.4),
                b::Sprite::from_image(
                    match team {
                        Team::Player => &assets.player_bullet_sprite,
                        Team::Enemy => &assets.enemy_bullet_sprite,
                    }
                    .clone(),
                ),
                PLAYFIELD_LAYERS,
                p::RigidBody::Kinematic,
                p::LinearVelocity(Vec2::from_angle(bullet_angle_rad).rotate(vec2(0.0, speed))),
                p::Collider::ellipse(sprite_size.x / 2., sprite_size.y / 2.),
                p::CollidingEntities::default(), // for dealing damage
                bullet_transform
                    * if bullet_angle_rad.cos() < 0.0 {
                        // don't rotate sprite more than ±90° so the highlight is good
                        b::Transform::from_rotation(b::Quat::from_rotation_z(PI))
                    } else {
                        b::Transform::IDENTITY
                    },
            ));

            // Muzzle flash sprite is transformed exactly like the bullet, but does not move forward.
            // This helps avoid fast bullets look disconnected.
            commands.spawn((
                Lifetime(0.04),
                b::Sprite::from_image(assets.muzzle_flash_sprite.clone()),
                PLAYFIELD_LAYERS,
                bullet_transform,
            ));
        }

        commands.spawn((
            b::AudioPlayer::new(assets.shoot_sound.clone()),
            b::PlaybackSettings {
                spatial: true,
                volume: match team {
                    Team::Player => bevy::audio::Volume::Decibels(-10.),
                    Team::Enemy => bevy::audio::Volume::Decibels(-30.),
                },
                speed: rand::rng().random_range(0.75..=1.25) + coherence.powi(2) * 2.0,
                ..b::PlaybackSettings::DESPAWN
            },
            origin_of_bullets_transform,
        ));

        // Side effects of firing besides a bullet.
        gun.cooldown = gun.base_cooldown;
        if is_player {
            fever_query.adjust(0.1 * coherence);
        }
    }

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
    bullet_query: b::Query<(&Team, &p::CollidingEntities, &mut Lifetime), b::With<Bullet>>,
    mut target_query: b::Query<(&Team, &mut Attackable, &b::Transform)>,
    assets: b::Res<crate::Preload>,
) -> b::Result {
    let mut killed = EntityHashSet::new();
    'bullet: for (bullet_team, collisions, mut bullet_lifetime) in bullet_query {
        // Note that a bullet may hit multiple targets and kill them if its collider
        // is large enough. This is on purpose to make high Coherence shots more effective.

        'colliding: for &colliding_entity in &collisions.0 {
            let Ok((&target_team, mut attackable, &attackable_transform)) =
                target_query.get_mut(colliding_entity)
            else {
                // collided but is not attackable
                continue 'colliding;
            };

            if !bullet_team.should_hurt(target_team) {
                continue 'colliding;
            }

            if killed.contains(&colliding_entity) {
                // already killed but not yet despawned; skip
                continue 'colliding;
            }

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

            bullet_lifetime.0 = 0.0; // cause bullet to die on the next frame
        }
    }
    Ok(())
}

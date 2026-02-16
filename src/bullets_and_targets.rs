use std::f32::consts::PI;

use avian2d::prelude as p;
use bevy::ecs::entity::EntityHashSet;
use bevy::math::{Vec2, Vec3Swizzles as _, vec2, vec3};
use bevy::prelude as b;
use bevy_enhanced_input::prelude as bei;
use rand::RngExt;
use rand_distr::Distribution as _;

use crate::pickup::Pickup;
use crate::quantity::fervor_is_active;
use crate::{
    Coherence, Fervor, Fever, Lifetime, PLAYFIELD_LAYERS, Player, Quantity, Shoot, Team, Zees,
};

// -------------------------------------------------------------------------------------------------

/// Entity is a bullet and does bullet things such as hurting enemies.
#[derive(Debug, b::Component)]
#[require(p::CollidingEntities)]
pub(crate) struct Bullet {
    damage: u8,
}

/// Something that dies if shot.
#[derive(Debug, b::Component)]
pub(crate) struct Attackable {
    /// Reduced by bullets, and when zero, this is despawned.
    pub health: u8,

    /// Set to 1.0 when damage occurs, and decays to 0.0.
    pub hurt_animation_cooldown: f32,

    pub hurt_sound: b::Handle<b::AudioSource>,

    pub destruction_particle: Option<b::Handle<b::Image>>,

    /// What team last hit it, to attribute the kill.
    pub last_hit_by: Option<Team>,
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

    pub shoot_sound: (b::Handle<b::AudioSource>, bevy::audio::Volume),
}

#[derive(Debug)]
pub(crate) enum Pattern {
    /// Fire a single, slow bullet.
    Single,
    /// Shotgun-to-laser depending on [`Coherence`].
    Coherent,
}

/// Event triggered whenever an [`Attackable`] takes damage, by the system making the health change.
#[derive(Debug, b::Event)]
pub(crate) struct Hurt(pub b::Entity);

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
    mut coherence_query: b::Single<&mut Quantity, (b::With<Coherence>, b::Without<Fever>)>,
    mut fever_query: b::Single<&mut Quantity, b::With<Fever>>,
    assets: b::Res<crate::MyAssets>,
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

        let (base_shooting_angle, unmodified_bullet_speed) = match team {
            Team::Player => (0.0, 400.0),
            Team::Enemy => (PI, 300.0),
        };

        // 1 + 2 * spread_count is the number of bullets
        let (coherence, spread_count): (f32, i32) = match gun.pattern {
            Pattern::Single => (0.0, 0),
            Pattern::Coherent => (coherence_query.effective_value(), 3),
        };

        let bullet_speed_with_boost = unmodified_bullet_speed + coherence.powi(2) * 20000.0;
        let bullet_angle_step_rad = (1.0 - coherence * 0.9) * 5f32.to_radians();
        // bullets scaled so that they overlap themselves from frame to frame,
        // for both reliable collisions and for good visuals.
        let bullet_scale = vec2(1.0, (bullet_speed_with_boost * 0.003).max(1.0));

        let sprite_size = images
            .get(&assets.player_bullet_sprite)
            .ok_or_else(|| b::BevyError::from("asset not loaded"))?
            .size_f32();
        let bullet_box_size = sprite_size * bullet_scale;

        for bullet_angle_index in -spread_count..=spread_count {
            let bullet_angle_rad =
                base_shooting_angle + bullet_angle_index as f32 * bullet_angle_step_rad;
            let single_speed = rand::rng().random_range(0.5..=1.0) * bullet_speed_with_boost;
            let bullet_transform = origin_of_bullets_transform
                * b::Transform::from_rotation(b::Quat::from_rotation_z(bullet_angle_rad))
                * b::Transform::from_translation(vec3(0.0, bullet_box_size.y / 2., 0.0))
                * b::Transform::from_scale(bullet_scale.extend(1.0));

            commands.spawn((
                Bullet {
                    damage: match gun.pattern {
                        Pattern::Single => 1,
                        // if coherence is high, add bonus damage
                        Pattern::Coherent => 1 + (coherence * 2.9).floor() as u8,
                    },
                },
                team,
                Lifetime(match team {
                    Team::Player => 2.0,
                    Team::Enemy => 10.0, // can cross the whole screen
                }),
                b::Sprite::from_image(
                    match team {
                        Team::Player => &assets.player_bullet_sprite,
                        Team::Enemy => &assets.enemy_bullet_sprite,
                    }
                    .clone(),
                ),
                PLAYFIELD_LAYERS,
                p::RigidBody::Kinematic,
                p::LinearVelocity(
                    Vec2::from_angle(bullet_angle_rad).rotate(vec2(0.0, single_speed)),
                ),
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

        let (ref shoot_sound, volume) = gun.shoot_sound;
        commands.spawn((
            b::AudioPlayer::new(shoot_sound.clone()),
            b::PlaybackSettings {
                spatial: true,
                volume,
                speed: rand::rng().random_range(0.75..=1.25) + coherence.powi(2) * 2.0,
                ..b::PlaybackSettings::DESPAWN
            },
            origin_of_bullets_transform,
        ));

        // Side effects of firing besides a bullet.
        gun.cooldown = gun.base_cooldown;
        if is_player {
            // Shooting with high coherence adds temporary fever, which must be mitigated by not
            // shooting too frequently
            fever_query.adjust_temporary_and_commit_previous_temporary(0.1 * coherence);

            // Shooting decreases coherence, which must be mitigated by not missing
            coherence_query.adjust_temporary_stacking_with_previous(-0.1);
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
    bullet_query: b::Query<(&Bullet, &Team, &p::CollidingEntities, &mut Lifetime)>,
    mut target_query: b::Query<
        (
            // Note: Beware requiring components here!
            // Every required component becomes a condition for attackability!
            &Team,
            &mut Attackable,
        ),
        b::Without<b::ChildOf>,
    >,
    mut coherence_query: b::Single<
        &mut Quantity,
        (b::With<Coherence>, b::Without<Fever>, b::Without<Fervor>),
    >,
) -> b::Result {
    let mut killed = EntityHashSet::new();
    for (bullet, &bullet_team, collisions, mut bullet_lifetime) in bullet_query {
        // Note that a bullet may hit multiple targets and kill them if its collider
        // is large enough. This is on purpose to make high Coherence shots more effective.

        'colliding: for &colliding_entity in &collisions.0 {
            let Ok((&target_team, mut target_attackable)) = target_query.get_mut(colliding_entity)
            else {
                // collided but is not attackable
                // b::warn!("collided with {colliding_entity} but is not attackable");
                continue 'colliding;
            };

            if !bullet_team.should_hurt(target_team) {
                continue 'colliding;
            }

            if killed.contains(&colliding_entity) {
                // already killed but not yet despawned; skip
                continue 'colliding;
            }

            let new_health = target_attackable.health.saturating_sub(bullet.damage);
            let is_killed = new_health == 0;

            target_attackable.last_hit_by = Some(bullet_team);
            target_attackable.health = new_health;
            commands.trigger(Hurt(colliding_entity));

            if is_killed {
                killed.insert(colliding_entity);
            }

            bullet_lifetime.0 = 0.0; // cause bullet to die on the next frame

            // Player successfully hitting *something* cancels coherence loss.
            if bullet_team == Team::Player {
                coherence_query.adjust_permanent_clearing_temporary(0.0);
            }
        }
    }
    Ok(())
}

/// Despawns [`Attackable`]s with health of 0, and produces side effects such as drops.
pub(crate) fn death_system(
    mut commands: b::Commands,
    attackable_query: b::Query<
        (
            // Note: Beware requiring components here!
            // Every required component becomes a condition!
            b::Entity,
            &mut Attackable,
            &b::Transform,
            Option<&p::LinearVelocity>,
            Option<&b::Children>,
        ),
        (b::Changed<Attackable>, b::Without<b::ChildOf>),
    >,
    fever_query: b::Single<&Quantity, b::With<Fever>>,
    coherence_query: b::Single<&Quantity, b::With<Coherence>>,
    mut fervor_query: b::Single<
        &mut Quantity,
        (b::With<Fervor>, b::Without<Coherence>, b::Without<Fever>),
    >,
    mut children_to_drop_query: b::Query<
        (&b::GlobalTransform, &mut b::Transform, &Pickup),
        (
            b::With<b::ChildOf>,
            b::Without<Bullet>,
            b::Without<Quantity>,
        ),
    >,
) {
    let rng = &mut rand::rng();

    for (dying_entity, dying_attackable, &dying_transform, dying_velocity, children_of_dying) in
        attackable_query
    {
        if dying_attackable.health > 0 {
            // not dying
            continue;
        }

        let dying_velocity = dying_velocity.map_or(Vec2::ZERO, |&p::LinearVelocity(v)| v);

        // Reparent children that are pickups.
        // (In the future we might want to have a different condition)
        for &child in children_of_dying.into_iter().flatten() {
            if let Ok((global_transform, mut local_transform, pickup)) =
                children_to_drop_query.get_mut(child)
            {
                // De-parent the pickup so it will survive the target being despawned,
                // preserve its global position, and give it its own physics.
                *local_transform = global_transform.compute_transform();

                let mut child_cmd = commands.entity(child);
                child_cmd.remove::<b::ChildOf>();
                child_cmd.insert(crate::pickup::after_drop_bundle(pickup));
            } else {
                b::warn!("attacked entity has child {child:?} which is not a pickup");
            }
        }

        // Spawn debris
        if let Some(particle) = dying_attackable.destruction_particle.as_ref() {
            let particle_count = rng.random_range(20u32..40);
            for _ in 0..particle_count {
                let particle_direction_1 = Vec2::from(rand_distr::UnitDisc.sample(rng));
                let particle_direction_2 = Vec2::from(rand_distr::UnitDisc.sample(rng));
                let particle_position =
                    dying_transform.translation.xy() + particle_direction_1 * 15.0;
                let particle_velocity =
                    dying_velocity + particle_direction_1 * 50.0 + particle_direction_2 * 50.0;
                commands.spawn((
                    b::Sprite::from_image(particle.clone()),
                    b::Transform::from_translation(particle_position.extend(Zees::Pickup.z()))
                        .with_rotation(b::Quat::from_rotation_z(
                            rng.random_range(0.0f32..=PI * 2.0),
                        )),
                    PLAYFIELD_LAYERS,
                    p::RigidBody::Kinematic,
                    p::Collider::circle(1.0), // TODO: use a simple movement system w/o physics so as not to exercise collision
                    p::LinearVelocity(particle_velocity),
                    Lifetime(0.5), // TODO: would be more efficient to detect when the sprite is off the screen
                ));
            }
        }

        if dying_attackable.last_hit_by == Some(Team::Player)
            && fervor_is_active(&fever_query, &coherence_query)
        {
            // Increase fervor if the player made this kill.
            // By adding some of the previous value we make it easier to get big boosts
            // with combo kills.
            let added_fervor = 0.0301 + 0.03 * fervor_query.temporary_stack().max(0.4);
            fervor_query.adjust_temporary_stacking_with_previous(added_fervor);
        }

        commands.entity(dying_entity).despawn();
    }
}

pub(crate) fn hurt_side_effects_observer(
    hurt: b::On<Hurt>,
    mut commands: b::Commands,
    assets: b::Res<crate::MyAssets>,
    mut hurt_entity_query: b::Query<(&mut Attackable, &b::Transform)>,
) -> b::Result {
    let (mut attackable, &transform) = hurt_entity_query.get_mut(hurt.0)?;
    let is_killed = attackable.health == 0;

    if attackable.hurt_animation_cooldown == 0.0 {
        attackable.hurt_animation_cooldown = 0.1;
    }

    // Play death or hurt sound
    // TODO: move death sound to death system for consistency in the presence of fever updates
    commands.spawn((
        b::AudioPlayer::new(
            // TODO: separate player kill sound
            if is_killed {
                &assets.enemy_kill_sound
            } else {
                &attackable.hurt_sound
            }
            .clone(),
        ),
        b::PlaybackSettings {
            spatial: true,
            volume: bevy::audio::Volume::Decibels(-10.),
            ..b::PlaybackSettings::DESPAWN
        },
        transform,
    ));

    Ok(())
}

pub(crate) fn hurt_animation_system(
    time: b::Res<b::Time>,
    query: b::Query<(&mut b::Sprite, &mut Attackable)>,
) {
    // arguably this should be 2 systems, one for cooldown and one for display
    for (mut sprite, mut attackable) in query {
        let luminance = if attackable.hurt_animation_cooldown > 0.0 {
            attackable.hurt_animation_cooldown =
                (attackable.hurt_animation_cooldown - time.delta_secs()).max(0.0);
            1000.0
        } else {
            1.0
        };
        sprite.color = b::Color::linear_rgb(luminance, luminance, luminance)
    }
}

pub(crate) fn player_health_is_fever_system(
    // Note that this query matches `Player` and not everything on `Team::Player`.
    // This doesn't matter now but we could imagine having drones or something.
    player_query: b::Query<&mut Attackable, b::With<Player>>,
    mut fever_query: b::Single<&mut Quantity, (b::With<Fever>, b::Without<Fervor>)>,
    mut fervor_query: b::Single<&mut Quantity, (b::With<Fervor>, b::Without<Fever>)>,
) {
    for mut attackable in player_query {
        let damage = u8::MAX - attackable.health;
        if damage > 0 {
            fever_query.adjust_permanent_including_temporary(damage as f32 * 0.1);

            if fever_query.effective_value() == 1.0 {
                // cause death
                attackable.health = 0;
            } else {
                attackable.health = u8::MAX;
            }

            // Taking any damage also resets fervor
            fervor_query.reset_to(0.0);
        }
    }
}

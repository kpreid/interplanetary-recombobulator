use avian2d::prelude::{self as p};
use bevy::math::{Vec2, Vec3Swizzles as _, vec2};
use bevy::prelude as b;
use rand::RngExt as _;
use rand::seq::IndexedRandom;

use crate::bullets_and_targets::Pattern;
use crate::pickup::PickupSpawnType;
use crate::{
    Gun, Lifetime, MyAssets, PLAYFIELD_LAYERS, PLAYFIELD_RECT, Pickup, Team, Zees,
    bullets_and_targets::Attackable,
};

// -------------------------------------------------------------------------------------------------

/// Component attached to a (currently) singleton entity that spawns enemies in a pattern.
#[derive(Debug, b::Component)]
pub(crate) struct EnemySpawner {
    pub cooldown: f32,
    pub spawn_pattern_state: usize,
}

/// Component adding enemy ship behaviors.
#[derive(Debug, b::Component)]
pub(crate) struct EnemyShipAi {
    /// What to do next.
    state: AiState,
    /// Where the ship moves to after being spawned.
    station: Vec2,
    /// Remaining time the ship stays on station before moving.
    time_on_station: f32,
}

#[derive(Clone, Copy, Debug)]
enum AiState {
    InitialWait(f32),
    GoToStation,
    WaitAtStation,
    Dive,
}

// -------------------------------------------------------------------------------------------------

/// Spawns enemies based on [`EnemySpawner`] state.
pub(crate) fn spawn_enemies_system(
    mut commands: b::Commands,
    time: b::Res<b::Time>,
    spawners: b::Query<&mut EnemySpawner>,
    assets: b::Res<crate::MyAssets>,
) {
    const SPAWN_PATTERN: [[u8; 10]; 10] = [
        *b" X X  X X ",
        *b"  X    X  ",
        *b" X X  X X ",
        *b"          ",
        *b"          ",
        *b"   XXXX   ",
        *b"   X  X   ",
        *b"  X XX X  ",
        *b"   X  X   ",
        *b"          ",
    ];

    let delta = time.delta_secs();
    for mut spawner in spawners {
        let EnemySpawner {
            cooldown,
            spawn_pattern_state,
        }: &mut EnemySpawner = &mut *spawner;
        if *cooldown > 0.0 {
            *cooldown = (*cooldown - delta).max(0.0);
        } else {
            *cooldown = 2.0;

            let row_to_spawn = &SPAWN_PATTERN[*spawn_pattern_state % SPAWN_PATTERN.len()];
            *spawn_pattern_state = (*spawn_pattern_state + 1) % SPAWN_PATTERN.len();

            // scales `i` below down to a 0-1 range, inclusive
            let index_scale_factor = ((row_to_spawn.len() - 1) as f32).recip();

            let wait_time_randomization = rand::rng().random_range(-3.0..=3.0);
            let (wait_time_range, wait_time_offset) = if wait_time_randomization > 0.0 {
                (wait_time_randomization, 0.0)
            } else {
                (-wait_time_randomization, 3.0)
            };

            for (i, &ch) in row_to_spawn.iter().enumerate() {
                let x = PLAYFIELD_RECT.min.x
                    + i as f32 * (PLAYFIELD_RECT.size().x * index_scale_factor);

                let wait_time = wait_time_offset + i as f32 * wait_time_range * index_scale_factor;

                match ch {
                    b' ' => {}
                    b'X' => {
                        commands.spawn(enemy_bundle(
                            &assets,
                            wait_time,
                            vec2(x, PLAYFIELD_RECT.max.y + 20.0),
                            vec2(x, PLAYFIELD_RECT.max.y - 20.0),
                        ));
                    }
                    _ => unreachable!(),
                }
            }
        }
    }
}

fn enemy_bundle(
    assets: &MyAssets,
    initial_wait: f32,
    spawn_position: Vec2,
    station_position: Vec2,
) -> impl b::Bundle {
    let pickup_spawn_table = const {
        &[
            (PickupSpawnType::Null, 1.0),
            (PickupSpawnType::Cool, 1.0),
            (PickupSpawnType::Cohere, 0.4),
        ]
    };

    let pickup = pickup_spawn_table
        .choose_weighted(&mut rand::rng(), |&(_, weight)| weight)
        .unwrap()
        .0
        .pickup_bundle(assets, vec2(0., 0.));

    (
        Team::Enemy,
        Attackable {
            health: 10,
            hurt_animation_cooldown: 0.0,
            destruction_particle: Some(assets.enemy_fragment_sprite.clone()),
            hurt_sound: assets.enemy_hurt_sound.clone(),
            last_hit_by: None,
        },
        Lifetime(20.0), // TODO: bad substitute for "die when offscreen"
        EnemyShipAi {
            state: AiState::InitialWait(initial_wait),
            station: station_position,
            time_on_station: 2.0,
        },
        // enemies damage if touched
        // TODO: it would probably be better to use the bullet system than the pickup system, with
        // some generalizations.
        Pickup::Damage(0.1),
        b::Transform::from_translation(spawn_position.extend(Zees::Enemy.z())),
        b::Sprite::from_image(assets.enemy_sprite.clone()),
        PLAYFIELD_LAYERS,
        p::RigidBody::Kinematic,
        p::Collider::circle(8.),
        p::LinearVelocity(vec2(0.0, 0.0)),
        Gun {
            cooldown: 3.0,
            base_cooldown: 3.0,
            trigger: false,
            pattern: Pattern::Single,
            shoot_sound: (
                assets.enemy_shoot_sound.clone(),
                bevy::audio::Volume::Decibels(-20.),
            ),
        },
        b::children![pickup],
    )
}

// -------------------------------------------------------------------------------------------------

pub(crate) fn enemy_ship_ai(
    time: b::Res<b::Time>,
    query: b::Query<(
        &mut EnemyShipAi,
        &b::Transform,
        &mut p::LinearVelocity,
        &mut Gun,
    )>,
) {
    let dt = time.delta_secs();

    for (mut ai, transform, mut velocity, mut gun) in query {
        let current_position = transform.translation.xy();

        match ai.state {
            AiState::InitialWait(wait_time) => {
                let new_wait_time = (wait_time - dt).max(0.0);
                if new_wait_time == 0.0 {
                    ai.state = AiState::GoToStation;
                } else {
                    ai.state = AiState::InitialWait(new_wait_time);
                }
            }
            AiState::GoToStation => {
                let station_relative_position = ai.station - current_position;
                let distance = station_relative_position.length();

                if distance <= 1.0 {
                    ai.state = AiState::WaitAtStation;
                } else {
                    // fly towards station
                    let acceleration = station_relative_position * 16.0 - velocity.0 * 4.0;
                    velocity.0 += acceleration * dt;
                }
            }
            AiState::WaitAtStation => {
                let new_time_on_station = (ai.time_on_station - dt).max(0.0);

                ai.time_on_station = new_time_on_station;
                if new_time_on_station == 0.0 {
                    ai.state = AiState::Dive;
                    velocity.0 = vec2(0.0, -100.0);
                } else {
                    velocity.0 = Vec2::ZERO;
                }

                gun.trigger = true;
            }
            AiState::Dive => {
                // velocity.0 += vec2(0.0, -40.0) * dt;
                gun.trigger = true;
            }
        }
    }
}

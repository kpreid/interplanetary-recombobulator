use avian2d::prelude::{self as p};
use bevy::math::{Vec2, Vec3Swizzles as _, vec2};
use bevy::prelude as b;
use rand::RngExt as _;
use rand::seq::IndexedRandom;

use crate::bullets_and_targets::Pattern;
use crate::pickup::PickupSpawnType;
use crate::quantity::{Fervor, Quantity};
use crate::{
    Gun, Lifetime, MyAssets, PLAYFIELD_LAYERS, PLAYFIELD_RECT, Pickup, Team, Zees,
    bullets_and_targets::Attackable,
};

// -------------------------------------------------------------------------------------------------

/// Component attached to a (currently) singleton entity that spawns enemies in a pattern.
#[derive(Debug, b::Component)]
pub(crate) struct EnemySpawner {
    pub cooldown: f32,
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
    fervor: b::Single<&Quantity, b::With<Fervor>>,
    assets: b::Res<crate::MyAssets>,
) {
    const SPAWN_PATTERNS: [[[u8; 10]; 4]; 7] = [
        [
            *b" XX  XX   ",
            *b"   XX  XX ",
            *b"          ",
            *b"          ",
        ],
        [
            *b"          ",
            *b" XXXXXXXX ",
            *b"          ",
            *b"          ",
        ],
        [
            *b"X        X",
            *b" X      X ",
            *b"  X    X  ",
            *b"   X  X   ",
        ],
        [
            *b" X X  X X ",
            *b"  X    X  ",
            *b" X X  X X ",
            *b"  X    X  ",
        ],
        [
            *b" XX       ",
            *b"X  X      ",
            *b"X  X      ",
            *b" XX       ",
        ],
        [
            *b"    XX    ",
            *b"   XXXX   ",
            *b"   XXXX   ",
            *b"    XX    ",
        ],
        [
            *b"       XX ",
            *b"      X  X",
            *b"      X  X",
            *b"       XX ",
        ],
    ];

    let dt = time.delta_secs();
    let rng = &mut rand::rng();
    let spawn_range_rect = PLAYFIELD_RECT.inflate(-20.0);

    for mut spawner in spawners {
        let EnemySpawner { cooldown }: &mut EnemySpawner = &mut *spawner;
        if *cooldown > 0.0 {
            // cooldown faster, i.e. spawn more often, when fervor is high
            let delta = (1.0 + fervor.effective_value()) * dt;
            *cooldown = (*cooldown - delta).max(0.0);
        } else {
            *cooldown = 5.0;

            let pattern_to_spawn: &[[u8; _]; _] = SPAWN_PATTERNS.choose(rng).unwrap();

            // scales `i` below down to a 0-1 range, inclusive
            let index_scale_factors = vec2(
                ((pattern_to_spawn[0].len() - 1) as f32).recip(),
                ((pattern_to_spawn.len() - 1) as f32).recip(),
            );

            let pattern_spacing = spawn_range_rect.size().x * index_scale_factors.x;

            // Choose how much [`AiState::InitialWait`] time is used depending on the x and y index
            let wait_time_scale = vec2(rng.random_range(-3.0..=3.0), rng.random_range(0.0..=3.0));
            let wait_time_offset = vec2(
                offset_from_signed_scale(wait_time_scale.x),
                offset_from_signed_scale(wait_time_scale.y),
            );

            for (yi, row) in pattern_to_spawn.iter().enumerate() {
                for (xi, &ch) in row.iter().enumerate() {
                    let x = spawn_range_rect.min.x + xi as f32 * pattern_spacing;
                    let y = spawn_range_rect.max.y - 40.0 - yi as f32 * pattern_spacing;

                    let wait_times = wait_time_offset
                        + vec2(xi as f32, yi as f32) * wait_time_scale * index_scale_factors;
                    let wait_time = wait_times.x + wait_times.y;

                    match ch {
                        b' ' => {}
                        b'X' => {
                            commands.spawn(enemy_bundle(
                                &assets,
                                wait_time,
                                vec2(x, PLAYFIELD_RECT.max.y + 30.0),
                                vec2(x, y),
                            ));
                        }
                        _ => unreachable!(),
                    }
                }
            }
        }
    }
}

fn offset_from_signed_scale(scale: f32) -> f32 {
    if scale >= 0.0 { 0.0 } else { scale }
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
            (PickupSpawnType::Cool, 1.5),
            (PickupSpawnType::Cohere, 0.4),
        ]
    };

    let rng = &mut rand::rng();

    let pickup = pickup_spawn_table
        .choose_weighted(rng, |&(_, weight)| weight)
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
            cooldown: rng.random_range(0.0..=3.0),
            base_cooldown: 4.0,
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
                    velocity.0 = vec2(0.0, -80.0);
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

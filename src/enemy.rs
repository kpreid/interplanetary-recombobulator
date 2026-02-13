use avian2d::prelude::{self as p};
use bevy::math::{Vec2, vec2};
use bevy::prelude as b;

use crate::bullets_and_targets::Pattern;
use crate::{
    Gun, Lifetime, PLAYFIELD_LAYERS, PLAYFIELD_RECT, Pickup, Preload, Team, Zees,
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
pub(crate) struct EnemyShipAi;

// -------------------------------------------------------------------------------------------------

/// Spawns enemies based on [`EnemySpawner`] state.
pub(crate) fn spawn_enemies_system(
    mut commands: b::Commands,
    time: b::Res<b::Time>,
    spawners: b::Query<&mut EnemySpawner>,
    assets: b::Res<crate::Preload>,
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

            for (i, &ch) in row_to_spawn.iter().enumerate() {
                let x = PLAYFIELD_RECT.min.x
                    + i as f32 * (PLAYFIELD_RECT.size().x / (row_to_spawn.len() - 1) as f32);
                match ch {
                    b' ' => {}
                    b'X' => {
                        commands.spawn(enemy_bundle(&assets, vec2(x, PLAYFIELD_RECT.max.y)));
                    }
                    _ => unreachable!(),
                }
            }
        }
    }
}

fn enemy_bundle(assets: &Preload, position: Vec2) -> impl b::Bundle {
    (
        Team::Enemy,
        Attackable {
            health: 10,
            hurt_animation_cooldown: 0.0,
            drops: true,
        },
        Lifetime(20.0), // TODO: bad substitute for "die when offscreen"
        EnemyShipAi,
        Pickup::Damage(0.1), // enemies damage if touched
        b::Transform::from_translation(position.extend(Zees::Enemy.z())),
        b::Sprite::from_image(assets.enemy_sprite.clone()),
        PLAYFIELD_LAYERS,
        p::RigidBody::Kinematic,
        p::Collider::circle(8.),
        p::LinearVelocity(vec2(0.0, -40.0)),
        Gun {
            cooldown: 0.0,
            base_cooldown: 2.0,
            trigger: false,
            pattern: Pattern::Single,
        },
    )
}

// -------------------------------------------------------------------------------------------------

pub(crate) fn enemy_ship_ai(query: b::Query<&mut Gun, b::With<EnemyShipAi>>) {
    for mut gun in query {
        gun.trigger = true;
    }
}

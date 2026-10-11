use std::path::Path;

use serde::Deserialize;

use crate::player_state::{PlayerDataKind, PlayerIconType, PlayerObjectData, PlayerState, Point};

#[derive(Deserialize)]
pub struct FakePlayerPoint {
    pub x: f32,
    pub y: f32,
    pub rot: f32,
    pub mode: u8,
    pub death: bool,
}

#[derive(Deserialize)]
pub struct FakePlayerConfiguration {
    pub account_id: i32,
    pub username: String,
    pub level_id: i32,

    pub points: Vec<FakePlayerPoint>,
}

pub struct FakePlayerState {
    config: FakePlayerConfiguration,
    state: PlayerState,
    cycle_pos: usize,
}

impl FakePlayerState {
    pub fn new(config: FakePlayerConfiguration) -> FakePlayerState {
        let state = Self::first_state_for_config(&config);

        Self { config, state, cycle_pos: 0 }
    }

    fn first_state_for_config(config: &FakePlayerConfiguration) -> PlayerState {
        PlayerState {
            account_id: config.account_id,
            ..Default::default()
        }
    }

    pub fn advance(&mut self, dt: f32) -> PlayerState {
        self.state.timestamp += dt;
        self.state.frame_number += 1;
        self.state.is_dead = false;

        if self.config.points.is_empty() {
            // rng
            let died = rand::random_ratio(1, 500);

            let prev = *self.state.player1();
            let mut pos = prev.position;
            let mut rot = prev.rotation;

            if died {
                self.state.is_dead = true;
                self.state.death_count += 1;
            } else {
                let target_height = rand::random_range(100.0..200.0);
                let x_delta = rand::random_range(4.0..6.0);

                pos.x += x_delta;

                // hedge lightly towards the target height
                let y_delta = target_height - pos.y;

                if pos.y < 100.0 || pos.y > 200.0 {
                    pos.y += y_delta.abs().sqrt() * y_delta.signum();
                } else {
                    // even more lightly
                    pos.y += y_delta.abs().cbrt() * y_delta.signum();
                }

                rot += rand::random_range(18.0..22.0);
                if rot >= 360.0 {
                    rot -= 360.0;
                }
            }

            let data = PlayerObjectData {
                position: pos,
                rotation: rot,
                ..prev
            };

            self.state.data_kind = PlayerDataKind::Single { player: data };
        } else {
            // loop
            let point = &self.config.points[self.cycle_pos];
            let data = Self::data_from_point(point);
            self.cycle_pos += 1;
            if self.cycle_pos >= self.config.points.len() {
                self.cycle_pos = 0;
            }

            self.state.data_kind = PlayerDataKind::Single { player: data };

            if point.death {
                self.state.death_count += 1;
                self.state.is_dead = true;
            }
        }

        self.state
    }

    fn data_from_point(point: &FakePlayerPoint) -> PlayerObjectData {
        PlayerObjectData {
            position: Point::new(point.x, point.y),
            rotation: point.rot,
            icon_type: PlayerIconType::try_from(point.mode).expect("invalid mode passed"),
            ..Default::default()
        }
    }
}

pub fn parse_players(dir: &Path) -> anyhow::Result<Vec<FakePlayerConfiguration>> {
    let mut out = Vec::new();

    for entry in dir.read_dir()? {
        let e = entry?;
        if !e.file_type()?.is_file() {
            continue;
        }

        let data = std::fs::read_to_string(e.path())?;
        let config = toml::from_str(&data)?;

        out.push(config);
    }

    Ok(out)
}

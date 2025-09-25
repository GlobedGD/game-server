use crate::data::{player_data, player_object_data};
use const_default::ConstDefault;
use server_shared::{
    encoding::DataDecodeError,
    schema::{game::extended_player_data, shared::IconType},
};

#[derive(Debug, Clone, Copy, Default, ConstDefault, PartialEq)]
pub struct Point {
    pub x: f32,
    pub y: f32,
}

impl Point {
    pub fn new(x: f32, y: f32) -> Self {
        Self { x, y }
    }

    pub fn distance(&self, other: &Point) -> f32 {
        let dx = self.x - other.x;
        let dy = self.y - other.y;
        (dx * dx + dy * dy).sqrt()
    }

    /// Calculates angle to another point in radians, with 0 meaning right and positive means CCW rotation
    /// Range is [0, 2pi)
    pub fn angle_to(&self, other: &Point) -> f32 {
        let dy = other.y - self.y;
        let dx = other.x - self.x;
        let mut angle = dy.atan2(dx);

        if angle < 0.0 {
            angle += std::f32::consts::TAU;
        }

        debug_assert!((0.0..std::f32::consts::TAU).contains(&angle));

        angle
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
#[repr(u16)]
pub enum PlayerIconType {
    #[default]
    Unknown = 0,
    Cube = 1,
    Ship = 2,
    Ball = 3,
    Ufo = 4,
    Wave = 5,
    Robot = 6,
    Spider = 7,
    Swing = 8,
    Jetpack = 9,
}

impl ConstDefault for PlayerIconType {
    const DEFAULT: Self = PlayerIconType::Unknown;
}

impl From<IconType> for PlayerIconType {
    fn from(value: IconType) -> Self {
        let raw = value as u16;

        match raw {
            0 => PlayerIconType::Unknown,
            1 => PlayerIconType::Cube,
            2 => PlayerIconType::Ship,
            3 => PlayerIconType::Ball,
            4 => PlayerIconType::Ufo,
            5 => PlayerIconType::Wave,
            6 => PlayerIconType::Robot,
            7 => PlayerIconType::Spider,
            8 => PlayerIconType::Swing,
            9 => PlayerIconType::Jetpack,
            _ => unreachable!(),
        }
    }
}

#[derive(Debug, Clone, Copy, Default, ConstDefault)]
pub struct ExtendedPlayerData {
    pub velocity: Point,
    pub accelerating: bool,
    pub acceleration: f32,
    pub fall_start_y: f32,
    pub is_on_ground2: bool,
    pub gravity_mod: f32,
    pub gravity: f32,
    pub touched_pad: bool,
}

#[derive(Debug, Clone, Copy, Default, ConstDefault)]
pub struct PlayerObjectData {
    pub position: Point,
    pub rotation: f32,
    pub icon_type: PlayerIconType,

    pub is_visible: bool,
    pub is_looking_left: bool,
    pub is_upside_down: bool,
    pub is_dashing: bool,
    pub is_mini: bool,
    pub is_grounded: bool,
    pub is_stationary: bool,
    pub is_falling: bool,
    pub is_rotating: bool,
    pub is_sideways: bool,

    pub ext_data: Option<ExtendedPlayerData>,
}

impl ExtendedPlayerData {
    pub fn from_reader(reader: extended_player_data::Reader<'_>) -> Result<Self, DataDecodeError> {
        let velocity_x = reader.get_velocity_x();
        let velocity_y = reader.get_velocity_y();

        if !velocity_x.is_finite() || !velocity_y.is_finite() {
            return Err(DataDecodeError::InvalidFloat);
        }

        let velocity = Point::new(velocity_x, velocity_y);

        let accelerating = reader.get_accelerating();
        let acceleration = reader.get_acceleration();
        let fall_start_y = reader.get_fall_start_y();
        let is_on_ground2 = reader.get_is_on_ground2();
        let gravity_mod = reader.get_gravity_mod();
        let gravity = reader.get_gravity();

        if !acceleration.is_finite()
            || !fall_start_y.is_finite()
            || !gravity_mod.is_finite()
            || !gravity.is_finite()
        {
            return Err(DataDecodeError::InvalidFloat);
        }

        let touched_pad = reader.get_touched_pad();

        Ok(Self {
            velocity,
            accelerating,
            acceleration,
            fall_start_y,
            is_on_ground2,
            gravity_mod,
            gravity,
            touched_pad,
        })
    }

    pub fn encode(&self, mut builder: extended_player_data::Builder<'_>) {
        builder.set_velocity_x(self.velocity.x);
        builder.set_velocity_y(self.velocity.y);
        builder.set_accelerating(self.accelerating);
        builder.set_acceleration(self.acceleration);
        builder.set_fall_start_y(self.fall_start_y);
        builder.set_is_on_ground2(self.is_on_ground2);
        builder.set_gravity_mod(self.gravity_mod);
        builder.set_gravity(self.gravity);
        builder.set_touched_pad(self.touched_pad);
    }
}

impl PlayerObjectData {
    pub fn from_reader(reader: player_object_data::Reader<'_>) -> Result<Self, DataDecodeError> {
        let position_x = reader.get_position_x();
        let position_y = reader.get_position_y();

        if !position_x.is_finite() || !position_y.is_finite() {
            return Err(DataDecodeError::InvalidFloat);
        }

        let position = Point::new(position_x, position_y);

        Ok(Self {
            position,
            rotation: reader.get_rotation(),
            icon_type: reader
                .get_icon_type()
                .map_err(|_| DataDecodeError::InvalidDiscriminant)?
                .into(),
            is_visible: reader.get_is_visible(),
            is_looking_left: reader.get_is_looking_left(),
            is_upside_down: reader.get_is_upside_down(),
            is_dashing: reader.get_is_dashing(),
            is_mini: reader.get_is_mini(),
            is_grounded: reader.get_is_grounded(),
            is_stationary: reader.get_is_stationary(),
            is_falling: reader.get_is_falling(),
            is_rotating: reader.get_is_rotating(),
            is_sideways: reader.get_is_sideways(),
            ext_data: if reader.has_ext_data() {
                Some(ExtendedPlayerData::from_reader(reader.get_ext_data()?)?)
            } else {
                None
            },
        })
    }

    pub fn encode(&self, mut builder: player_object_data::Builder<'_>) {
        builder.set_position_x(self.position.x);
        builder.set_position_y(self.position.y);
        builder.set_rotation(self.rotation);
        builder.set_icon_type(IconType::try_from(self.icon_type as u16).unwrap());

        builder.set_is_visible(self.is_visible);
        builder.set_is_looking_left(self.is_looking_left);
        builder.set_is_upside_down(self.is_upside_down);
        builder.set_is_dashing(self.is_dashing);
        builder.set_is_mini(self.is_mini);
        builder.set_is_grounded(self.is_grounded);
        builder.set_is_stationary(self.is_stationary);
        builder.set_is_falling(self.is_falling);
        builder.set_is_rotating(self.is_rotating);
        builder.set_is_sideways(self.is_sideways);

        if let Some(ext_data) = &self.ext_data {
            let mut ext_builder = builder.init_ext_data();
            ext_data.encode(ext_builder.reborrow());
        }
    }

    pub fn in_range(&self, camera_range: &CameraRange) -> bool {
        self.position.distance(&camera_range.center) < camera_range.radius
    }
}

#[derive(Debug, Clone, Copy)]
pub enum PlayerDataKind {
    Dual {
        player1: PlayerObjectData,
        player2: PlayerObjectData,
    },

    Single {
        player: PlayerObjectData,
    },
}

impl Default for PlayerDataKind {
    fn default() -> Self {
        Self::Single {
            player: PlayerObjectData::default(),
        }
    }
}

impl ConstDefault for PlayerDataKind {
    const DEFAULT: Self = PlayerDataKind::Single {
        player: PlayerObjectData::DEFAULT,
    };
}

/// In-level player state
#[derive(Debug, Clone, Copy, Default, ConstDefault)]
pub struct PlayerState {
    pub account_id: i32,
    pub timestamp: f32,
    pub frame_number: u8,
    pub death_count: u8,
    pub percentage: u16,
    pub is_dead: bool,
    pub is_paused: bool,
    pub is_practicing: bool,
    pub is_in_editor: bool,
    pub is_editor_building: bool,
    pub is_last_death_real: bool,

    pub data_kind: PlayerDataKind,
}

impl PlayerState {
    pub fn from_reader(reader: player_data::Reader<'_>) -> Result<Self, DataDecodeError> {
        let data_kind = match reader.which().map_err(|_| DataDecodeError::InvalidDiscriminant)? {
            player_data::Which::Dual(k) => {
                let player1 = k.get_player1()?;
                let player2 = k.get_player2()?;

                let player1 = PlayerObjectData::from_reader(player1)?;
                let player2 = PlayerObjectData::from_reader(player2)?;

                PlayerDataKind::Dual { player1, player2 }
            }

            player_data::Which::Single(k) => {
                let player1 = k.get_player1()?;
                let player1 = PlayerObjectData::from_reader(player1)?;

                PlayerDataKind::Single { player: player1 }
            }

            player_data::Which::Culled(_) => Err(DataDecodeError::ValidationFailed)?,
        };

        Ok(Self {
            account_id: reader.get_account_id(),
            timestamp: reader.get_timestamp(),
            frame_number: reader.get_frame_number(),
            death_count: reader.get_death_count(),
            percentage: reader.get_percentage(),
            is_dead: reader.get_is_dead(),
            is_paused: reader.get_is_paused(),
            is_practicing: reader.get_is_practicing(),
            is_in_editor: reader.get_is_in_editor(),
            is_editor_building: reader.get_is_editor_building(),
            is_last_death_real: reader.get_is_last_death_real(),
            data_kind,
        })
    }

    pub fn encode(
        &self,
        mut builder: player_data::Builder<'_>,
        platformer: bool,
        camera_range: &CameraRange,
    ) {
        builder.set_account_id(self.account_id);
        builder.set_timestamp(self.timestamp);
        builder.set_frame_number(self.frame_number);
        builder.set_death_count(self.death_count);
        builder.set_is_dead(self.is_dead);
        builder.set_is_paused(self.is_paused);
        builder.set_is_practicing(self.is_practicing);
        builder.set_is_in_editor(self.is_in_editor);
        builder.set_is_editor_building(self.is_editor_building);
        builder.set_is_last_death_real(self.is_last_death_real);

        if platformer {
            // in platformers, the percentage field will be the angle between the center of the player's screen and that player's position
            let angle = self.angle_to(camera_range);

            // map it to a value between 0 and 65535
            let perc = (angle / std::f32::consts::TAU * 65535.0) as u16;
            builder.set_percentage(perc);
        } else {
            // in classic levels, just send over the percentage as calculated by that client
            builder.set_percentage(self.percentage);
        }

        if self.in_range(camera_range) {
            match &self.data_kind {
                PlayerDataKind::Single { player } => {
                    player.encode(builder.init_single().init_player1());
                }

                PlayerDataKind::Dual { player1, player2 } => {
                    let mut dual = builder.init_dual();
                    player1.encode(dual.reborrow().init_player1());
                    player2.encode(dual.reborrow().init_player2());
                }
            }
        } else {
            builder.init_culled();
        }
    }

    pub fn in_range(&self, camera_range: &CameraRange) -> bool {
        match &self.data_kind {
            PlayerDataKind::Single { player } => player.in_range(camera_range),
            PlayerDataKind::Dual { player1, player2 } => {
                player1.in_range(camera_range) || player2.in_range(camera_range)
            }
        }
    }

    pub fn angle_to(&self, camera_range: &CameraRange) -> f32 {
        match &self.data_kind {
            PlayerDataKind::Single { player } | PlayerDataKind::Dual { player1: player, .. } => {
                camera_range.center.angle_to(&player.position)
            }
        }
    }

    pub fn player1(&self) -> &PlayerObjectData {
        match &self.data_kind {
            PlayerDataKind::Single { player } => player,
            PlayerDataKind::Dual { player1, player2: _ } => player1,
        }
    }
}

pub struct CameraRange {
    center: Point,
    radius: f32,
}

impl CameraRange {
    pub fn new(x: f32, y: f32, radius: f32) -> Self {
        Self {
            center: Point::new(x, y),
            radius,
        }
    }
}

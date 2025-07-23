use crate::data::{player_data, player_object_data};
use const_default::ConstDefault;
use server_shared::{encoding::DataDecodeError, schema::shared::IconType};

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

#[derive(Debug, Clone, Default, ConstDefault)]
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
    }
}

#[derive(Debug, Clone)]
pub enum PlayerDataKind {
    Dual {
        player1: PlayerObjectData,
        player2: PlayerObjectData,
    },

    Single {
        player: PlayerObjectData,
    },
    // TODO (very low): more complete data for spectating
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
#[derive(Debug, Clone, Default, ConstDefault)]
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

    pub fn encode(&self, mut builder: player_data::Builder<'_>) {
        builder.set_account_id(self.account_id);
        builder.set_timestamp(self.timestamp);
        builder.set_frame_number(self.frame_number);
        builder.set_death_count(self.death_count);
        builder.set_percentage(self.percentage);
        builder.set_is_dead(self.is_dead);
        builder.set_is_paused(self.is_paused);
        builder.set_is_practicing(self.is_practicing);
        builder.set_is_in_editor(self.is_in_editor);
        builder.set_is_editor_building(self.is_editor_building);
        builder.set_is_last_death_real(self.is_last_death_real);

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
    }

    /// Determines if another player is "near" this one, aka this player can see the other player.
    pub fn is_near(&self, _other: &Self) -> bool {
        // TODO (low)
        true
    }
}

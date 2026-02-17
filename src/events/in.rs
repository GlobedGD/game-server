use server_shared::encoding::DataDecodeError;

use super::*;

#[non_exhaustive]
#[derive(Clone)]
pub enum InEvent {
    CounterChange(CounterChangeEvent),

    // These 2 are emitted by the mod itself, can't be sent by the client
    PlayerJoin(i32),
    PlayerLeave(i32),

    /// Represents an event for the script system
    Scripted {
        r#type: u16,
        args: heapless::Vec<IntOrFloat, 5>,
    },

    RequestScriptLogs,

    TwoPlayerLinkRequest {
        player_id: i32,
        player1: bool,
    },

    TwoPlayerUnlink {
        player_id: i32,
    },

    // switcheroo
    SwitcherooFullState {
        active_player: i32,
        flags: u8,
    },

    SwitcherooSwitch {
        player: i32,
        r#type: u8,
    },
}

impl InEvent {
    #[allow(non_contiguous_range_endpoints)]
    pub fn decode(ty: u16, reader: &mut ByteReader) -> Result<Self, DataDecodeError> {
        match ty {
            EVENT_COUNTER_CHANGE => {
                let data = reader.read_u64()?;

                let raw_type = (data >> 56) as u8;
                let item_id = ((data >> 32) as u32) & 0x00ffffff;
                let raw_value = data as u32;

                let r#type = match raw_type {
                    0 => CounterChangeType::Set(raw_value as i32),
                    1 => CounterChangeType::Add(raw_value as i32),
                    2 => CounterChangeType::Multiply(f32::from_bits(raw_value)),
                    3 => CounterChangeType::Divide(f32::from_bits(raw_value)),
                    _ => return Err(DataDecodeError::ValidationFailed),
                };

                Ok(Self::CounterChange(CounterChangeEvent { item_id, r#type }))
            }

            EVENT_SCR_REQUEST_SCRIPT_LOGS => Ok(Self::RequestScriptLogs),

            EVENT_2P_LINK_REQUEST => {
                let player_id = reader.read_i32()?;
                let player1 = reader.read_bool()?;

                Ok(Self::TwoPlayerLinkRequest { player_id, player1 })
            }

            EVENT_2P_UNLINK => {
                let player_id = reader.read_i32()?;

                Ok(InEvent::TwoPlayerUnlink { player_id })
            }

            EVENT_SWITCHEROO_FULL_STATE => {
                let player_id = reader.read_i32()?;
                let flags = reader.read_u8()?;

                Ok(InEvent::SwitcherooFullState {
                    active_player: player_id,
                    flags,
                })
            }

            EVENT_SWITCHEROO_SWITCH => {
                let player = reader.read_i32()?;
                let r#type = reader.read_u8()?;

                Ok(InEvent::SwitcherooSwitch { player, r#type })
            }

            r#type @ 0..EVENT_GLOBED_BASE => {
                let mut args = heapless::Vec::new();

                let count = reader.read_u8()?;

                if count > args.capacity() as u8 {
                    return Err(DataDecodeError::ValidationFailed);
                }

                // decode argument types, 1 bit per argument, high bit means float, low bit means int
                let type_byte = reader.read_u8()?;

                for i in 0..count {
                    let shift = 7 - i;
                    let bit = (type_byte >> shift) & 1;

                    let arg = if bit == 1 {
                        IntOrFloat::Float(reader.read_f32()?)
                    } else {
                        IntOrFloat::Int(reader.read_i32()?)
                    };

                    let _ = args.push(arg);
                }

                Ok(InEvent::Scripted { r#type, args })
            }

            _ => Err(DataDecodeError::ValidationFailed),
        }
    }

    pub fn type_int(&self) -> u16 {
        match self {
            Self::Scripted { r#type, .. } => *r#type,
            Self::CounterChange(_) => EVENT_COUNTER_CHANGE,
            Self::PlayerJoin(_) => EVENT_PLAYER_JOIN,
            Self::PlayerLeave(_) => EVENT_PLAYER_LEAVE,

            Self::RequestScriptLogs => EVENT_SCR_REQUEST_SCRIPT_LOGS,

            Self::TwoPlayerLinkRequest { .. } => EVENT_2P_LINK_REQUEST,
            Self::TwoPlayerUnlink { .. } => EVENT_2P_UNLINK,
            Self::SwitcherooFullState { .. } => EVENT_SWITCHEROO_FULL_STATE,
            Self::SwitcherooSwitch { .. } => EVENT_SWITCHEROO_SWITCH,
        }
    }
}

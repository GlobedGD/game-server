use qunet::buffers::{Bits, ByteReader, ByteWriter, ByteWriterError};
use server_shared::encoding::DataDecodeError;
use smallvec::SmallVec;
use thiserror::Error;

use crate::data::event;

const EVENT_GLOBED_BASE: u16 = 0xf000;
pub const EVENT_COUNTER_CHANGE: u16 = 0xf001;
pub const EVENT_PLAYER_JOIN: u16 = 0xf002;
pub const EVENT_PLAYER_LEAVE: u16 = 0xf003;

pub const EVENT_SCR_SPAWN_GROUP: u16 = 0xf010;
pub const EVENT_SCR_SET_ITEM: u16 = 0xf011;
pub const EVENT_SCR_REQUEST_SCRIPT_LOGS: u16 = 0xf012;
pub const EVENT_SCR_MOVE_GROUP: u16 = 0xf013;
pub const EVENT_SCR_MOVE_GROUP_ABSOLUTE: u16 = 0xf014;
pub const EVENT_SCR_FOLLOW_PLAYER: u16 = 0xf015;

pub const EVENT_2P_LINK_REQUEST: u16 = 0xf100;
pub const EVENT_2P_UNLINK: u16 = 0xf101;

#[derive(Clone)]
pub enum CounterChangeType {
    Set(i32),
    Add(i32),
    Multiply(f32),
    Divide(f32),
}

#[derive(Clone)]
pub struct CounterChangeEvent {
    pub item_id: u32,
    pub r#type: CounterChangeType,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum IntOrFloat {
    Int(i32),
    Float(f32),
}

#[derive(Default, Clone)]
pub struct SpawnInfo {
    pub group_id: i32,
    pub delay: f32,
    pub delay_variance: f32,
    pub ordered: bool,
    pub remaps: SmallVec<[u32; 6]>,
}

#[derive(Debug, Error)]
pub enum EventEncodeError {
    #[error("{0}")]
    Encode(#[from] ByteWriterError),
    #[error("Invalid event data")]
    InvalidData,
}

#[non_exhaustive]
#[derive(Clone)]
pub enum Event {
    CounterChange(CounterChangeEvent),

    PlayerJoin(i32),
    PlayerLeave(i32),

    /// Represents an event for the script system
    Scripted {
        r#type: u16,
        args: heapless::Vec<IntOrFloat, 5>,
    },

    SpawnGroup(SpawnInfo),

    SetItem {
        item_id: u32,
        value: i32,
    },

    RequestScriptLogs,

    MoveGroup {
        group: u16,
        dx: f32,
        dy: f32,
    },

    MoveGroupAbsolute {
        group: u16,
        center: u16,
        x: f32,
        y: f32,
    },

    FollowPlayer {
        player_id: i32,
        group: u16,
        enable: bool,
    },

    TwoPlayerLinkRequest {
        player_id: i32,
        player1: bool,
    },

    TwoPlayerUnlink {
        player_id: i32,
    },
}

impl Event {
    #[allow(non_contiguous_range_endpoints)]
    pub fn from_reader(reader: event::Reader<'_>) -> Result<Self, DataDecodeError> {
        match reader.get_type() {
            EVENT_COUNTER_CHANGE => {
                let data = reader.get_data()?;

                if data.len() < 8 {
                    return Err(DataDecodeError::ValidationFailed);
                }

                let data = u64::from_le_bytes(data[0..8].try_into().unwrap());

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

                Ok(Event::CounterChange(CounterChangeEvent { item_id, r#type }))
            }

            r#type @ 0..EVENT_GLOBED_BASE => {
                let mut args = heapless::Vec::new();

                let mut reader = ByteReader::new(reader.get_data()?);
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

                Ok(Event::Scripted { r#type, args })
            }

            EVENT_SCR_REQUEST_SCRIPT_LOGS => Ok(Event::RequestScriptLogs),

            EVENT_2P_LINK_REQUEST => {
                let data = reader.get_data()?;
                let mut reader = ByteReader::new(data);

                let player_id = reader.read_i32()?;
                let player1 = reader.read_bool()?;

                Ok(Event::TwoPlayerLinkRequest { player_id, player1 })
            }

            EVENT_2P_UNLINK => {
                let data = reader.get_data()?;

                let player_id = ByteReader::new(data).read_i32()?;

                Ok(Event::TwoPlayerUnlink { player_id })
            }

            _ => Err(DataDecodeError::ValidationFailed),
        }
    }

    pub fn type_int(&self) -> u16 {
        match self {
            Event::Scripted { r#type, .. } => *r#type,
            Event::CounterChange(_) => EVENT_COUNTER_CHANGE,
            Event::PlayerJoin(_) => EVENT_PLAYER_JOIN,
            Event::PlayerLeave(_) => EVENT_PLAYER_LEAVE,

            Event::RequestScriptLogs => EVENT_SCR_REQUEST_SCRIPT_LOGS,
            Event::SpawnGroup { .. } => EVENT_SCR_SPAWN_GROUP,
            Event::SetItem { .. } => EVENT_SCR_SET_ITEM,
            Event::MoveGroup { .. } => EVENT_SCR_MOVE_GROUP,
            Event::MoveGroupAbsolute { .. } => EVENT_SCR_MOVE_GROUP_ABSOLUTE,
            Event::FollowPlayer { .. } => EVENT_SCR_FOLLOW_PLAYER,

            Event::TwoPlayerLinkRequest { .. } => EVENT_2P_LINK_REQUEST,
            Event::TwoPlayerUnlink { .. } => EVENT_2P_UNLINK,
        }
    }

    pub fn encode(&self, mut writer: event::Builder<'_>) -> Result<(), EventEncodeError> {
        writer.set_type(self.type_int());

        match self {
            Event::CounterChange(ev) => {
                let mut data = [0u8; 8];
                let raw_type = match ev.r#type {
                    CounterChangeType::Set(_) => 0,
                    CounterChangeType::Add(_) => 1,
                    CounterChangeType::Multiply(_) => 2,
                    CounterChangeType::Divide(_) => 3,
                };

                let item_id = (ev.item_id as u64) & 0x00ffffff;
                let value = match ev.r#type {
                    CounterChangeType::Set(val) => val as u64,
                    CounterChangeType::Add(val) => val as u64,
                    CounterChangeType::Multiply(val) => val.to_bits() as u64,
                    CounterChangeType::Divide(val) => val.to_bits() as u64,
                };

                let packed_data = ((raw_type as u64) << 56) | (item_id << 32) | value;

                data.copy_from_slice(&packed_data.to_le_bytes());

                writer.set_data(&data);
            }

            Event::PlayerJoin(player_id) => {
                let mut data = [0u8; 4];
                data.copy_from_slice(&player_id.to_le_bytes());
                writer.set_data(&data);
            }

            Event::PlayerLeave(player_id) => {
                let mut data = [0u8; 4];
                data.copy_from_slice(&player_id.to_le_bytes());
                writer.set_data(&data);
            }

            Event::SpawnGroup(info) => {
                let mut data = [0u8; 40];
                let mut buffer = ByteWriter::new(&mut data);

                buffer.write_u8(0); // flags, set later
                buffer.write_varuint(info.group_id as u64)?;

                let mut bits = Bits::new(0u8);

                if info.delay != 0.0 {
                    bits.set_bit(0);
                    buffer.write_f32(info.delay);

                    if info.delay_variance != 0.0 {
                        bits.set_bit(1);
                        buffer.write_f32(info.delay_variance);
                    }
                }

                if info.ordered {
                    bits.set_bit(2);
                }

                if !info.remaps.is_empty() {
                    if info.remaps.len() > 255 {
                        return Err(EventEncodeError::InvalidData);
                    }

                    bits.set_bit(3);
                    buffer.write_u8(info.remaps.len() as u8);

                    for key in info.remaps.iter() {
                        buffer.write_varuint(*key as u64)?;
                    }
                }

                writer.set_data(buffer.written());
            }

            Event::SetItem { item_id, value } => {
                let mut data = [0u8; 16];
                let mut buffer = ByteWriter::new(&mut data);

                buffer.write_varuint(*item_id as u64)?;
                buffer.write_varint(*value as i64)?;

                writer.set_data(buffer.written());
            }

            Event::RequestScriptLogs => {
                unimplemented!()
            }

            &Event::MoveGroup { group, dx, dy } => {
                let mut data = [0u8; 16];
                let mut buffer = ByteWriter::new(&mut data);

                buffer.write_varuint(group as u64)?;
                buffer.write_f32(dx);
                buffer.write_f32(dy);

                writer.set_data(buffer.written());
            }
            &Event::MoveGroupAbsolute { group, center, x, y } => {
                let mut data = [0u8; 16];
                let mut buffer = ByteWriter::new(&mut data);

                buffer.write_varuint(group as u64)?;
                buffer.write_varuint(center as u64)?;
                buffer.write_f32(x);
                buffer.write_f32(y);

                writer.set_data(buffer.written());
            }

            &Event::FollowPlayer { mut group, player_id, enable } => {
                let mut data = [0u8; 16];
                let mut buffer = ByteWriter::new(&mut data);

                if enable {
                    // set top bit
                    group |= 1 << 15;
                } else {
                    // clear top bit
                    group &= !(1 << 15);
                }

                buffer.write_u16(group);
                buffer.write_i32(player_id);

                writer.set_data(buffer.written());
            }

            Event::TwoPlayerLinkRequest { player_id, player1 } => {
                let mut data = [0u8; 5];
                let mut buffer = ByteWriter::new(&mut data);

                buffer.write_i32(*player_id);
                buffer.write_bool(*player1);

                writer.set_data(buffer.written());
            }

            Event::TwoPlayerUnlink { player_id } => {
                let mut data = [0u8; 4];
                data.copy_from_slice(&player_id.to_le_bytes());
                writer.set_data(&data);
            }

            Event::Scripted { r#type: _, args: _ } => {
                // let mut data = [0u8; 128];

                // // encode argument types
                // let mut type_byte = 0u8;
                unimplemented!()
            }
        }

        Ok(())
    }
}

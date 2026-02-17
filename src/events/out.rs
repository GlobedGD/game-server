use server_shared::qunet::buffers::Bits;

use super::*;

#[derive(Default, Clone)]
pub struct SpawnInfo {
    pub group_id: u16,
    pub delay: f32,
    pub delay_variance: f32,
    pub ordered: bool,
    pub remaps: SmallVec<[u16; 6]>,
}

#[non_exhaustive]
#[derive(Clone)]
pub enum OutEvent {
    CounterChange(CounterChangeEvent),

    SpawnGroup(SpawnInfo),

    SetItem {
        item_id: u32,
        value: i32,
    },

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

    FollowRotation {
        player_id: i32,
        group: u16,
        center: u16,
        enable: bool,
    },

    // 2 player mode
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

impl OutEvent {
    pub fn type_int(&self) -> u16 {
        match self {
            Self::CounterChange(_) => EVENT_COUNTER_CHANGE,
            Self::SpawnGroup(_) => EVENT_SCR_SPAWN_GROUP,
            Self::SetItem { .. } => EVENT_SCR_SET_ITEM,
            Self::MoveGroup { .. } => EVENT_SCR_MOVE_GROUP,
            Self::MoveGroupAbsolute { .. } => EVENT_SCR_MOVE_GROUP_ABSOLUTE,
            Self::FollowPlayer { .. } => EVENT_SCR_FOLLOW_PLAYER,
            Self::FollowRotation { .. } => EVENT_SCR_FOLLOW_ROTATION,

            Self::TwoPlayerLinkRequest { .. } => EVENT_2P_LINK_REQUEST,
            Self::TwoPlayerUnlink { .. } => EVENT_2P_UNLINK,
            Self::SwitcherooFullState { .. } => EVENT_SWITCHEROO_FULL_STATE,
            Self::SwitcherooSwitch { .. } => EVENT_SWITCHEROO_SWITCH,
        }
    }

    pub fn estimate_bytes(&self) -> usize {
        match self {
            Self::CounterChange(_) => 8,
            Self::SpawnGroup(s) => 16 + s.remaps.len() * 8,
            Self::SetItem { .. } => 10,
            Self::MoveGroup { .. } => 10,
            Self::MoveGroupAbsolute { .. } => 12,
            Self::FollowPlayer { .. } => 6,
            Self::FollowRotation { .. } => 8,

            Self::TwoPlayerLinkRequest { .. } => 5,
            Self::TwoPlayerUnlink { .. } => 4,
            Self::SwitcherooFullState { .. } => 5,
            Self::SwitcherooSwitch { .. } => 5,
        }
    }

    pub fn encode(&self, writer: &mut ByteWriter) -> Result<(), EventEncodeError> {
        match self {
            Self::CounterChange(ev) => {
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

                writer.write_u64(packed_data);
            }

            Self::SpawnGroup(info) => {
                let flags_pos = writer.pos();
                writer.write_u8(0); // flags, set later
                writer.write_varuint(info.group_id as u64)?;

                let mut bits = Bits::new(0u8);

                if info.delay != 0.0 {
                    bits.set_bit(0);
                    writer.write_f32(info.delay);

                    if info.delay_variance != 0.0 {
                        bits.set_bit(1);
                        writer.write_f32(info.delay_variance);
                    }
                }

                if info.ordered {
                    bits.set_bit(2);
                }

                if !info.remaps.is_empty() {
                    if info.remaps.len() > 510 || !info.remaps.len().is_multiple_of(2) {
                        return Err(EventEncodeError::InvalidData);
                    }

                    bits.set_bit(3);
                    writer.write_u8((info.remaps.len() / 2) as u8);

                    for key in info.remaps.iter() {
                        writer.write_varuint(*key as u64)?;
                    }
                }

                writer.perform_at(flags_pos, |w| {
                    w.write_u8(bits.to_bits());
                });
            }

            &Self::SetItem { item_id, value } => {
                writer.write_varuint(item_id as u64)?;
                writer.write_varint(value as i64)?;
            }

            &Self::MoveGroup { group, dx, dy } => {
                writer.write_varuint(group as u64)?;
                writer.write_f32(dx);
                writer.write_f32(dy);
            }

            &Self::MoveGroupAbsolute { group, center, x, y } => {
                writer.write_varuint(group as u64)?;
                writer.write_varuint(center as u64)?;
                writer.write_f32(x);
                writer.write_f32(y);
            }

            &Self::FollowPlayer { player_id, mut group, enable } => {
                if enable {
                    // set top bit
                    group |= 1 << 15;
                } else {
                    // clear top bit
                    group &= !(1 << 15);
                }

                writer.write_u16(group);
                writer.write_i32(player_id);
            }

            &Self::FollowRotation {
                player_id,
                mut group,
                center,
                enable,
            } => {
                if enable {
                    // set top bit
                    group |= 1 << 15;
                } else {
                    // clear top bit
                    group &= !(1 << 15);
                }

                writer.write_u16(group);
                writer.write_u16(center);
                writer.write_i32(player_id);
            }

            &Self::TwoPlayerLinkRequest { player_id, player1 } => {
                writer.write_i32(player_id);
                writer.write_bool(player1);
            }

            &Self::TwoPlayerUnlink { player_id } => {
                writer.write_i32(player_id);
            }

            &Self::SwitcherooFullState { active_player, flags } => {
                writer.write_i32(active_player);
                writer.write_u8(flags);
            }

            &Self::SwitcherooSwitch { player, r#type } => {
                writer.write_i32(player);
                writer.write_u8(r#type);
            }
        }

        Ok(())
    }
}

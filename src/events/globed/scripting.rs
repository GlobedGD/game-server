use bitpiece::bitpiece;
use server_shared::{
    encoding::DataDecodeError,
    events::EventEncode,
    qunet::buffers::{ByteReader, HeapByteWriter},
};
use smallvec::SmallVec;

// Counter change

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

impl CounterChangeEvent {
    pub fn decode(data: &[u8]) -> Result<Self, DataDecodeError> {
        let mut reader = ByteReader::new(data);

        let raw_data = reader.read_u64()?;

        let raw_type = (raw_data >> 56) as u8;
        let item_id = ((raw_data >> 32) as u32) & 0x00ffffff;
        let raw_value = raw_data as u32;

        let r#type = match raw_type {
            0 => CounterChangeType::Set(raw_value as i32),
            1 => CounterChangeType::Add(raw_value as i32),
            2 => CounterChangeType::Multiply(f32::from_bits(raw_value)),
            3 => CounterChangeType::Divide(f32::from_bits(raw_value)),
            _ => return Err(DataDecodeError::ValidationFailed),
        };

        Ok(CounterChangeEvent { item_id, r#type })
    }
}

impl EventEncode for CounterChangeEvent {
    fn size_bound(&self) -> Option<usize> {
        Some(8)
    }

    fn id() -> &'static str {
        "globed/counter-change"
    }

    fn encode(&self, writer: &mut HeapByteWriter) {
        let raw_type = match self.r#type {
            CounterChangeType::Set(_) => 0,
            CounterChangeType::Add(_) => 1,
            CounterChangeType::Multiply(_) => 2,
            CounterChangeType::Divide(_) => 3,
        };

        let item_id = (self.item_id as u64) & 0x00ffffff;
        let value = match self.r#type {
            CounterChangeType::Set(val) => val as u64,
            CounterChangeType::Add(val) => val as u64,
            CounterChangeType::Multiply(val) => val.to_bits() as u64,
            CounterChangeType::Divide(val) => val.to_bits() as u64,
        };

        let packed_data = ((raw_type as u64) << 56) | (item_id << 32) | value;

        writer.write_u64(packed_data);
    }
}

// Spawn

#[derive(Default, Clone)]
pub struct SpawnInfo {
    pub group_id: u16,
    pub delay: f32,
    pub delay_variance: f32,
    pub ordered: bool,
    pub remaps: SmallVec<[u16; 6]>,
}

#[bitpiece]
#[derive(Default)]
struct SpawnInfoFlags {
    has_delay: bool,
    has_delay_variance: bool,
    ordered: bool,
    has_remaps: bool,
}

pub struct SpawnGroupEvent(pub SpawnInfo);

impl SpawnGroupEvent {
    pub fn new(info: SpawnInfo) -> Option<Self> {
        if info.remaps.len() > 510 || !info.remaps.len().is_multiple_of(2) {
            return None;
        }

        Some(Self(info))
    }
}

impl EventEncode for SpawnGroupEvent {
    fn size_bound(&self) -> Option<usize> {
        Some(16 + self.0.remaps.len() * 8)
    }

    fn id() -> &'static str {
        "globed/scripting.spawn-group"
    }

    fn encode(&self, writer: &mut HeapByteWriter) {
        let flags_pos = writer.pos();
        writer.write_u8(0); // flags, set later
        let _ = writer.write_varuint(self.0.group_id as u64);

        let mut flags = SpawnInfoFlags::default();

        if self.0.delay != 0.0 {
            flags.set_has_delay(true);
            writer.write_f32(self.0.delay);

            if self.0.delay_variance != 0.0 {
                flags.set_has_delay_variance(true);
                writer.write_f32(self.0.delay_variance);
            }
        }

        flags.set_ordered(self.0.ordered);

        if !self.0.remaps.is_empty() {
            flags.set_has_remaps(true);
            writer.write_u8((self.0.remaps.len() / 2) as u8);

            for key in self.0.remaps.iter() {
                let _ = writer.write_varuint(*key as u64);
            }
        }

        writer.perform_at(flags_pos, |w| {
            w.write_u8(flags.to_bits());
        });
    }
}

// Fire server event

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum IntOrFloat {
    Int(i32),
    Float(f32),
}

pub struct CustomScriptEvent {
    pub r#type: u16,
    pub args: heapless::Vec<IntOrFloat, 5>,
}

impl CustomScriptEvent {
    pub fn decode(data: &[u8]) -> Result<Self, DataDecodeError> {
        let mut args = heapless::Vec::new();
        let mut reader = ByteReader::new(data);

        let r#type = reader.read_u16()?;
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

        Ok(CustomScriptEvent { r#type, args })
    }
}

// Other scripting events

pub struct SetItemEvent {
    pub item_id: u32,
    pub value: i32,
}

impl EventEncode for SetItemEvent {
    fn size_bound(&self) -> Option<usize> {
        Some(10)
    }

    fn id() -> &'static str {
        "globed/scripting.set-item"
    }

    fn encode(&self, writer: &mut HeapByteWriter) {
        let _ = writer.write_varuint(self.item_id as u64);
        let _ = writer.write_varint(self.value as i64);
    }
}

pub struct MoveGroupEvent {
    pub group: u16,
    pub dx: f32,
    pub dy: f32,
}

impl EventEncode for MoveGroupEvent {
    fn size_bound(&self) -> Option<usize> {
        Some(11)
    }

    fn id() -> &'static str {
        "globed/scripting.move-group"
    }

    fn encode(&self, writer: &mut HeapByteWriter) {
        let _ = writer.write_varuint(self.group as u64);
        writer.write_f32(self.dx);
        writer.write_f32(self.dy);
    }
}

pub struct MoveGroupAbsoluteEvent {
    pub group: u16,
    pub center: u16,
    pub x: f32,
    pub y: f32,
}

impl EventEncode for MoveGroupAbsoluteEvent {
    fn size_bound(&self) -> Option<usize> {
        Some(13)
    }

    fn id() -> &'static str {
        "globed/scripting.move-group-absolute"
    }

    fn encode(&self, writer: &mut HeapByteWriter) {
        let _ = writer.write_varuint(self.group as u64);
        let _ = writer.write_varuint(self.center as u64);
        writer.write_f32(self.x);
        writer.write_f32(self.y);
    }
}

pub struct FollowPlayerEvent {
    pub player_id: i32,
    pub group: u16,
    pub enable: bool,
}

impl EventEncode for FollowPlayerEvent {
    fn size_bound(&self) -> Option<usize> {
        Some(6)
    }

    fn id() -> &'static str {
        "globed/scripting.follow-player"
    }

    fn encode(&self, writer: &mut HeapByteWriter) {
        let mut group = self.group;

        if self.enable {
            // set top bit
            group |= 1 << 15;
        } else {
            // clear top bit
            group &= !(1 << 15);
        }

        writer.write_u16(group);
        writer.write_i32(self.player_id);
    }
}

pub struct FollowRotationEvent {
    pub player_id: i32,
    pub group: u16,
    pub center: u16,
    pub enable: bool,
}

impl EventEncode for FollowRotationEvent {
    fn size_bound(&self) -> Option<usize> {
        Some(8)
    }

    fn id() -> &'static str {
        "globed/scripting.follow-rotation"
    }

    fn encode(&self, writer: &mut HeapByteWriter) {
        let mut group = self.group;

        if self.enable {
            // set top bit
            group |= 1 << 15;
        } else {
            // clear top bit
            group &= !(1 << 15);
        }

        writer.write_u16(group);
        writer.write_u16(self.center);
        writer.write_i32(self.player_id);
    }
}

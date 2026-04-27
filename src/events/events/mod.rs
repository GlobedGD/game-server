use server_shared::qunet::buffers::{BinaryWriter, ByteReader, ByteWriter, ByteWriterError};
use smallvec::SmallVec;
use thiserror::Error;
use super::*;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum IntOrFloat {
    Int(i32),
    Float(f32),
}

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

impl EventEncode for CounterChangeEvent {
    fn size_bound(&self) -> Option<usize> {
        Some(8)
    }

    fn encode(&self, writer: &mut impl std::io::Write) -> std::io::Result<()> {
        let mut writer = BinaryWriter::new(writer);

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

        writer.write_u64(packed_data)?;

        Ok(())
    }
}

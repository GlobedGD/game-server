mod ids;
mod r#in;
mod out;

pub use ids::*;
pub use r#in::*;
pub use out::*;

use server_shared::qunet::buffers::{ByteReader, ByteWriter, ByteWriterError};
use smallvec::SmallVec;
use thiserror::Error;

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

#[derive(Debug, Error)]
pub enum EventEncodeError {
    #[error("{0}")]
    Encode(#[from] ByteWriterError),
    #[error("Invalid event data")]
    InvalidData,
}

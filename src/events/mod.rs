mod ids;
mod r#in;
mod legacy;
mod out;
mod events;

pub use ids::*;
pub use r#in::*;
pub use legacy::*;
pub use out::*;
pub use events::*;

use server_shared::qunet::buffers::{ByteWriter, ByteWriterError};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum EventEncodeError {
    #[error("{0}")]
    Encode(#[from] ByteWriterError),
    #[error("Invalid event data")]
    InvalidData,
}

pub trait EventEncode {
    fn size_bound(&self) -> Option<usize>;
    fn encode(&self, writer: &mut impl std::io::Write) -> std::io::Result<()>;
}

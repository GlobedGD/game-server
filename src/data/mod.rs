pub use server_shared::{encoding::*, schema::game::*};

macro_rules! decode_message_match {
    ($this:expr, $data:expr, {$($variant:ident($msg_var:ident) => {  $($t:tt)* }),* $(,)?}) => {
        server_shared::decode_message_match!(server_shared::schema::game, $this.server(), $data, {$($variant($msg_var) => {  $($t)* }),*})
    };
}

#[allow(unused)]
macro_rules! encode_message_unsafe {
    ($this:expr, $estcap:expr, $msg:ident => $code:expr) => {
        server_shared::encode_message_unsafe!(server_shared::schema::game, $this.server(), $estcap, $msg => $code)
    }
}

macro_rules! encode_message_heap {
    ($this:expr, $estcap:expr, $msg:ident => $code:expr) => {
        server_shared::encode_message_heap!(server_shared::schema::game, $this.server(), $estcap, $msg => $code)
    }
}

macro_rules! encode_message {
    ($this:expr, $estcap:expr, $msg:ident => $code:expr) => {
        server_shared::encode_message!(server_shared::schema::game, $this.server(), $estcap, $msg => $code)
    }
}

pub(crate) use decode_message_match;
pub(crate) use encode_message;
pub(crate) use encode_message_heap;

pub fn heapless_str_from_reader<'a, const N: usize>(
    reader: capnp::text::Reader<'a>,
) -> Result<heapless::String<N>, DataDecodeError> {
    let s = reader.to_str()?;
    heapless::String::try_from(s).map_err(|_| DataDecodeError::StringTooLong(s.len(), N))
}

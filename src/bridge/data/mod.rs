macro_rules! decode_message_match {
    ($this:expr, $data:expr, {$($variant:ident($msg_var:ident) => {  $($t:tt)* }),* $(,)?}) => {
        server_shared::decode_message_match!(server_shared::schema::srvc, $this.server(), $data, {$($variant($msg_var) => {  $($t)* }),*})
    };
}

macro_rules! encode_message_unsafe {
    ($this:expr, $estcap:expr, $msg:ident => $code:expr) => {
        server_shared::encode_message_unsafe!(server_shared::schema::srvc, $this.server(), $estcap, $msg => $code)
    }
}

macro_rules! encode_message_heap {
    ($this:expr, $estcap:expr, $msg:ident => $code:expr) => {
        server_shared::encode_message_heap!(server_shared::schema::srvc, $this.server(), $estcap, $msg => $code)
    }
}

pub(crate) use decode_message_match;
pub(crate) use encode_message_heap;
pub(crate) use encode_message_unsafe;

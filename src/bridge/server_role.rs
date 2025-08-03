use server_shared::encoding::{DataDecodeError, heapless_str_from_reader};

use super::data;

pub struct ServerRole {
    pub id: u8,
    pub string_id: heapless::String<32>,
}

impl ServerRole {
    pub fn from_reader(reader: data::server_role::Reader<'_>) -> Result<Self, DataDecodeError> {
        let id = reader.get_id();
        let string_id = heapless_str_from_reader(reader.get_string_id()?)?;

        Ok(Self { id, string_id })
    }
}

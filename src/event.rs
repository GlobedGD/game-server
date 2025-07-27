use server_shared::encoding::DataDecodeError;

use crate::data::event;

pub const EVENT_COUNTER_CHANGE: u16 = 1;

pub enum CounterChangeType {
    Set(i32),
    Add(i32),
    Multiply(f32),
    Divide(f32),
}

pub struct CounterChangeEvent {
    pub item_id: u32,
    pub r#type: CounterChangeType,
}

pub enum Event {
    CounterChange(CounterChangeEvent),
}

impl Event {
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

            _ => Err(DataDecodeError::ValidationFailed),
        }
    }

    pub fn type_int(&self) -> u16 {
        match self {
            Event::CounterChange(_) => EVENT_COUNTER_CHANGE,
        }
    }

    pub fn encode(&self, mut writer: event::Builder<'_>) {
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
        }
    }
}

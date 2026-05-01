use std::{collections::HashMap, io::Write, sync::Arc};

use super::ids::*;
use server_shared::{
    events::{
        EventDecodingError as NewEventDecodingError, EventDictionaryBuildError,
        EventEncoder as NewEventEncoder, EventEncodingError as NewEventEncodingError, EventOptions,
        EventStringCache, OwnedEvent,
    },
    qunet::buffers::{BinaryWriter, ByteReader, ByteReaderError, HeapByteWriter},
};
use thiserror::Error;
use tracing::debug;

#[derive(Error, Debug)]
pub enum EventEncodingError {
    #[error("{0}")]
    New(#[from] NewEventEncodingError),
    #[error("Cannot encode this event with the legacy encoder")]
    UnknownLegacyEvent,
    #[error("Failed to write legacy event data: {0}")]
    Write(#[from] std::io::Error),
}

#[derive(Error, Debug)]
pub enum EventDecodingError {
    #[error("{0}")]
    New(#[from] NewEventDecodingError),
    #[error("Cannot decode this event with the legacy decoder")]
    UnknownLegacyEvent,
}

impl From<ByteReaderError> for EventDecodingError {
    fn from(value: ByteReaderError) -> Self {
        Self::New(value.into())
    }
}

pub struct LegacyEventEncoder {
    mapping: HashMap<u16, Arc<str>>,
    inv_mapping: HashMap<Arc<str>, u16>,
    custom_id: Arc<str>,
}

pub enum EventEncoder {
    Legacy(Arc<LegacyEventEncoder>),
    New(NewEventEncoder),
}

impl LegacyEventEncoder {
    pub fn create(cache: &EventStringCache) -> Arc<Self> {
        let mut mapping = HashMap::new();
        let mut inv_mapping = HashMap::new();

        let mut insert_one = |id: u16, str_id: &str| {
            let str_id = cache.get(str_id);
            mapping.insert(id, str_id.clone());
            inv_mapping.insert(str_id, id);
        };

        insert_one(EVENT_COUNTER_CHANGE, "globed/counter-change");
        insert_one(EVENT_DISPLAY_DATA_REFRESHED, "globed/display-data-refreshed");

        let custom_id = cache.get("globed/scripting.custom");
        insert_one(0, &custom_id); // one-way event, numeric id doesnt matter
        insert_one(EVENT_SCR_SPAWN_GROUP, "globed/scripting.spawn-group");
        insert_one(EVENT_SCR_SET_ITEM, "globed/scripting.set-item");
        insert_one(EVENT_SCR_REQUEST_SCRIPT_LOGS, "globed/scripting.request-script-logs");
        insert_one(EVENT_SCR_MOVE_GROUP, "globed/scripting.move-group");
        insert_one(EVENT_SCR_MOVE_GROUP_ABSOLUTE, "globed/scripting.move-group-absolute");
        insert_one(EVENT_SCR_FOLLOW_PLAYER, "globed/scripting.follow-player");
        insert_one(EVENT_SCR_FOLLOW_ROTATION, "globed/scripting.follow-rotation");

        insert_one(EVENT_2P_LINK_REQUEST, "globed/2p.link");
        insert_one(EVENT_2P_UNLINK, "globed/2p.unlink");

        insert_one(EVENT_SWITCHEROO_FULL_STATE, "globed/switcheroo.full-state");
        insert_one(EVENT_SWITCHEROO_SWITCH, "globed/switcheroo.switch");

        Arc::new(Self {
            mapping,
            inv_mapping,
            custom_id,
        })
    }

    pub fn knows_event(&self, id: &str) -> bool {
        self.inv_mapping.contains_key(id)
    }

    pub fn encode_event(
        &self,
        id: &str,
        data: &[u8],
        options: &EventOptions,
        writer: &mut impl Write,
    ) -> Result<(), EventEncodingError> {
        let mut writer = BinaryWriter::new(writer);

        if id == &*self.custom_id {
            // data already includes the type at the start
            writer.write_bytes(data)?;
        } else {
            let id = *self.inv_mapping.get(id).ok_or(EventEncodingError::UnknownLegacyEvent)?;
            writer.write_u16(id)?;

            // encode 'sent by' for 2p
            if id == EVENT_2P_LINK_REQUEST || id == EVENT_2P_UNLINK {
                writer.write_i32(options.sent_by_player.unwrap_or_default())?;
            }

            writer.write_bytes(data)?;
        }

        Ok(())
    }

    pub fn encode_events(
        &self,
        events: &[OwnedEvent],
        writer: &mut impl Write,
    ) -> Result<(), EventEncodingError> {
        let mut writer = BinaryWriter::new(writer);

        for event in events {
            if let Err(e) = self.encode_event(&event.id, &event.data, &event.options, &mut writer) {
                debug!("legacy writer failed to encode event {}: {e}", event.id);
            }
        }

        Ok(())
    }

    pub fn decode_events_owned(&self, data: &[u8]) -> Result<Vec<OwnedEvent>, EventDecodingError> {
        let mut reader = ByteReader::new(data);
        let mut events = Vec::new();

        while reader.remaining() > 0 {
            let ty = reader.read_u16()?;
            let len = length_for_legacy_event(ty, reader.remaining_bytes())
                .ok_or(EventDecodingError::UnknownLegacyEvent)?;

            let mut data = reader.skip_bytes(len)?;
            let mut options = EventOptions {
                // legacy events were assumed to always be reliable
                reliable: true,
                ..Default::default()
            };

            let (id, data) = if ty < EVENT_GLOBED_BASE {
                let mut buf = HeapByteWriter::new();
                buf.write_u16(ty);
                buf.write_bytes(data);

                (self.custom_id.clone(), buf.into_vec())
            } else {
                let id =
                    self.mapping.get(&ty).ok_or(EventDecodingError::UnknownLegacyEvent)?.clone();

                if ty == EVENT_2P_LINK_REQUEST || ty == EVENT_2P_UNLINK {
                    // these events provide a player ID as part of their data, but we want to strip that into the options
                    let mut reader = ByteReader::new(data);
                    let target = reader.read_i32()?;
                    options.target_players = vec![target];

                    // for link request also flip the bool lol, since the server used to do that before
                    if ty == EVENT_2P_LINK_REQUEST {
                        let p1 = reader.read_bool()?;
                        data = std::slice::from_ref(if p1 { &0u8 } else { &1u8 });
                    } else {
                        data = &[]
                    }
                } else if ty == EVENT_SWITCHEROO_FULL_STATE || ty == EVENT_SWITCHEROO_SWITCH {
                    // switcheroo events were also sent back to the player that sent them
                    options.send_back = true;
                }

                (id, data.to_vec())
            };

            events.push(OwnedEvent { id, data, options });
        }

        Ok(events)
    }
}

impl EventEncoder {
    pub fn new(dict: &[u8], cache: &EventStringCache) -> Result<Self, EventDictionaryBuildError> {
        Ok(Self::New(NewEventEncoder::create_with_dictionary(dict, cache, true)?))
    }

    pub fn is_legacy(&self) -> bool {
        matches!(self, Self::Legacy(_))
    }

    pub fn knows_event(&self, id: &str) -> bool {
        match self {
            EventEncoder::Legacy(encoder) => encoder.knows_event(id),
            EventEncoder::New(encoder) => encoder.knows_event(id),
        }
    }

    pub fn encode_event(
        &self,
        id: &str,
        data: &[u8],
        options: &EventOptions,
        writer: &mut impl Write,
    ) -> Result<(), EventEncodingError> {
        match self {
            EventEncoder::Legacy(encoder) => encoder.encode_event(id, data, options, writer),
            EventEncoder::New(encoder) => Ok(encoder.encode_event(id, data, options, writer)?),
        }
    }

    pub fn encode_events(
        &self,
        events: &[OwnedEvent],
        writer: &mut impl Write,
    ) -> Result<(), EventEncodingError> {
        match self {
            EventEncoder::Legacy(encoder) => encoder.encode_events(events, writer),
            EventEncoder::New(encoder) => Ok(encoder.encode_events(events, writer)?),
        }
    }

    pub fn decode_events_owned(&self, data: &[u8]) -> Result<Vec<OwnedEvent>, EventDecodingError> {
        match self {
            EventEncoder::Legacy(encoder) => encoder.decode_events_owned(data),
            EventEncoder::New(encoder) => Ok(encoder.decode_events_owned(data)?),
        }
    }
}

#[allow(non_contiguous_range_endpoints)]
fn length_for_legacy_event(id: u16, data: &[u8]) -> Option<usize> {
    let mut reader = ByteReader::new(data);

    Some(match id {
        EVENT_COUNTER_CHANGE => 8,
        EVENT_SCR_REQUEST_SCRIPT_LOGS => 0,
        EVENT_2P_LINK_REQUEST => 5,
        EVENT_2P_UNLINK => 4,
        EVENT_SWITCHEROO_FULL_STATE => 5,
        EVENT_SWITCHEROO_SWITCH => 5,

        0..EVENT_GLOBED_BASE => {
            // old scripting events
            let count = reader.read_u8().ok()?;

            // 1 byte for count
            // 1 byte for argument types (bitmask)
            // 4 * n bytes for arguments
            2 + (count as usize) * 4
        }

        _ => return None,
    })
}

use std::{collections::HashMap, io::Write, sync::Arc};

use super::ids::*;
use server_shared::{
    events::{
        EventDecodingError as NewEventDecodingError, EventDictionaryBuildError,
        EventEncoder as NewEventEncoder, EventEncodingError as NewEventEncodingError, EventOptions,
        EventStringCache, OwnedEvent,
    },
    qunet::buffers::{BinaryWriter, ByteReader},
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

pub struct LegacyEventEncoder {
    mapping: HashMap<u16, Arc<str>>,
    inv_mapping: HashMap<Arc<str>, u16>,
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

        insert_one(EVENT_COUNTER_CHANGE, "dankmeme.globed2/counter-change");
        insert_one(EVENT_PLAYER_JOIN, "dankmeme.globed2/player-join");
        insert_one(EVENT_PLAYER_LEAVE, "dankmeme.globed2/player-leave");
        insert_one(EVENT_DISPLAY_DATA_REFRESHED, "dankmeme.globed2/display-data-refreshed");

        insert_one(EVENT_SCR_SPAWN_GROUP, "dankmeme.globed2/scripting.spawn-group");
        insert_one(EVENT_SCR_SET_ITEM, "dankmeme.globed2/scripting.set-item");
        insert_one(EVENT_SCR_REQUEST_SCRIPT_LOGS, "dankmeme.globed2/scripting.request-script-logs");
        insert_one(EVENT_SCR_MOVE_GROUP, "dankmeme.globed2/scripting.move-group");
        insert_one(EVENT_SCR_MOVE_GROUP_ABSOLUTE, "dankmeme.globed2/scripting.move-group-absolute");
        insert_one(EVENT_SCR_FOLLOW_PLAYER, "dankmeme.globed2/scripting.follow-player");
        insert_one(EVENT_SCR_FOLLOW_ROTATION, "dankmeme.globed2/scripting.follow-rotation");

        insert_one(EVENT_2P_LINK_REQUEST, "dankmeme.globed2/2p.link-request");
        insert_one(EVENT_2P_UNLINK, "dankmeme.globed2/2p.unlink");

        insert_one(EVENT_SWITCHEROO_FULL_STATE, "dankmeme.globed2/switcheroo.full-state");
        insert_one(EVENT_SWITCHEROO_SWITCH, "dankmeme.globed2/switcheroo.switch");

        Arc::new(Self { mapping, inv_mapping })
    }

    pub fn encode_event(
        &self,
        id: &str,
        data: &[u8],
        _options: &EventOptions,
        writer: &mut impl Write,
    ) -> Result<(), EventEncodingError> {
        let mut writer = BinaryWriter::new(writer);

        let id = *self.inv_mapping.get(id).ok_or(EventEncodingError::UnknownLegacyEvent)?;
        writer.write_u16(id)?;
        writer.write_bytes(data)?;

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
            let ty = reader.read_u16().map_err(NewEventDecodingError::from)?;
            let len = length_for_legacy_event(ty, reader.remaining_bytes())
                .ok_or(EventDecodingError::UnknownLegacyEvent)?;

            let data = reader.skip_bytes(len).map_err(NewEventDecodingError::from)?;
            let str_id =
                self.mapping.get(&ty).ok_or(EventDecodingError::UnknownLegacyEvent)?.clone();

            events.push(OwnedEvent {
                id: str_id,
                data: data.to_vec(),
                // legacy events were assumed to always be reliable
                options: EventOptions {
                    reliable: true,
                    ..Default::default()
                },
            });
        }

        Ok(events)
    }
}

impl EventEncoder {
    pub fn new(dict: &[u8], cache: &EventStringCache) -> Result<Self, EventDictionaryBuildError> {
        Ok(Self::New(NewEventEncoder::create_with_dictionary(dict, cache, true)?))
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

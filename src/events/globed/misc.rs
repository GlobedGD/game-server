use server_shared::{events::EventEncode, qunet::buffers::HeapByteWriter};

pub struct DisplayDataRefreshedEvent {
    pub player: i32,
}

impl EventEncode for DisplayDataRefreshedEvent {
    fn size_bound(&self) -> Option<usize> {
        Some(4)
    }

    fn id() -> &'static str {
        "globed/display-data-refreshed"
    }

    fn encode(&self, writer: &mut HeapByteWriter) {
        writer.write_i32(self.player);
    }
}

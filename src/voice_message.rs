use std::sync::Arc;

use server_shared::encoding::DataDecodeError;

pub struct VoiceMessage {
    from: i32,
    splits: heapless::Vec<usize, 16>,
    data: Vec<u8>,
}

impl VoiceMessage {
    pub fn encoded_len(&self) -> usize {
        64 + 16 * self.splits.len() + self.data.len()
    }

    pub fn sender(&self) -> i32 {
        self.from
    }

    pub fn decode(
        account_id: i32,
        input: crate::data::voice_data_message::Reader<'_>,
    ) -> Result<Arc<Self>, DataDecodeError> {
        let mut data = Vec::new();
        let mut splits = heapless::Vec::new();

        let total_size =
            input.get_frames()?.iter().map(|x| x.map(|x| x.len()).unwrap_or(0)).sum::<usize>();
        data.reserve(total_size);

        for frame in input.get_frames()? {
            let frame = frame?;
            data.extend_from_slice(frame);
            splits.push(frame.len()).map_err(|_| DataDecodeError::ValidationFailed)?;
        }

        Ok(Arc::new(VoiceMessage { from: account_id, splits, data }))
    }

    pub fn encode(&self, mut writer: crate::data::voice_broadcast_message::Builder<'_>) {
        writer.set_account_id(self.from);

        let mut out = writer.reborrow().init_frames(self.splits.len() as u32);

        let mut offset = 0;
        for (i, len) in self.splits.iter().enumerate() {
            let frame = &self.data[offset..(offset + len)];
            out.reborrow().set(i as u32, frame);
            offset += len;
        }
    }
}

use dashmap::DashMap;

use crate::event::{CounterChangeEvent, CounterChangeType};

#[derive(Default)]
pub struct TriggerManager {
    values: DashMap<u32, i32>,
}

impl TriggerManager {
    pub fn handle_change(&self, event: &CounterChangeEvent) -> (u32, i32) {
        let mut entry = self.values.entry(event.item_id).or_insert(0);

        match event.r#type {
            CounterChangeType::Add(val) => {
                *entry = entry.wrapping_add(val);
            }

            CounterChangeType::Set(val) => {
                *entry = val;
            }

            CounterChangeType::Multiply(val) => {
                if val.is_finite() {
                    *entry = ((*entry as f32) * val) as i32;
                }
            }

            CounterChangeType::Divide(val) => {
                if val != 0.0 && val.is_finite() {
                    *entry = ((*entry as f32) / val) as i32;
                }
            }
        }

        (event.item_id, *entry)
    }
}

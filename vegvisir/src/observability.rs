use std::{
    collections::VecDeque,
    fs::OpenOptions,
    io::Write,
    path::PathBuf,
    sync::{Arc, Mutex},
};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

const DEFAULT_EVENT_MEMORY_MAX_EVENTS: usize = 512;
const EVENT_PAYLOAD_MAX_BYTES: usize = 32 * 1024;
const EVENT_STRING_MAX_BYTES: usize = 8 * 1024;
const EVENT_MAX_DEPTH: usize = 8;
const EVENT_MAX_COLLECTION_ITEMS: usize = 64;
const EVENT_PAYLOAD_OMITTED: &str = "[event payload omitted by memory bound]";
const EVENT_PAYLOAD_TRUNCATED: &str = "[event value truncated by memory bound]";

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Event {
    pub name: String,
    #[serde(default)]
    pub payload: Value,
    pub timestamp: DateTime<Utc>,
}

#[derive(Clone, Debug)]
pub struct EventLogger {
    path: Option<PathBuf>,
    events: Arc<Mutex<VecDeque<Event>>>,
    max_events: usize,
}

impl Default for EventLogger {
    fn default() -> Self {
        Self::new(None)
    }
}

impl EventLogger {
    pub fn new(path: Option<PathBuf>) -> Self {
        if let Some(path) = &path
            && let Some(parent) = path.parent()
        {
            let _ = std::fs::create_dir_all(parent);
        }
        Self {
            path,
            events: Arc::new(Mutex::new(VecDeque::new())),
            max_events: DEFAULT_EVENT_MEMORY_MAX_EVENTS,
        }
    }

    /// Set the number of recent events retained in process memory.
    ///
    /// The JSONL trace remains the durable event archive; this buffer is only
    /// for recent UI/diagnostic queries and must not grow with the lifetime of
    /// the process.
    pub fn with_memory_max_events(mut self, max_events: usize) -> Self {
        self.max_events = max_events.max(1);
        self
    }

    pub fn events(&self) -> Vec<Event> {
        self.events
            .lock()
            .map(|events| events.iter().cloned().collect())
            .unwrap_or_default()
    }

    pub fn len(&self) -> usize {
        self.events.lock().map(|events| events.len()).unwrap_or(0)
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn emit(&self, name: impl Into<String>, payload: Value) {
        // Tool arguments and provider/tool diagnostics can contain complete
        // file contents. Bound the retained and durable representation before
        // either the in-memory ring or JSONL writer sees it.
        let mut budget = EVENT_PAYLOAD_MAX_BYTES;
        let payload = bound_event_value(payload, &mut budget, 0);
        let event = Event {
            name: name.into(),
            payload,
            timestamp: Utc::now(),
        };
        if let Ok(mut events) = self.events.lock() {
            events.push_back(event.clone());
            while events.len() > self.max_events {
                events.pop_front();
            }
        }
        if let Some(path) = &self.path
            && let Ok(mut file) = OpenOptions::new().create(true).append(true).open(path)
        {
            let _ = writeln!(
                file,
                "{}",
                serde_json::to_string(&event).unwrap_or_default()
            );
        }
    }
}

fn bound_event_value(value: Value, budget: &mut usize, depth: usize) -> Value {
    if depth >= EVENT_MAX_DEPTH || *budget == 0 {
        return Value::String(EVENT_PAYLOAD_OMITTED.to_string());
    }

    match value {
        Value::String(text) => {
            let max_bytes = (*budget).min(EVENT_STRING_MAX_BYTES);
            let bounded = bounded_text(&text, max_bytes);
            *budget = budget.saturating_sub(bounded.len());
            Value::String(bounded)
        }
        Value::Array(values) => {
            let original_len = values.len();
            let mut bounded = Vec::with_capacity(original_len.min(EVENT_MAX_COLLECTION_ITEMS));
            for value in values.into_iter().take(EVENT_MAX_COLLECTION_ITEMS) {
                if *budget == 0 {
                    break;
                }
                bounded.push(bound_event_value(value, budget, depth + 1));
            }
            if bounded.len() < original_len.min(EVENT_MAX_COLLECTION_ITEMS) {
                bounded.push(Value::String(EVENT_PAYLOAD_OMITTED.to_string()));
            }
            if original_len > EVENT_MAX_COLLECTION_ITEMS {
                bounded.push(Value::String(EVENT_PAYLOAD_OMITTED.to_string()));
            }
            Value::Array(bounded)
        }
        Value::Object(values) => {
            let original_len = values.len();
            let mut bounded = Map::new();
            for (key, value) in values.into_iter().take(EVENT_MAX_COLLECTION_ITEMS) {
                if *budget == 0 {
                    break;
                }
                let bounded_key = bounded_text(&key, EVENT_STRING_MAX_BYTES.min(*budget));
                *budget = budget.saturating_sub(bounded_key.len());
                bounded.insert(bounded_key, bound_event_value(value, budget, depth + 1));
            }
            if original_len > EVENT_MAX_COLLECTION_ITEMS {
                bounded.insert(
                    "_vegvisir_omitted_fields".to_string(),
                    Value::String(EVENT_PAYLOAD_OMITTED.to_string()),
                );
            }
            Value::Object(bounded)
        }
        other => {
            *budget = budget.saturating_sub(32);
            other
        }
    }
}

fn bounded_text(text: &str, max_bytes: usize) -> String {
    if text.len() <= max_bytes {
        return text.to_string();
    }
    if max_bytes <= EVENT_PAYLOAD_TRUNCATED.len() {
        return text
            .char_indices()
            .take_while(|(index, _)| *index < max_bytes)
            .map(|(_, ch)| ch)
            .collect();
    }

    let available = max_bytes - EVENT_PAYLOAD_TRUNCATED.len();
    let head_limit = available.saturating_mul(2) / 3;
    let tail_limit = available.saturating_sub(head_limit);
    let head_end = safe_char_boundary_at_or_before(text, head_limit);
    let tail_start = safe_char_boundary_at_or_after(text, text.len().saturating_sub(tail_limit));
    format!(
        "{}{}{}",
        &text[..head_end],
        EVENT_PAYLOAD_TRUNCATED,
        &text[tail_start..]
    )
}

fn safe_char_boundary_at_or_before(text: &str, mut index: usize) -> usize {
    index = index.min(text.len());
    while index > 0 && !text.is_char_boundary(index) {
        index -= 1;
    }
    index
}

fn safe_char_boundary_at_or_after(text: &str, mut index: usize) -> usize {
    index = index.min(text.len());
    while index < text.len() && !text.is_char_boundary(index) {
        index += 1;
    }
    index
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn logger_retains_only_a_bounded_recent_ring() {
        let logger = EventLogger::new(None).with_memory_max_events(2);
        logger.emit("one", json!(1));
        logger.emit("two", json!(2));
        logger.emit("three", json!(3));

        let events = logger.events();
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].name, "two");
        assert_eq!(events[1].name, "three");
    }

    #[test]
    fn logger_bounds_large_nested_payloads_before_retaining_them() {
        let logger = EventLogger::new(None);
        logger.emit(
            "large",
            json!({
                "content": "x".repeat(EVENT_STRING_MAX_BYTES * 4),
                "nested": {"content": "y".repeat(EVENT_STRING_MAX_BYTES * 4)},
            }),
        );

        let event = &logger.events()[0];
        let serialized = serde_json::to_vec(&event.payload).unwrap();
        assert!(serialized.len() <= EVENT_PAYLOAD_MAX_BYTES + 1024);
    }
}

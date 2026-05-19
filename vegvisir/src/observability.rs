use std::{
    fs::OpenOptions,
    io::Write,
    path::PathBuf,
    sync::{Arc, Mutex},
};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Event {
    pub name: String,
    #[serde(default)]
    pub payload: Value,
    pub timestamp: DateTime<Utc>,
}

#[derive(Clone, Debug, Default)]
pub struct EventLogger {
    path: Option<PathBuf>,
    events: Arc<Mutex<Vec<Event>>>,
}

impl EventLogger {
    pub fn new(path: Option<PathBuf>) -> Self {
        if let Some(path) = &path {
            if let Some(parent) = path.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
        }
        Self {
            path,
            events: Arc::new(Mutex::new(Vec::new())),
        }
    }

    pub fn emit(&self, name: impl Into<String>, payload: Value) {
        let event = Event {
            name: name.into(),
            payload,
            timestamp: Utc::now(),
        };
        if let Ok(mut events) = self.events.lock() {
            events.push(event.clone());
        }
        if let Some(path) = &self.path {
            if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(path) {
                let _ = writeln!(
                    file,
                    "{}",
                    serde_json::to_string(&event).unwrap_or_default()
                );
            }
        }
    }

    pub fn events(&self) -> Vec<Event> {
        self.events
            .lock()
            .map(|events| events.clone())
            .unwrap_or_default()
    }
}

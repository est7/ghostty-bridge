use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

#[derive(Serialize, Deserialize, Default)]
struct LabelStore {
    labels: HashMap<String, String>,
}

fn store_path() -> PathBuf {
    let base = dirs::data_local_dir().unwrap_or_else(|| PathBuf::from("/tmp"));
    let dir = base.join("ghostty-bridge");
    let _ = fs::create_dir_all(&dir);
    dir.join("labels.json")
}

pub fn load() -> HashMap<String, String> {
    let path = store_path();
    let data = fs::read_to_string(&path).unwrap_or_default();
    let store: LabelStore = serde_json::from_str(&data).unwrap_or_default();
    store.labels
}

pub fn set(label: &str, id: &str) {
    let mut labels = load();
    labels.insert(label.to_string(), id.to_string());
    let store = LabelStore { labels };
    let path = store_path();
    if let Ok(json) = serde_json::to_string_pretty(&store) {
        let _ = fs::write(&path, json);
    }
}

pub fn resolve(label: &str) -> Option<String> {
    let labels = load();
    labels.get(label).cloned()
}

use std::collections::BTreeMap;
use std::io::ErrorKind;
use std::path::Path;

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde::Serialize;
use serde::de::DeserializeOwned;

use crate::html_parse::Record;
use crate::indicatif::Indicatif;
use crate::search::Session;

pub type Vides = BTreeMap<Indicatif, DateTime<Utc>>;

pub fn read_json<T: DeserializeOwned>(path: &Path, label: &str) -> Result<T> {
    let text = std::fs::read_to_string(path)
        .with_context(|| format!("lecture {label} {}", path.display()))?;
    serde_json::from_str(&text).with_context(|| format!("parsing {label} {}", path.display()))
}

pub fn read_json_or_default<T: DeserializeOwned + Default>(path: &Path, label: &str) -> Result<T> {
    match std::fs::read_to_string(path) {
        Ok(text) => serde_json::from_str(&text)
            .with_context(|| format!("parsing {label} {}", path.display())),
        Err(e) if e.kind() == ErrorKind::NotFound => Ok(T::default()),
        Err(e) => Err(e).with_context(|| format!("lecture {label} {}", path.display())),
    }
}

pub fn write_json_pretty<T: Serialize>(path: &Path, value: &T, label: &str) -> Result<()> {
    let json = serde_json::to_string_pretty(value)?;
    std::fs::write(path, json).with_context(|| format!("écriture {label} {}", path.display()))
}

pub fn load_session(path: &Path) -> Result<Session> {
    read_json(path, "session")
}

pub fn save_session(path: &Path, session: &Session) -> Result<()> {
    write_json_pretty(path, session, "session")
}

pub fn load_records(path: &Path) -> Result<Vec<Record>> {
    read_json_or_default(path, "records")
}

pub fn save_records(path: &Path, records: &[Record]) -> Result<()> {
    let mut sorted: Vec<&Record> = records.iter().collect();
    sorted.sort_by(|a, b| a.indicatif.as_str().cmp(b.indicatif.as_str()));
    write_json_pretty(path, &sorted, "records")
}

pub fn load_vides(path: &Path) -> Result<Vides> {
    read_json_or_default(path, "vides")
}

pub fn save_vides(path: &Path, vides: &Vides) -> Result<()> {
    write_json_pretty(path, vides, "vides")
}

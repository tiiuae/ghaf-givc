use std::collections::hash_map::{HashMap,Entry};
use std::fmt;
use std::sync::{Arc,Mutex};
use std::result::Result;

use crate::types::*;

#[derive(Clone, Debug)]
struct Registry {
    /// The shared state is guarded by a mutex. This is a `std::sync::Mutex` and
    /// not a Tokio mutex. This is because there are no asynchronous operations
    /// being performed while holding the mutex. Additionally, the critical
    /// sections are very small.
    map: Arc<Mutex<HashMap<String,RegistryEntry>>> 
}

impl Registry {
    pub fn new() -> Self {
        Self {
            map: Arc::new(Mutex::new(HashMap::new()))
        }
    }

    pub fn register(&self, entry: RegistryEntry) {
        let mut state = self.map.lock().unwrap();
        state.entry(entry.name.clone()).or_insert(entry);
    }

    pub fn by_name(&self, name: String) -> Result<RegistryEntry, String> {
        let mut state = self.map.lock().unwrap();
        match state.entry(name.clone()) {
            Entry::Occupied(v) => Ok(v.get().clone()),
            Entry::Vacant(_) => Err(format!("Service {name} not registered")),
        }
    }

    pub fn by_type_many(&self, r#type: UnitType) -> Vec<RegistryEntry> {
        let state = self.map.lock().unwrap();
        state.values().filter(|x| x.r#type == r#type).map(|x| x.clone()).collect()
    }

    pub fn by_type(&self, r#type: UnitType) -> Result<RegistryEntry, String> {
        let vec = self.by_type_many(r#type);
        match vec.len() {
            1 => Ok(vec[0].clone()),
            0 => Err("No service registered for".to_string()),
            _ => Err("More than one unique services registered".to_string()), // FIXME: Fail registration, this situation should never happens
        }
    }

    pub fn create_unique_entry_name(&self, name: String) -> String {
        let state = self.map.lock().unwrap();
        let mut counter = 0;
        loop {
            let new_name = format!("{name}@{counter}.service");
            if !state.contains_key(&new_name) {
                return new_name;
            }
            counter += 1;
        };
    }

    pub fn watch_list(&self) -> Vec<RegistryEntry> {
        let state = self.map.lock().unwrap();
        state.values().filter(|x| x.watch).map(|x| x.clone()).collect()
    }
}

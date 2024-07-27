use std::collections::hash_map::HashMap;
use std::sync::{Arc, Mutex};

use super::entry::RegistryEntry;
use crate::types::*;
use anyhow::{anyhow, bail};
use tracing::info;

#[derive(Clone, Debug)]
pub struct Registry {
    /// The shared state is guarded by a mutex. This is a `std::sync::Mutex` and
    /// not a Tokio mutex. This is because there are no asynchronous operations
    /// being performed while holding the mutex. Additionally, the critical
    /// sections are very small.
    map: Arc<Mutex<HashMap<String, RegistryEntry>>>,
}

impl Registry {
    pub fn new() -> Self {
        Self {
            map: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub fn register(&self, entry: RegistryEntry) {
        let mut state = self.map.lock().unwrap();
        info!("Registering {:#?}", entry);
        match state.insert(entry.name.clone(), entry) {
            Some(old) => info!("Replaced old entry {:#?}", old),
            None => (),
        };
    }

    pub fn deregister(&self, name: &String) -> anyhow::Result<()> {
        let mut state = self.map.lock().unwrap();
        match state.remove(name) {
            Some(entry) => {
                info!("Deregistering {:#?}", entry);
                Ok(())
            }
            None => bail!("Can't deregister entry {}, it not registered", name),
        }
    }

    pub fn by_name(&self, name: &String) -> anyhow::Result<RegistryEntry> {
        let state = self.map.lock().unwrap();
        state
            .get(name)
            .cloned()
            .ok_or_else(|| anyhow!("Service {name} not registered"))
    }

    pub fn find_names(&self, name: &String) -> anyhow::Result<Vec<String>> {
        let state = self.map.lock().unwrap();
        let list: Vec<String> = state
            .values()
            .filter(|x| x.name.starts_with(name.as_str()))
            .map(|x| x.name.clone())
            .collect();
        if list.len() == 0 {
            bail!("No entries match string {}", name)
        } else {
            Ok(list)
        }
    }

    pub fn by_type_many(&self, ty: &UnitType) -> Vec<RegistryEntry> {
        let state = self.map.lock().unwrap();
        state
            .values()
            .filter(|x| x.r#type == *ty)
            .cloned()
            .collect()
    }

    pub fn by_type(&self, ty: &UnitType) -> anyhow::Result<RegistryEntry> {
        let vec = self.by_type_many(&ty);
        match vec.len() {
            1 => Ok(vec[0].clone()),
            0 => bail!("No service registered for"),
            _ => bail!("More than one unique services registered"), // FIXME: Fail registration, this situation should never happens
        }
    }

    pub fn contains(&self, name: &String) -> bool {
        let state = self.map.lock().unwrap();
        state.contains_key(name)
    }

    pub fn create_unique_entry_name(&self, name: &String) -> String {
        let state = self.map.lock().unwrap();
        let mut counter = 0;
        loop {
            let new_name = format!("{name}@{counter}.service");
            if !state.contains_key(&new_name) {
                return new_name;
            }
            counter += 1;
        }
    }

    pub fn watch_list(&self) -> Vec<RegistryEntry> {
        let state = self.map.lock().unwrap();
        state.values().filter(|x| x.watch).cloned().collect()
    }

    pub fn update_state(&self, name: &String, status: UnitStatus) -> anyhow::Result<()> {
        let mut state = self.map.lock().unwrap();
        if let Some(e) = state.get_mut(name) {
            e.status = status
        } else {
            bail!("Can't update state for {}, is not registered", name)
        };
        Ok(())
    }

    // FIXME: Should we dump full contents here for `query`/`query_list` high-level API
    // FIXME: .by_types_many() should works, but I add this one for debug convenience
    pub fn contents(&self) -> Vec<RegistryEntry> {
        let state = self.map.lock().unwrap();
        state.values().cloned().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::utils::naming::parse_application_name;

    #[test]
    fn test_register_deregister() -> anyhow::Result<()> {
        let r = Registry::new();

        let foo = RegistryEntry::dummy("foo".to_string());
        let foo_key = "foo".to_string();
        let bar = RegistryEntry::dummy("bar".to_string());

        r.register(foo.clone());
        r.register(bar);

        assert!(r.contains(&foo_key));
        assert!(r.contains(&"bar".to_string()));

        let foo1 = r.by_name(&foo_key)?;
        assert_eq!(foo1, foo);

        assert!(r.deregister(&foo_key).is_ok());
        assert!(!r.contains(&foo_key));
        assert!(r.by_name(&foo_key).is_err());
        assert!(r.deregister(&foo_key).is_err()); // fail to dereg second time
        Ok(())
    }

    #[test]
    fn test_unique_name() -> anyhow::Result<()> {
        let r = Registry::new();

        let foo = "foo".to_string();
        let name1 = r.create_unique_entry_name(&foo);
        assert_eq!(name1, "foo@0.service");
        let re1 = RegistryEntry::dummy(name1.clone());
        r.register(re1);

        let name2 = r.create_unique_entry_name(&foo);
        assert_eq!(name2, "foo@1.service");

        // Integration test -- ensure all names are parsable
        assert!(parse_application_name(&name1).is_ok());
        assert!(parse_application_name(&name2).is_ok());
        Ok(())
    }
}

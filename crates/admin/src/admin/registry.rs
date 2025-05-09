use std::collections::hash_map::HashMap;
use std::sync::{Arc, Mutex};

use super::entry::RegistryEntry;
use crate::types::{UnitStatus, UnitType};
use anyhow::{anyhow, bail};
use givc_common::query::{Event, QueryResult};
use tokio::sync::broadcast;
use tracing::{debug, error, info};

#[derive(Clone, Debug)]
pub struct Registry {
    /// The shared state is guarded by a mutex. This is a `std::sync::Mutex` and
    /// not a Tokio mutex. This is because there are no asynchronous operations
    /// being performed while holding the mutex. Additionally, the critical
    /// sections are very small.
    map: Arc<Mutex<HashMap<String, RegistryEntry>>>,
    pubsub: broadcast::Sender<Event>,
}

impl Default for Registry {
    fn default() -> Self {
        Self::new()
    }
}

impl Registry {
    #[must_use]
    pub fn new() -> Self {
        Self {
            map: Arc::new(Mutex::new(HashMap::new())),
            pubsub: broadcast::Sender::new(16),
        }
    }

    pub(crate) fn register(&self, entry: RegistryEntry) {
        let mut state = self.map.lock().unwrap();
        info!("Registering {:#?}", entry);
        let event = Event::UnitRegistered(entry.clone().into());
        if let Some(old) = state.insert(entry.name.clone(), entry) {
            info!("Replaced old entry {:#?}", old);
            self.send_event(Event::UnitShutdown(old.into()));
        }
        info!("Sending event {event:?}");
        self.send_event(event);
    }

    pub(crate) fn deregister(&self, name: &str) -> anyhow::Result<()> {
        let mut state = self.map.lock().unwrap();
        match state.remove(name) {
            Some(entry) => {
                let cascade: Vec<String> = state
                    .values()
                    .filter_map(|re| {
                        if re.agent_name() == Some(name) || re.vm_name() == Some(name) {
                            Some(re.name.clone())
                        } else {
                            None
                        }
                    })
                    .collect();
                for each in cascade {
                    if let Some(entry) = state.remove(&each) {
                        info!("Cascade deregistering {entry:#?}");
                        self.send_event(Event::UnitShutdown(entry.into()));
                    } else {
                        error!("Problems due cascade deregistering {each} (via {name})");
                    }
                }
                info!("Deregistering {:#?}", entry);
                self.send_event(Event::UnitShutdown(entry.into()));
                Ok(())
            }
            None => Err(anyhow!(
                "Can't deregister entry {}, it not registered",
                name
            )),
        }
    }

    pub(crate) fn by_name(&self, name: &str) -> anyhow::Result<RegistryEntry> {
        let state = self.map.lock().unwrap();
        state
            .get(name)
            .cloned()
            .ok_or_else(|| anyhow!("Service {name} not registered"))
    }

    pub(crate) fn find_names(&self, name: &str) -> anyhow::Result<Vec<String>> {
        let state = self.map.lock().unwrap();
        let list: Vec<String> = state
            .keys()
            .filter(|x| x.starts_with(name))
            .cloned()
            .collect();
        if list.is_empty() {
            bail!("No entries match string {}", name)
        }
        Ok(list)
    }

    pub(crate) fn find_map<T, F: FnMut(&RegistryEntry) -> Option<T>>(&self, filter: F) -> Vec<T> {
        let state = self.map.lock().unwrap();
        state.values().filter_map(filter).collect()
    }

    pub(crate) fn by_type_many(&self, ty: UnitType) -> Vec<RegistryEntry> {
        let state = self.map.lock().unwrap();
        state.values().filter(|x| x.r#type == ty).cloned().collect()
    }

    pub(crate) fn by_type(&self, ty: UnitType) -> anyhow::Result<RegistryEntry> {
        let vec = self.by_type_many(ty);
        match vec.len() {
            1 => Ok(vec.into_iter().next().unwrap()),
            0 => bail!("No service registered for"),
            _ => bail!("More than one unique services registered"), // FIXME: Fail registration, this situation should never happens
        }
    }

    #[allow(dead_code)]
    pub(crate) fn contains(&self, name: &str) -> bool {
        let state = self.map.lock().unwrap();
        state.contains_key(name)
    }

    pub(crate) fn create_unique_entry_name(&self, name: &str) -> String {
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

    pub(crate) fn watch_list(&self) -> Vec<RegistryEntry> {
        let state = self.map.lock().unwrap();
        state.values().filter(|x| x.watch).cloned().collect()
    }

    pub(crate) fn update_state(&self, name: &str, status: UnitStatus) -> anyhow::Result<()> {
        let mut state = self.map.lock().unwrap();
        state
            .get_mut(name)
            .map(|e| {
                e.status = status;
                self.send_event(Event::UnitStatusChanged(e.clone().into()));
            })
            .ok_or_else(|| anyhow!("Can't update state for {name}, is not registered"))
    }

    // FIXME: Should we dump full contents here for `query`/`query_list` high-level API
    // FIXME: .by_types_many() should works, but I add this one for debug convenience
    pub(crate) fn contents(&self) -> Vec<RegistryEntry> {
        let state = self.map.lock().unwrap();
        state.values().cloned().collect()
    }

    #[must_use]
    pub(crate) fn subscribe(&self) -> (Vec<QueryResult>, broadcast::Receiver<Event>) {
        let rx = self.pubsub.subscribe();
        let state = self.map.lock().unwrap();
        let contents = state.values().cloned().map(Into::into).collect();
        (contents, rx)
    }

    fn send_event(&self, event: Event) {
        if let Err(e) = self.pubsub.send(event) {
            debug!("error sending event: {e}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::admin::entry::Placement;
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
        assert!(r.contains("bar"));

        let foo1 = r.by_name(&foo_key)?;
        assert_eq!(foo1, foo);

        assert!(r.deregister(&foo_key).is_ok());
        assert!(!r.contains(&foo_key));
        assert!(r.by_name(&foo_key).is_err());
        assert!(r.deregister(&foo_key).is_err()); // fail to dereg second time
        Ok(())
    }

    #[test]
    fn test_cascade_deregister() -> anyhow::Result<()> {
        let r = Registry::new();
        let foo = RegistryEntry::dummy("foo".to_string());
        let bar = RegistryEntry {
            placement: Placement::Managed {
                by: "foo".into(),
                vm: "foo-vm".into(),
            },
            ..RegistryEntry::dummy("bar".to_string())
        };
        let baz = RegistryEntry {
            placement: Placement::Managed {
                by: "foo".into(),
                vm: "foo-vm".into(),
            },
            ..RegistryEntry::dummy("baz".to_string())
        };

        r.register(foo);
        r.register(bar);
        r.register(baz);
        assert!(r.contains("foo"));
        assert!(r.contains("bar"));
        assert!(r.contains("baz"));

        r.deregister("baz")?;
        assert!(r.contains("foo"));
        assert!(r.contains("bar"));
        assert!(!r.contains("baz"));

        r.deregister("foo")?;
        assert!(!r.contains("foo"));
        assert!(!r.contains("bar"));

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

// SPDX-FileCopyrightText: 2025-2026 TII (SSRC) and the Ghaf contributors
// SPDX-License-Identifier: Apache-2.0

// Types related to QueryList and Watch API
use super::types::{ServiceType, VmType};
use crate::pb;
use pb::admin::watch_item::Status;

use std::str::FromStr;
use std::string::ToString;

use anyhow::{Context, bail};
use serde::Serialize;
use strum::{Display, EnumString};

#[derive(Clone, Copy, Debug, Default, Serialize, EnumString, Display)]
#[cfg_attr(feature = "glib", derive(glib::Enum))]
#[cfg_attr(feature = "glib", enum_type(name = "GivcVMStatus"))]
#[repr(u8)]
pub enum VMStatus {
    #[default]
    Running = 0,
    PoweredOff = 1,
    Paused = 2,
}

#[derive(Clone, Copy, Debug, Default, Serialize, EnumString, Display)]
#[cfg_attr(feature = "glib", derive(glib::Enum))]
#[cfg_attr(feature = "glib", enum_type(name = "GivcTrustLevel"))]
#[repr(u8)]
pub enum TrustLevel {
    Secure = 0,
    #[default]
    Warning = 1,
    NotSecure = 2,
}

#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "glib", derive(glib::Boxed))]
#[cfg_attr(feature = "glib", boxed_type(name = "GivcQueryResult"))]
pub struct QueryResult {
    pub name: String,        //VM name
    pub description: String, //App name, some details
    pub status: VMStatus,
    pub trust_level: TrustLevel,
    pub vm_type: VmType,
    pub service_type: ServiceType,
    pub vm_name: Option<String>,
    pub agent_name: Option<String>,
}

impl QueryResult {
    /// # Errors
    /// Fails if unable to parse `pb::QueryListItem` into `QueryResult`
    pub fn parse_list(items: Vec<pb::QueryListItem>) -> anyhow::Result<Vec<QueryResult>> {
        items.into_iter().map(Self::try_from).collect()
    }
}

impl TryFrom<pb::QueryListItem> for QueryResult {
    type Error = anyhow::Error;
    fn try_from(item: pb::QueryListItem) -> Result<QueryResult, Self::Error> {
        Ok(QueryResult {
            name: item.name,
            description: item.description,
            status: VMStatus::from_str(item.vm_status.as_str())
                .with_context(|| format!("While parsing vm_status {}", item.vm_status))?,
            trust_level: TrustLevel::from_str(item.trust_level.as_str())
                .with_context(|| format!("While parsing trust_level {}", item.trust_level))?,
            vm_type: VmType::from_str(item.vm_type.as_str())
                .with_context(|| format!("While parsing vm_type {}", item.vm_type))?,
            service_type: ServiceType::from_str(item.service_type.as_str())
                .with_context(|| format!("While parsing service_type {}", item.service_type))?,
            agent_name: item.agent_name,
            vm_name: item.vm_name,
        })
    }
}

impl From<QueryResult> for pb::QueryListItem {
    fn from(val: QueryResult) -> Self {
        Self {
            name: val.name,
            description: val.description,
            vm_status: val.status.to_string(),
            trust_level: val.trust_level.to_string(),
            vm_type: val.vm_type.to_string(),
            service_type: val.service_type.to_string(),
            agent_name: val.agent_name,
            vm_name: val.vm_name,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub enum Event {
    UnitStatusChanged(QueryResult), // When unit updated/added
    UnitRegistered(QueryResult),
    UnitShutdown(QueryResult),
}

impl Event {
    pub fn into_initial(items: Vec<QueryResult>) -> pb::WatchItem {
        let values = items.into_iter().map(Into::into).collect();
        let init = pb::QueryListResponse { list: values };
        pb::WatchItem {
            status: Some(Status::Initial(init)),
        }
    }

    #[inline]
    pub(crate) fn watch_item(status: Status) -> pb::WatchItem {
        pb::WatchItem {
            status: Some(status),
        }
    }
}

impl TryFrom<pb::WatchItem> for Event {
    type Error = anyhow::Error;
    fn try_from(item: pb::WatchItem) -> Result<Self, Self::Error> {
        if let Some(status) = item.status {
            Ok(match status {
                Status::Initial(_) => bail!("Unexpected repeated Status::Initial"),
                Status::Added(value) => Event::UnitRegistered(QueryResult::try_from(value)?),
                Status::Updated(value) => Event::UnitStatusChanged(QueryResult::try_from(value)?),
                Status::Removed(value) => Event::UnitShutdown(QueryResult::try_from(value)?),
            })
        } else {
            bail!("WatchItem missing")
        }
    }
}

impl From<Event> for pb::WatchItem {
    fn from(val: Event) -> Self {
        match val {
            Event::UnitRegistered(value) => Event::watch_item(Status::Added(value.into())),
            Event::UnitStatusChanged(value) => Event::watch_item(Status::Updated(value.into())),
            Event::UnitShutdown(value) => Event::watch_item(Status::Removed(value.into())),
        }
    }
}

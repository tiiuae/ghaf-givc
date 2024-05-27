// This module contain literal translations of types from internal/pkgs/types/types.go
// Some of them would be rewritten, replaced, or even removed

#[derive(Debug, Clone, PartialEq)]
pub enum UnitType {
    HostMgr = 0,
    HostSvc = 1,
    HostApp = 2,

    AdmVm     = 3,
    AdmVmMgr = 4,
    AdmVmSvc = 5,
    AdmVmApp = 6,

    SysVm     = 7,
    SysVmMgr = 8,
    SysVmSvc = 9,
    SysVmApp = 10,

    AppVm     = 11,
    AppVmMgr = 12,
    AppVmSvc = 13,
    AppVmApp = 14,
}

/* FIXME: Eventually replace UnitType with following:
#[derive(Debug, Clone)]
enum UnitType {
    SysVM,
    AppVM,
    HostVM,
}

#[derive(Debug, Clone)]
enum UnitSubType {
    Mgr,
    Svc,
    App,
}
*/

#[derive(Debug, Clone)]
pub struct UnitStatus {
    pub name:   String,
    pub description: String,
    pub load_state:   String, 
    pub active_state: String,
    pub sub_state:    String,
    pub path:        String, // FIXME: PathBuf?
}

#[derive(Debug, Clone)]
pub struct TlsConfig {
}

#[derive(Debug, Clone)]
pub struct TransportConfig {
    pub address:   String,
    pub port:      u16,
    pub protocol:  String,
    pub tls_config: TlsConfig,
}

#[derive(Debug, Clone)]
pub struct EndpointConfig {
    pub name:      String,
    pub transport: TransportConfig,
    pub services:  Vec<String>
}

#[derive(Debug, Clone)]
pub struct EndpointEntry {
    pub name:     String,
    pub protocol: String,
    pub address:  String,
    pub port:     String,
    pub with_tls:  bool,
}

#[derive(Debug, Clone)]
pub struct RegistryEntry {
    pub name: String,
    pub parent: String,
    pub r#type: UnitType,
    pub status: UnitStatus,
    pub endpoint: EndpointEntry,
    pub watch: bool,
}

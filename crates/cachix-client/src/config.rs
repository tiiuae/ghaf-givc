use crate::client::CachixClient;

// With exception of `cache_name` all fields match `cachix.dhall` structure
pub struct CachixClientConfig {
    pub(crate) cache_name: String,
    pub(crate) hostname: String,
    pub(crate) auth_token: Option<String>,

    #[allow(dead_code)]
    pub(crate) binary_caches: Vec<(String, String)>,
}

impl CachixClientConfig {
    #[must_use]
    pub fn new(cache_name: String) -> CachixClientConfig {
        CachixClientConfig {
            cache_name,
            hostname: "https://cachix.org".to_string(),
            binary_caches: Vec::new(),
            auth_token: None,
        }
    }

    #[must_use]
    pub fn set_hostname(self, hostname: String) -> CachixClientConfig {
        Self { hostname, ..self }
    }

    #[must_use]
    pub fn set_auth_token(self, token: String) -> CachixClientConfig {
        Self {
            auth_token: Some(token),
            ..self
        }
    }

    #[must_use]
    pub fn build(self) -> CachixClient {
        CachixClient::new(self)
    }
}

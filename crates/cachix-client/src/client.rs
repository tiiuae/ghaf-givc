use crate::{CacheInfo, CachixError, PinList};
use reqwest::{Client, StatusCode};
use std::sync::Arc;

#[derive(Clone)]
pub struct CachixClient {
    cache_name: String,
    token: Option<String>,
    client: Arc<Client>,
}

/// API implemented for <https://app.cachix.org/api/v1/>
impl CachixClient {
    /// Create new Cachix client
    #[must_use]
    pub fn new(cache_name: String, token: Option<String>) -> Self {
        Self {
            cache_name,
            token,
            client: Arc::new(Client::new()),
        }
    }

    fn api_url(&self, path: &[&str]) -> String {
        format!(
            "https://app.cachix.org/api/v1/cache/{}/{}",
            self.cache_name,
            path.join("/")
        )
    }

    fn auth(&self, req: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        if let Some(token) = &self.token {
            req.header("Authorization", format!("Bearer {token}"))
        } else {
            req
        }
    }

    fn get(&self, path: &[&str]) -> reqwest::RequestBuilder {
        let url = self.api_url(path);
        self.auth(self.client.get(url))
    }

    fn delete(&self, path: &[&str]) -> reqwest::RequestBuilder {
        let url = self.api_url(path);
        self.auth(self.client.delete(url))
    }

    /// Info about cache
    /// # Errors
    /// Fails if cachix return an error
    pub async fn cache_info(&self) -> Result<CacheInfo, CachixError> {
        let res = self
            .get(&[])
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        Ok(res)
    }

    /// Enumerate existing pins in cache
    /// # Errors
    /// Fails if cachix return an error
    pub async fn list_pins(&self) -> Result<PinList, CachixError> {
        let res = self
            .get(&["pin"])
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        Ok(res)
    }

    /// Delete pin
    ///   (require owner permissions)
    /// # Errors
    /// Fails if cachix return an error
    pub async fn delete_pin(&self, name: &str) -> Result<(), CachixError> {
        let res = self.delete(&["pin", name]).send().await?;
        match res.status() {
            StatusCode::NO_CONTENT => Ok(()),
            StatusCode::UNAUTHORIZED => Err(CachixError::Unauthorized),
            s => Err(CachixError::UnexpectedStatus(s)),
        }
    }

    /// Retrieve single file from store
    /// # Errors
    /// Fails if cachix return an error
    pub async fn get_file_from_store(
        &self,
        nar_hash: &str,
        path: &str,
    ) -> Result<Vec<u8>, CachixError> {
        let res = self
            .get(&["serve", nar_hash, path.trim_start_matches('/')])
            .send()
            .await?
            .error_for_status()?
            .bytes()
            .await?;
        Ok(res.to_vec())
    }
}

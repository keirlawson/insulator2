use async_trait::async_trait;
use log::error;
use serde::de::DeserializeOwned;

use super::error::Result;
use super::BasicAuth;

#[async_trait]
pub trait HttpClient {
    async fn get<T: 'static + DeserializeOwned>(&self, url: &str) -> Result<T>;
    async fn delete(&self, url: &str) -> Result<()>;
}

pub struct ReqwestClient {
    client: reqwest::Client,
    timeout_seconds: u64,
    auth: Option<BasicAuth>,
}

#[async_trait]
impl HttpClient for ReqwestClient {
    async fn get<T: 'static + DeserializeOwned>(&self, url: &str) -> Result<T> {
        let mut request = self.client.get(url.to_string());
        request = request.timeout(core::time::Duration::from_secs(self.timeout_seconds));
        if let Some(auth) = &self.auth {
            request = request.basic_auth(auth.username.to_owned(), auth.password.to_owned());
        }
        let response = request.send().await?;
        let res = response.json().await?;
        Ok(res)
    }

    async fn delete(&self, url: &str) -> Result<()> {
        let mut request = self.client.delete(url.to_string());
        request = request.timeout(core::time::Duration::from_secs(self.timeout_seconds));
        if let Some(auth) = &self.auth {
            request = request.basic_auth(auth.username.to_owned(), auth.password.to_owned());
        }
        let response = request.send().await?;
        if response.status().is_success() {
            Ok(())
        } else {
            error!("{:?}", response.status());
            Err(crate::lib::schema_registry::SchemaRegistryError::HttpClient {
                message: "Error calling the delete".into(),
            })
        }
    }
}

impl ReqwestClient {
    pub fn new(auth: Option<BasicAuth>) -> Self {
        Self {
            client: Default::default(),
            timeout_seconds: 10,
            auth,
        }
    }
}

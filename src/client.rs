use crate::{
    CreateSandboxRequest, CreateSandboxResponse, DisksService, Error, Health, HostPublic, Instance,
    ListSandboxesOptions, NetworksService, Readiness, RequestOptions, Result, RetryOptions,
    RootFsData, Sandbox, Shape, TemplatesService, WhoAmI,
    transport::{DEFAULT_BASE_URL, Transport},
};
use reqwest::{Method, StatusCode};
use serde::de::DeserializeOwned;
use serde_json::Value;
use std::{env, sync::Arc, time::Duration};
use url::Url;

/// Authenticated entry point to the `CreateOS` Sandbox API.
#[derive(Clone)]
pub struct Client {
    pub(crate) transport: Arc<Transport>,
}

/// Builder for [`Client`].
#[derive(Default)]
#[must_use = "a client builder does nothing until build() is called"]
pub struct ClientBuilder {
    api_key: Option<String>,
    base_url: Option<String>,
    timeout: Option<Duration>,
    retry: Option<RetryOptions>,
    user_agent: Option<String>,
    http_client: Option<reqwest::ClientBuilder>,
}

impl ClientBuilder {
    /// Sets the API key.
    pub fn api_key(mut self, value: impl Into<String>) -> Self {
        self.api_key = Some(value.into());
        self
    }
    /// Overrides the control-plane URL.
    pub fn base_url(mut self, value: impl Into<String>) -> Self {
        self.base_url = Some(value.into());
        self
    }
    /// Sets the default complete-request timeout.
    pub fn timeout(mut self, value: Duration) -> Self {
        self.timeout = Some(value);
        self
    }
    /// Sets the retry policy.
    pub fn retry(mut self, value: RetryOptions) -> Self {
        self.retry = Some(value);
        self
    }
    /// Disables automatic retries.
    pub fn without_retry(mut self) -> Self {
        self.retry = Some(RetryOptions {
            max_retries: 0,
            ..RetryOptions::default()
        });
        self
    }
    /// Overrides the user agent.
    pub fn user_agent(mut self, value: impl Into<String>) -> Self {
        self.user_agent = Some(value.into());
        self
    }
    /// Uses a preconfigured HTTP client builder.
    ///
    /// The SDK applies its user agent and disables redirects when [`Self::build`]
    /// is called so that the API key cannot be forwarded to another origin.
    pub fn http_client(mut self, value: reqwest::ClientBuilder) -> Self {
        self.http_client = Some(value);
        self
    }

    /// Validates configuration and constructs the client.
    pub fn build(self) -> Result<Client> {
        let api_key = self
            .api_key
            .or_else(|| env::var("CREATEOS_API_KEY").ok())
            .map(|key| key.trim().to_owned())
            .filter(|key| !key.is_empty());
        let raw_url = self
            .base_url
            .or_else(|| env::var("CREATEOS_SANDBOX_BASE_URL").ok())
            .unwrap_or_else(|| DEFAULT_BASE_URL.to_owned());
        let mut base_url = Url::parse(raw_url.trim())
            .map_err(|error| Error::Configuration(format!("invalid base URL: {error}")))?;
        if !matches!(base_url.scheme(), "http" | "https") || base_url.host_str().is_none() {
            return Err(Error::Configuration(
                "base URL must be an HTTP(S) URL with a host".into(),
            ));
        }
        if base_url.username() != ""
            || base_url.password().is_some()
            || base_url.query().is_some()
            || base_url.fragment().is_some()
        {
            return Err(Error::Configuration(
                "base URL must not contain credentials, a query, or a fragment".into(),
            ));
        }
        let normalized = format!("{}/", base_url.as_str().trim_end_matches('/'));
        base_url =
            Url::parse(&normalized).map_err(|error| Error::Configuration(error.to_string()))?;
        if self.timeout.is_some_and(|timeout| timeout.is_zero()) {
            return Err(Error::Configuration("timeout must be positive".into()));
        }
        let retry = self.retry.unwrap_or_default();
        let user_agent = self
            .user_agent
            .unwrap_or_else(|| format!("createos-rust-sdk/{}", env!("CARGO_PKG_VERSION")));
        if user_agent.trim().is_empty() {
            return Err(Error::Configuration("user agent must not be empty".into()));
        }
        Ok(Client {
            transport: Transport::new(
                base_url,
                api_key,
                self.http_client,
                self.timeout,
                retry,
                &user_agent,
            )?,
        })
    }
}

impl Client {
    /// Starts a client builder. Environment variables provide optional defaults.
    pub fn builder() -> ClientBuilder {
        ClientBuilder::default()
    }
    /// Constructs a client from environment variables and defaults.
    pub fn from_env() -> Result<Self> {
        Self::builder().build()
    }
    /// Returns the configured control-plane URL.
    pub fn base_url(&self) -> &Url {
        &self.transport.base_url
    }
    /// Returns account-level template operations.
    pub fn templates(&self) -> TemplatesService {
        TemplatesService::new(self.transport.clone())
    }
    /// Returns account-level overlay-network operations.
    pub fn networks(&self) -> NetworksService {
        NetworksService::new(self.transport.clone())
    }
    /// Returns account-level persistent-disk operations.
    pub fn disks(&self) -> DisksService {
        DisksService::new(self.transport.clone())
    }

    /// Returns unauthenticated control-plane liveness.
    pub async fn health(&self) -> Result<Health> {
        self.transport
            .get("/healthz", &[], &RequestOptions::default(), false)
            .await
    }

    /// Returns unauthenticated readiness. HTTP 503 is represented as `ready: false`.
    pub async fn readiness(&self) -> Result<Readiness> {
        let options = RequestOptions {
            disable_retry: true,
            ..RequestOptions::default()
        };
        let response = self
            .transport
            .raw(Method::GET, "/readyz", &[], &options, false)
            .await?;
        let status = response.status();
        if !matches!(status, StatusCode::OK | StatusCode::SERVICE_UNAVAILABLE) {
            return Err(
                crate::ApiError::from_response(&Method::GET, "/readyz", response)
                    .await
                    .into(),
            );
        }
        let fallback = Readiness {
            ready: status == StatusCode::OK,
            ..Readiness::default()
        };
        let body = response.bytes().await?;
        let value: Value = match serde_json::from_slice(&body) {
            Ok(value) => value,
            Err(_) => return Ok(fallback),
        };
        Ok(
            serde_json::from_value(value.get("data").cloned().unwrap_or(Value::Null))
                .unwrap_or(fallback),
        )
    }

    /// Returns the identity associated with this API key.
    pub async fn who_am_i(&self) -> Result<WhoAmI> {
        self.transport
            .get("/v1/whoami", &[], &RequestOptions::default(), true)
            .await
    }

    /// Creates a running sandbox.
    pub async fn create_sandbox(&self, request: CreateSandboxRequest) -> Result<Instance> {
        self.create_sandbox_with(request, &RequestOptions::default())
            .await
    }

    /// Creates a sandbox with per-request transport options.
    pub async fn create_sandbox_with(
        &self,
        request: CreateSandboxRequest,
        options: &RequestOptions,
    ) -> Result<Instance> {
        let created: CreateSandboxResponse = self
            .transport
            .send(Method::POST, "/v1/sandboxes", &[], &request, options)
            .await?;
        let sandbox = Sandbox {
            id: created.id,
            status: created.status,
            name: created.name,
            ip_address: Some(created.ip_address),
            shape: created.shape,
            rootfs: created.rootfs,
            vcpu: created.vcpu,
            memory_mib: created.memory_mib,
            disk_mib: created.disk_mib,
            spawn_milliseconds: created.spawn_milliseconds,
            ingress_enabled: request.ingress_enabled,
            ingress_url_template: created.ingress_url_template,
            egress_rules: created.egress_rules,
            ..Sandbox::default()
        };
        Ok(Instance::new(self.transport.clone(), sandbox))
    }

    /// Gets a sandbox by ID.
    pub async fn sandbox(&self, id: &str) -> Result<Instance> {
        self.sandbox_at(&format!("/v1/sandboxes/{}", encode(id)))
            .await
    }
    /// Gets a sandbox by private IP address.
    pub async fn sandbox_by_ip(&self, ip: &str) -> Result<Instance> {
        self.sandbox_at(&format!("/v1/sandboxes/by-ip/{}", encode(ip)))
            .await
    }
    async fn sandbox_at(&self, path: &str) -> Result<Instance> {
        let data = self
            .transport
            .get(path, &[], &RequestOptions::default(), true)
            .await?;
        Ok(Instance::new(self.transport.clone(), data))
    }

    /// Lists sandboxes, automatically walking pages unless limited.
    pub async fn sandboxes(&self, options: ListSandboxesOptions) -> Result<Vec<Instance>> {
        let mut query = Vec::new();
        if let Some(status) = options.status {
            query.push(("status".into(), status.to_string()));
        }
        Ok(fetch_all::<Sandbox>(
            &self.transport,
            "/v1/sandboxes",
            query,
            options.limit,
            None,
            &options.request,
        )
        .await?
        .into_iter()
        .map(|data| Instance::new(self.transport.clone(), data))
        .collect())
    }

    /// Lists public sandbox sizing presets.
    pub async fn shapes(&self) -> Result<Vec<Shape>> {
        fetch_all_with_auth(
            &self.transport,
            "/v1/shapes",
            vec![],
            None,
            Some("shapes"),
            &RequestOptions::default(),
            false,
        )
        .await
    }
    /// Returns the built-in root filesystem catalog.
    pub async fn root_file_systems(&self) -> Result<RootFsData> {
        self.transport
            .get("/v1/rootfs", &[], &RequestOptions::default(), false)
            .await
    }
    /// Lists worker hosts. This endpoint requires administrator credentials.
    pub async fn hosts(&self) -> Result<Vec<HostPublic>> {
        fetch_all(
            &self.transport,
            "/v1/hosts",
            vec![],
            None,
            None,
            &RequestOptions::default(),
        )
        .await
    }
}

pub(crate) async fn fetch_all<T: DeserializeOwned>(
    transport: &Transport,
    path: &str,
    query: Vec<(String, String)>,
    limit: Option<usize>,
    legacy_key: Option<&str>,
    options: &RequestOptions,
) -> Result<Vec<T>> {
    fetch_all_with_auth(transport, path, query, limit, legacy_key, options, true).await
}

async fn fetch_all_with_auth<T: DeserializeOwned>(
    transport: &Transport,
    path: &str,
    mut query: Vec<(String, String)>,
    limit: Option<usize>,
    legacy_key: Option<&str>,
    options: &RequestOptions,
    authenticated: bool,
) -> Result<Vec<T>> {
    const PAGE_SIZE: usize = 500;
    let mut result = Vec::new();
    let mut offset = query
        .iter()
        .find(|(key, _)| key == "offset")
        .and_then(|(_, value)| value.parse().ok())
        .unwrap_or(0);
    loop {
        let size = limit.map_or(PAGE_SIZE, |limit| {
            (limit.saturating_sub(result.len())).min(PAGE_SIZE)
        });
        if size == 0 {
            break;
        }
        query.retain(|(key, _)| key != "limit" && key != "offset");
        query.push(("limit".into(), size.to_string()));
        query.push(("offset".into(), offset.to_string()));
        let value: Value = transport.get(path, &query, options, authenticated).await?;
        let (items, total) = decode_page(value, legacy_key)?;
        let count = items.len();
        result.extend(items);
        if count == 0 || total.is_none_or(|total| offset + count >= total) {
            break;
        }
        offset += count;
    }
    if let Some(limit) = limit {
        result.truncate(limit);
    }
    Ok(result)
}

fn decode_page<T: DeserializeOwned>(
    value: Value,
    legacy_key: Option<&str>,
) -> Result<(Vec<T>, Option<usize>)> {
    if value.is_array() {
        return Ok((serde_json::from_value(value)?, None));
    }
    let total = value
        .get("pagination")
        .and_then(|value| value.get("total"))
        .and_then(Value::as_u64)
        .and_then(|n| usize::try_from(n).ok());
    let items = value
        .get("data")
        .cloned()
        .or_else(|| legacy_key.and_then(|key| value.get(key).cloned()))
        .unwrap_or_else(|| Value::Array(vec![]));
    Ok((serde_json::from_value(items)?, total))
}

pub(crate) fn encode(value: &str) -> String {
    percent_encoding::utf8_percent_encode(value, percent_encoding::NON_ALPHANUMERIC).to_string()
}

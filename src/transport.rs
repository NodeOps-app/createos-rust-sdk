use crate::{Error, RequestOptions, Result, RetryOptions};
use rand::Rng as _;
use reqwest::{Method, Response, StatusCode, header};
use serde::{Serialize, de::DeserializeOwned};
use serde_json::Value;
use std::{sync::Arc, time::Duration};
use url::Url;

pub(crate) const DEFAULT_BASE_URL: &str = "https://api.sb.createos.sh";
const DEFAULT_TIMEOUT: Duration = Duration::from_secs(60);

#[derive(Clone)]
pub(crate) struct Transport {
    pub base_url: Url,
    client: reqwest::Client,
    api_key: Option<String>,
    timeout: Duration,
    retry: RetryOptions,
}

impl Transport {
    pub fn new(
        base_url: Url,
        api_key: Option<String>,
        client: Option<reqwest::ClientBuilder>,
        timeout: Option<Duration>,
        retry: RetryOptions,
        user_agent: &str,
    ) -> Result<Arc<Self>> {
        let client = client
            .unwrap_or_default()
            .user_agent(user_agent)
            .redirect(reqwest::redirect::Policy::none())
            .build()?;
        Ok(Arc::new(Self {
            base_url,
            client,
            api_key,
            timeout: timeout.unwrap_or(DEFAULT_TIMEOUT),
            retry,
        }))
    }

    pub async fn get<T: DeserializeOwned>(
        &self,
        path: &str,
        query: &[(String, String)],
        options: &RequestOptions,
        authenticated: bool,
    ) -> Result<T> {
        self.json(
            Method::GET,
            path,
            query,
            Option::<&()>::None,
            options,
            authenticated,
        )
        .await
    }

    pub async fn send<B: Serialize + ?Sized, T: DeserializeOwned>(
        &self,
        method: Method,
        path: &str,
        query: &[(String, String)],
        body: &B,
        options: &RequestOptions,
    ) -> Result<T> {
        self.json(method, path, query, Some(body), options, true)
            .await
    }

    pub async fn empty<T: DeserializeOwned>(
        &self,
        method: Method,
        path: &str,
        query: &[(String, String)],
        options: &RequestOptions,
    ) -> Result<T> {
        self.json(method, path, query, Option::<&()>::None, options, true)
            .await
    }

    async fn json<B: Serialize + ?Sized, T: DeserializeOwned>(
        &self,
        method: Method,
        path: &str,
        query: &[(String, String)],
        body: Option<&B>,
        options: &RequestOptions,
        authenticated: bool,
    ) -> Result<T> {
        let bytes = body.map(serde_json::to_vec).transpose()?;
        let response = self
            .execute(
                method.clone(),
                path,
                query,
                bytes,
                options,
                authenticated,
                true,
            )
            .await?;
        if !response.status().is_success() {
            return Err(crate::ApiError::from_response(&method, path, response)
                .await
                .into());
        }
        let status = response.status();
        let body = response.bytes().await?;
        if body.iter().all(u8::is_ascii_whitespace)
            && matches!(status, StatusCode::NO_CONTENT | StatusCode::RESET_CONTENT)
        {
            return serde_json::from_value(Value::Null).map_err(Error::from);
        }
        let envelope: Envelope<Value> = serde_json::from_slice(&body).map_err(|error| {
            Error::Protocol(format!(
                "decode {} {path} JSend envelope: {error}",
                method.as_str()
            ))
        })?;
        if envelope.status != "success" {
            return Err(Error::Protocol(envelope.message.unwrap_or_else(|| {
                format!("unexpected JSend status {:?}", envelope.status)
            })));
        }
        serde_json::from_value(envelope.data.unwrap_or(Value::Null)).map_err(Error::from)
    }

    pub async fn raw(
        &self,
        method: Method,
        path: &str,
        query: &[(String, String)],
        options: &RequestOptions,
        authenticated: bool,
    ) -> Result<Response> {
        self.execute(method, path, query, None, options, authenticated, true)
            .await
    }

    pub async fn stream_json<B: Serialize + ?Sized>(
        &self,
        method: Method,
        path: &str,
        query: &[(String, String)],
        body: Option<&B>,
        options: &RequestOptions,
    ) -> Result<Response> {
        let bytes = body.map(serde_json::to_vec).transpose()?;
        let mut no_retry = options.clone();
        no_retry.disable_retry = true;
        let response = self
            .execute(method.clone(), path, query, bytes, &no_retry, true, true)
            .await?;
        if response.status().is_success() {
            Ok(response)
        } else {
            Err(crate::ApiError::from_response(&method, path, response)
                .await
                .into())
        }
    }

    pub async fn upload(
        &self,
        method: Method,
        path: &str,
        query: &[(String, String)],
        body: reqwest::Body,
        options: &RequestOptions,
    ) -> Result<Response> {
        let url = self.url(path, query)?;
        let mut request = self
            .client
            .request(method, url)
            .headers(sanitized_headers(&options.headers))
            .header(header::ACCEPT, "application/json")
            .header(header::CONTENT_TYPE, "application/octet-stream")
            .body(body);
        request = self.authenticate(request, true)?;
        let timeout = options.timeout.unwrap_or(self.timeout);
        Ok(request.timeout(timeout).send().await?)
    }

    async fn execute(
        &self,
        method: Method,
        path: &str,
        query: &[(String, String)],
        body: Option<Vec<u8>>,
        options: &RequestOptions,
        authenticated: bool,
        json: bool,
    ) -> Result<Response> {
        let retry = options.retry.as_ref().unwrap_or(&self.retry);
        validate_retry(retry)?;
        let max_retries = if options.disable_retry {
            0
        } else {
            retry.max_retries
        };
        let url = self.url(path, query)?;
        for attempt in 0..=max_retries {
            let mut request = self
                .client
                .request(method.clone(), url.clone())
                .headers(sanitized_headers(&options.headers))
                .header(header::ACCEPT, "application/json");
            if let Some(body) = &body {
                request = request.body(body.clone());
                if json {
                    request = request.header(header::CONTENT_TYPE, "application/json");
                }
            }
            request = self.authenticate(request, authenticated)?;
            let result = request
                .timeout(options.timeout.unwrap_or(self.timeout))
                .send()
                .await;
            match result {
                Ok(response) => {
                    if attempt == max_retries || !retryable_status(&method, response.status()) {
                        return Ok(response);
                    }
                    let delay =
                        retry_after(response.headers()).unwrap_or_else(|| backoff(attempt, retry));
                    drop(response);
                    tokio::time::sleep(delay).await;
                }
                Err(error) => {
                    if attempt == max_retries || !idempotent(&method) || error.is_timeout() {
                        return Err(error.into());
                    }
                    tokio::time::sleep(backoff(attempt, retry)).await;
                }
            }
        }
        unreachable!("retry loop always returns")
    }

    fn authenticate(
        &self,
        mut request: reqwest::RequestBuilder,
        required: bool,
    ) -> Result<reqwest::RequestBuilder> {
        if !required {
            return Ok(request);
        }
        let key = self.api_key.as_deref().ok_or_else(|| {
            Error::Configuration("authentication is required: configure an API key".into())
        })?;
        request = request.header("x-api-key", key);
        Ok(request)
    }

    fn url(&self, path: &str, query: &[(String, String)]) -> Result<Url> {
        let mut url = self
            .base_url
            .join(path.trim_start_matches('/'))
            .map_err(|error| Error::Configuration(format!("invalid endpoint URL: {error}")))?;
        if !query.is_empty() {
            url.query_pairs_mut().extend_pairs(query);
        }
        Ok(url)
    }
}

#[derive(serde::Deserialize)]
struct Envelope<T> {
    status: String,
    data: Option<T>,
    message: Option<String>,
}

fn validate_retry(retry: &RetryOptions) -> Result<()> {
    if retry.base_delay.is_zero() || retry.max_delay < retry.base_delay {
        return Err(Error::Configuration("retry delays are invalid".into()));
    }
    Ok(())
}

fn idempotent(method: &Method) -> bool {
    matches!(
        *method,
        Method::GET | Method::HEAD | Method::PUT | Method::DELETE
    )
}

fn retryable_status(method: &Method, status: StatusCode) -> bool {
    matches!(
        status,
        StatusCode::TOO_MANY_REQUESTS | StatusCode::SERVICE_UNAVAILABLE
    ) || (idempotent(method)
        && matches!(
            status,
            StatusCode::REQUEST_TIMEOUT
                | StatusCode::INTERNAL_SERVER_ERROR
                | StatusCode::BAD_GATEWAY
                | StatusCode::GATEWAY_TIMEOUT
        ))
}

fn backoff(attempt: u32, retry: &RetryOptions) -> Duration {
    let factor = 1_u32.checked_shl(attempt.min(30)).unwrap_or(u32::MAX);
    let exponential = retry.base_delay.saturating_mul(factor);
    let jitter_max = u64::try_from(retry.base_delay.as_millis()).unwrap_or(u64::MAX);
    let jitter = Duration::from_millis(rand::rng().random_range(0..jitter_max.max(1)));
    exponential.saturating_add(jitter).min(retry.max_delay)
}

fn retry_after(headers: &header::HeaderMap) -> Option<Duration> {
    let value = headers.get(header::RETRY_AFTER)?.to_str().ok()?.trim();
    if let Ok(seconds) = value.parse::<u64>() {
        return Some(Duration::from_secs(seconds));
    }
    let when = chrono::DateTime::parse_from_rfc2822(value).ok()?;
    let remaining = when.signed_duration_since(chrono::Utc::now());
    Some(remaining.to_std().unwrap_or_default())
}

fn sanitized_headers(headers: &header::HeaderMap) -> header::HeaderMap {
    let mut headers = headers.clone();
    for name in [
        header::AUTHORIZATION,
        header::PROXY_AUTHORIZATION,
        header::COOKIE,
        header::SET_COOKIE,
        header::HeaderName::from_static("x-api-key"),
        header::HeaderName::from_static("x-auth-token"),
        header::HeaderName::from_static("x-csrf-token"),
    ] {
        headers.remove(name);
    }
    headers
}

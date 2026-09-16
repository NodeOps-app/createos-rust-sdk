use crate::RunCommandResponse;
use reqwest::{StatusCode, header::HeaderMap};
use serde_json::Value;
use std::{fmt, time::Duration};

const MAX_ERROR_BODY: usize = 4 * 1024 * 1024;

/// A crate-wide result.
pub type Result<T> = std::result::Result<T, Error>;

/// A non-successful response from the `CreateOS` API.
#[derive(Debug)]
pub struct ApiError {
    /// HTTP response status.
    pub status: StatusCode,
    /// Stable API error code, when supplied.
    pub code: Option<i64>,
    /// Server request identifier, when supplied.
    pub request_id: Option<String>,
    /// Request method.
    pub method: String,
    /// API endpoint path.
    pub endpoint: String,
    /// Response headers.
    pub headers: HeaderMap,
    /// Raw response body.
    pub body: bytes::Bytes,
    /// Human-readable API message.
    pub message: String,
}

impl fmt::Display for ApiError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} {}: HTTP {}: {}",
            self.method,
            self.endpoint,
            self.status.as_u16(),
            self.message
        )
    }
}

impl std::error::Error for ApiError {}

/// A shell command that completed with a nonzero exit code or agent error.
#[derive(Debug)]
pub struct CommandError {
    /// Complete command response, including stdout, stderr, and exit status.
    pub response: RunCommandResponse,
}

impl fmt::Display for CommandError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let result = &self.response.result;
        write!(f, "command exited with status {}", result.exit_code)?;
        if !result.error_message.is_empty() {
            write!(f, "\nerror: {}", result.error_message)?;
        }
        if !result.standard_error.is_empty() {
            write!(f, "\nstderr: {}", tail(&result.standard_error, 2000))?;
        }
        Ok(())
    }
}

impl std::error::Error for CommandError {}

impl ApiError {
    pub(crate) async fn from_response(
        method: &reqwest::Method,
        endpoint: &str,
        response: reqwest::Response,
    ) -> Self {
        let status = response.status();
        let headers = response.headers().clone();
        let request_id = headers
            .get("x-request-id")
            .and_then(|value| value.to_str().ok())
            .map(str::to_owned);
        let mut stream = response.bytes_stream();
        let mut body = bytes::BytesMut::new();
        while body.len() < MAX_ERROR_BODY {
            use futures_util::StreamExt as _;
            let Some(chunk) = stream.next().await else {
                break;
            };
            let Ok(chunk) = chunk else { break };
            let remaining = MAX_ERROR_BODY - body.len();
            body.extend_from_slice(&chunk[..chunk.len().min(remaining)]);
        }
        let body = body.freeze();
        let envelope: Option<Value> = serde_json::from_slice(&body).ok();
        let code = envelope
            .as_ref()
            .and_then(|value| value.get("code"))
            .and_then(Value::as_i64);
        let message = envelope
            .as_ref()
            .and_then(|value| value.get("message"))
            .and_then(Value::as_str)
            .map(str::to_owned)
            .or_else(|| fail_data_message(envelope.as_ref()?.get("data")?))
            .unwrap_or_else(|| {
                status
                    .canonical_reason()
                    .unwrap_or("request failed")
                    .to_owned()
            });
        Self {
            status,
            code,
            request_id,
            method: method.as_str().to_owned(),
            endpoint: endpoint.to_owned(),
            headers,
            body,
            message,
        }
    }
}

fn fail_data_message(value: &Value) -> Option<String> {
    if let Some(message) = value.as_str() {
        return Some(message.to_owned());
    }
    let fields = value.as_object()?;
    let mut fields = fields
        .iter()
        .filter_map(|(key, value)| Some((key, value.as_str()?)))
        .collect::<Vec<_>>();
    fields.sort_unstable_by_key(|(key, _)| *key);
    (!fields.is_empty()).then(|| {
        fields
            .into_iter()
            .map(|(key, value)| format!("{key}: {value}"))
            .collect::<Vec<_>>()
            .join("; ")
    })
}

/// Errors returned by this SDK.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// Client configuration is invalid.
    #[error("invalid configuration: {0}")]
    Configuration(String),
    /// A request argument is invalid.
    #[error("invalid argument: {0}")]
    InvalidArgument(String),
    /// The HTTP client failed before receiving a valid API response.
    #[error("HTTP transport error: {0}")]
    Transport(#[from] reqwest::Error),
    /// The API returned a non-successful response.
    #[error(transparent)]
    Api(#[from] ApiError),
    /// A shell command completed unsuccessfully.
    #[error(transparent)]
    Command(#[from] CommandError),
    /// The API response did not match the protocol.
    #[error("protocol error: {0}")]
    Protocol(String),
    /// JSON encoding or decoding failed.
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
    /// Base64 process output was invalid.
    #[error("base64 error: {0}")]
    Base64(#[from] base64::DecodeError),
    /// Local I/O failed.
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    /// An SDK wait operation exceeded its budget.
    #[error("operation timed out after {0:?}")]
    Timeout(Duration),
}

fn tail(value: &str, maximum: usize) -> &str {
    if value.len() <= maximum {
        return value;
    }
    let mut start = value.len() - maximum;
    while !value.is_char_boundary(start) {
        start += 1;
    }
    &value[start..]
}

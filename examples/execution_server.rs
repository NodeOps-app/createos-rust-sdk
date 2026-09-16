use axum::{
    Json, Router,
    extract::{DefaultBodyLimit, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::post,
};
use createos::{Client, CreateSandboxRequest, ExecOptions, RunCommandRequest};
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, sync::Arc, time::Duration};
use tokio::sync::Semaphore;

#[derive(Clone)]
struct AppState {
    client: Client,
    slots: Arc<Semaphore>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct ExecuteRequest {
    command: String,
    #[serde(default)]
    arguments: Vec<String>,
    #[serde(default)]
    standard_input: Option<String>,
    #[serde(default)]
    environment_variables: HashMap<String, String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ExecuteResponse {
    stdout: String,
    stderr: String,
    exit_code: i32,
    #[serde(skip_serializing_if = "String::is_empty")]
    error: String,
    execution_milliseconds: f64,
}

async fn execute(State(state): State<AppState>, Json(input): Json<ExecuteRequest>) -> Response {
    if input.command.trim().is_empty() {
        return (StatusCode::BAD_REQUEST, "command is required").into_response();
    }
    let Ok(_permit) = state.slots.clone().try_acquire_owned() else {
        return (StatusCode::TOO_MANY_REQUESTS, "execution capacity reached").into_response();
    };
    let operation = tokio::time::timeout(Duration::from_secs(120), async {
        let sandbox = state
            .client
            .create_sandbox(CreateSandboxRequest {
                shape: "s-1vcpu-1gb".into(),
                rootfs: Some("devbox:1".into()),
                ..Default::default()
            })
            .await?;
        let result = sandbox
            .run_command(
                RunCommandRequest {
                    command: input.command,
                    arguments: input.arguments,
                    standard_input: input.standard_input,
                    environment_variables: input.environment_variables,
                    ..Default::default()
                },
                ExecOptions::default(),
            )
            .await;
        let cleanup = sandbox.destroy().await;
        let result = result?;
        cleanup?;
        Ok::<_, createos::Error>(result)
    })
    .await;
    match operation {
        Ok(Ok(result)) => Json(ExecuteResponse {
            stdout: result.result.standard_output,
            stderr: result.result.standard_error,
            exit_code: result.result.exit_code,
            error: result.result.error_message,
            execution_milliseconds: result.execution_milliseconds,
        })
        .into_response(),
        Ok(Err(error)) => (
            StatusCode::BAD_GATEWAY,
            format!("execution failed: {error}"),
        )
            .into_response(),
        Err(_) => (StatusCode::GATEWAY_TIMEOUT, "execution timed out").into_response(),
    }
}

#[tokio::main]
async fn main() -> createos::Result<()> {
    let address =
        std::env::var("EXECUTION_SERVER_ADDRESS").unwrap_or_else(|_| "127.0.0.1:8080".into());
    let state = AppState {
        client: Client::from_env()?,
        slots: Arc::new(Semaphore::new(4)),
    };
    let app = Router::new()
        .route("/v1/execute", post(execute))
        .layer(DefaultBodyLimit::max(1024 * 1024))
        .with_state(state);
    let listener = tokio::net::TcpListener::bind(&address).await?;
    println!("execution server listening on {address}");
    axum::serve(listener, app)
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await
        .map_err(|error| createos::Error::Protocol(format!("server failed: {error}")))
}

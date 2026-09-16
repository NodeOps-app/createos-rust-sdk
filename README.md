# CreateOS Rust SDK

Launch an isolated cloud sandbox, run commands, stream output, move files,
publish a preview URL, and tear everything down from async Rust.

## Your first sandbox

```toml
[dependencies]
createos = "0.1"
tokio = { version = "1", features = ["macros", "rt-multi-thread"] }
```

The crates.io dependency above applies after the first `0.1` release. Until
then, clone this repository and use a local path dependency while developing.

```rust,no_run
use createos::{Client, CreateSandboxRequest, RunCommandRequest};

#[tokio::main]
async fn main() -> createos::Result<()> {
    let client = Client::builder()
        .api_key("your-api-key")
        .build()?;

    let sandbox = client
        .create_sandbox(CreateSandboxRequest {
            name: Some("hello-rust".into()),
            shape: "s-4vcpu-4gb".into(),
            rootfs: Some("devbox:1".into()),
            ..Default::default()
        })
        .await?;

    let response = sandbox
        .run_command(
            RunCommandRequest {
                command: "sh".into(),
                arguments: vec!["-c".into(), "printf 'Rust says hello from '; uname -m".into()],
                ..Default::default()
            },
            Default::default(),
        )
        .await;

    let cleanup = sandbox.destroy().await;
    let response = response?;
    print!("{}", response.result.standard_output);
    cleanup?;
    Ok(())
}
```

```text
Rust says hello from x86_64
```

The builder configures authentication explicitly. Do not commit a real API key
to source control; inject it through your application's secret manager.
Additional builder methods configure the endpoint, timeout, and retry policy:

```rust,no_run
use createos::{Client, RetryOptions};
use std::time::Duration;

# fn example(api_key: String) -> createos::Result<()> {
let client = Client::builder()
    .api_key(api_key)
    .base_url("http://localhost:8080")
    .timeout(Duration::from_secs(30))
    .retry(RetryOptions {
        max_retries: 3,
        base_delay: Duration::from_millis(250),
        max_delay: Duration::from_secs(10),
    })
    .build()?;
# Ok(()) }
```

`Client::from_env()` reads `CREATEOS_SANDBOX_API_KEY` and the optional
`CREATEOS_SANDBOX_BASE_URL`. Explicit builder values take precedence. Never
commit an API key to source control.

Authenticated operations send the token only as `X-Api-Key`. Health,
readiness, shape, and root-filesystem catalog requests intentionally omit it.
Use HTTPS for every non-loopback endpoint. For custom proxy or TLS settings,
pass a `reqwest::ClientBuilder` to `Client::builder().http_client(...)`; the SDK
still disables redirects so credentials cannot be forwarded to another origin.

## Documentation

- [CreateOS Sandbox overview](https://nodeops.network/createos/docs/Sandbox/Overview)
- [CreateOS Sandbox API documentation](https://nodeops.network/createos/docs)
- Rust API reference: run `cargo doc --open` locally; docs.rs will be available
  after the first crates.io release
- [CreateOS TypeScript SDK](https://github.com/NodeOps-app/createos-sandbox-sdk)
- [Runnable examples](#examples)
- [Contributing guide](CONTRIBUTING.md)

## Stream output as it happens

```rust,no_run
# use createos::{Client, ExecStreamEventType, RunCommandRequest};
# async fn example(client: Client) -> createos::Result<()> {
# let sandbox = client.sandbox("sandbox-id").await?;
let mut stream = sandbox
    .stream_command(
        RunCommandRequest {
            command: "sh".into(),
            arguments: vec!["-c".into(), "for n in 1 2 3; do echo step-$n; sleep 1; done".into()],
            ..Default::default()
        },
        Default::default(),
    )
    .await?;

while let Some(event) = stream.next().await? {
    match event.kind.as_str() {
        ExecStreamEventType::STDOUT => print!("{}", event.data),
        ExecStreamEventType::STDERR => eprint!("{}", event.data),
        ExecStreamEventType::EXIT => println!("exit code: {:?}", event.exit_code),
        _ => {}
    }
}
# Ok(()) }
```

Dropping a stream or download response closes its HTTP response body.

## Move files without shell escaping

```rust,no_run
# use createos::{Client, RequestOptions};
# async fn example(client: Client) -> createos::Result<()> {
# let sandbox = client.sandbox("sandbox-id").await?;
sandbox
    .files()
    .upload(
        "/workspace/config.json",
        br#"{"mode":"production"}"#.to_vec(),
        &RequestOptions::default(),
    )
    .await?;

let contents = sandbox
    .files()
    .download("/workspace/config.json", &RequestOptions::default())
    .await?
    .bytes()
    .await?;
# Ok(()) }
```

Uploads accept any value convertible to `reqwest::Body` and are never retried.
Downloads return a `reqwest::Response`, allowing either buffered or streaming
consumption. A per-request timeout covers the complete transfer.

For large transfers, override the timeout for that operation without changing
the client's default:

```rust,no_run
# use createos::{Client, RequestOptions};
# use std::time::Duration;
# async fn example(client: Client) -> createos::Result<()> {
# let sandbox = client.sandbox("sandbox-id").await?;
let transfer = RequestOptions {
    timeout: Some(Duration::from_secs(30 * 60)),
    ..Default::default()
};

sandbox
    .files()
    .upload("/workspace/archive.tar", Vec::<u8>::new(), &transfer)
    .await?;
let download = sandbox
    .files()
    .download("/workspace/archive.tar", &transfer)
    .await?;
# Ok(()) }
```

The timeout remains active while the response body is consumed. Uploads are
not retried because an arbitrary request body may not be safe to replay after a
partial write.

## Keep a process alive after disconnecting

Managed processes are resources rather than terminal sessions. They support
output replay, binary input, signals, PTY resizing, and leader/tree waits:

```rust,no_run
# use createos::{Client, ManagedProcessCreateRequest, ManagedProcessWaitOptions, ManagedProcessWaitScope};
# use std::time::Duration;
# async fn example(client: Client) -> createos::Result<()> {
# let sandbox = client.sandbox("sandbox-id").await?;
let process = sandbox.processes().create(ManagedProcessCreateRequest {
    command: "python3".into(),
    arguments: vec!["-m".into(), "http.server".into(), "8080".into()],
    ..Default::default()
}).await?;

let completed = sandbox.processes().wait(
    &process.process_id,
    ManagedProcessWaitOptions {
        scope: Some(ManagedProcessWaitScope::TREE.into()),
        wait_timeout: Some(Duration::from_secs(30)),
        ..Default::default()
    },
).await?;
# Ok(()) }
```

## Turn a service into a URL

```rust,no_run
# use createos::{Client, CreateSandboxRequest, ManagedProcessCreateRequest};
# use std::time::Duration;
# async fn example(client: Client) -> createos::Result<()> {
let sandbox = client.create_sandbox(CreateSandboxRequest {
    shape: "s-1vcpu-1gb".into(),
    rootfs: Some("devbox:1".into()),
    ingress_enabled: true,
    ..Default::default()
}).await?;

sandbox.processes().create(ManagedProcessCreateRequest {
    command: "python3".into(),
    arguments: vec!["-m".into(), "http.server".into(), "8080".into(), "--bind".into(), "0.0.0.0".into()],
    ..Default::default()
}).await?;
sandbox.wait_for_port(None, 8080, Duration::from_secs(15)).await?;
println!("{}", sandbox.preview_url(8080)?);
# Ok(()) }
```

## Everything is already connected

Account-level services are available from the client:

```rust,no_run
# use createos::{Client, PaginationOptions};
# async fn example(client: Client) -> createos::Result<()> {
let templates = client.templates().list(PaginationOptions::default()).await?;
let networks = client.networks().list(PaginationOptions::default()).await?;
let disks = client.disks().list(PaginationOptions::default()).await?;
# Ok(()) }
```

Sandbox handles expose files, managed processes, mouse, keyboard, windows, and
screens. Lifecycle mutations update the handle's cached projection; call
`refresh()` to reload it explicitly.

```rust,no_run
# use createos::Client;
# async fn example(client: Client) -> createos::Result<()> {
let sandbox = client.sandbox("sandbox-id").await?;
let files = sandbox.files();
let processes = sandbox.processes();
let mouse = sandbox.computer().mouse();
let keyboard = sandbox.computer().keyboard();
let windows = sandbox.computer().windows();
let screens = sandbox.computer().screens();
# Ok(()) }
```

## Connect sandboxes on a private network

```rust,no_run
# use createos::{Client, NetworkCreateRequest};
# async fn example(client: Client) -> createos::Result<()> {
# let sandbox = client.sandbox("sandbox-id").await?;
let network = client.networks().create(NetworkCreateRequest {
    name: "agent-mesh".into(),
}).await?;
sandbox.attach_network(&network.id).await?;

let connected = client.networks().get(&network.id).await?;
for member in connected.members {
    println!("sandbox={} private-ip={} status={}",
        member.sandbox_id, member.ip_address, member.status);
}
# Ok(()) }
```

## Lifecycle reads like the domain

```rust,no_run
# use createos::{Client, ForkSandboxRequest, WaitOptions};
# async fn example(client: Client) -> createos::Result<()> {
# let sandbox = client.sandbox("sandbox-id").await?;
sandbox.pause().await?;
sandbox.wait_until_paused(WaitOptions::default()).await?;

let fork = sandbox.fork(ForkSandboxRequest::default()).await?;
sandbox.resume().await?;
fork.destroy().await?;
sandbox.destroy().await?;
# Ok(()) }
```

The `Instance` handle caches the latest server projection safely. Lifecycle
mutations and `refresh()` update it, while `id()`, `name()`, `status()`,
`ip_address()`, and `data()` provide synchronized reads.

## Errors stay inspectable

All operations return `createos::Result<T>`. Match `Error::Api(error)` to inspect
the HTTP status, stable API code, request ID, headers, endpoint, and raw body.
`Error::Command(error)` retains the complete response for a failed shell command,
and `Error::Timeout` identifies SDK polling timeouts.

```rust,no_run
# use createos::{Client, Error};
# async fn example(client: Client) {
match client.who_am_i().await {
    Ok(identity) => println!("user: {}", identity.user_id),
    Err(Error::Api(error)) => {
        eprintln!(
            "HTTP {}, code={:?}, request={:?}",
            error.status, error.code, error.request_id
        );
    }
    Err(Error::Timeout(duration)) => {
        eprintln!("operation timed out after {duration:?}");
    }
    Err(error) => eprintln!("request failed: {error}"),
}
# }
```

## Examples

Runnable examples cover the primary sandbox workflows:

- [Hello world](examples/hello_world.rs)
- [HTTP execution server](examples/execution-server/README.md)
- [Command streaming](examples/command_streaming.rs)
- [Files and snapshots](examples/files_and_snapshots.rs)
- [Ingress preview](examples/ingress_preview.rs)
- [Private overlay network](examples/network.rs)
- [Custom template](examples/custom_template.rs)
- [Managed process lifecycle](examples/managed_process.rs)
- [Desktop and noVNC](examples/desktop.rs)

Run one with the API key in the environment:

```sh
export CREATEOS_SANDBOX_API_KEY="your-api-key"
cargo run --example hello_world
```

## Development

Tool versions are pinned in `.tool-versions` for asdf:

```sh
asdf install
make check
make test
```

The checks run `rustfmt`, Clippy with warnings denied, all targets and examples,
tests, and rustdoc. Commits follow Conventional Commits and CI repeats the same
checks for pushes and pull requests. See [CONTRIBUTING.md](CONTRIBUTING.md) for
the accepted commit types and examples.

## Package layout

```text
src/client.rs     client configuration and account operations
src/instance.rs   stateful sandbox lifecycle and commands
src/services.rs   files, processes, templates, networks, disks, and desktop APIs
src/models.rs     public request, response, option, and wire-value contracts
src/transport.rs  HTTP, JSend, retries, timeouts, and authentication
examples/         independently runnable programs
```

## About CreateOS

[CreateOS](https://createos.sh) is an execution and governance platform for AI
agents and applications. Learn more about isolated Firecracker-based workloads
on the [CreateOS Sandbox product page](https://createos.sh/products/sandbox).

## License

MIT. See [LICENSE](LICENSE).

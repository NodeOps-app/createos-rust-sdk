# Execution server

This example turns the CreateOS Rust SDK into a small HTTP execution service.
A `POST /v1/execute` request creates a fresh sandbox, runs one command, captures
its output, destroys the sandbox, and returns JSON.

```sh
export CREATEOS_API_KEY="your-api-key"
cargo run --example execution_server
```

It listens on `127.0.0.1:8080` by default:

```sh
curl --fail-with-body http://127.0.0.1:8080/v1/execute \
  --header 'Content-Type: application/json' \
  --data '{"command":"python3","arguments":["-c","print(sum(range(10)))"]}'
```

The server limits request bodies to 1 MiB and permits four concurrent
executions. It intentionally binds only to localhost. Add authentication,
authorization, rate limiting, audit logging, and workload policy before
exposing a similar service to any network.

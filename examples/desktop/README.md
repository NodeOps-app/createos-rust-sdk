# Desktop and noVNC

Starts a desktop sandbox, takes a screenshot, checks mouse and clipboard operations, opens a URL, and prints a temporary noVNC connection URL. The example destroys the sandbox when it finishes. See [main.rs](main.rs) for the code.

From the repository root:

```sh
export CREATEOS_API_KEY="your-api-key"
cargo run --example desktop
```

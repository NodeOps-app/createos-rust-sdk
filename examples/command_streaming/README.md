# Command streaming

Uploads a Python script and prints its command output as streaming events arrive. The example destroys the sandbox when it finishes. See [main.rs](main.rs) for the code.

From the repository root:

```sh
export CREATEOS_API_KEY="your-api-key"
cargo run --example command_streaming
```

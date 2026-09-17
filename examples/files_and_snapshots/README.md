# Files and snapshots

Uploads a file, pauses and forks a sandbox, then checks that the fork inherits the file while later changes remain separate. The example destroys both sandboxes. See [main.rs](main.rs) for the code.

From the repository root:

```sh
export CREATEOS_API_KEY="your-api-key"
cargo run --example files_and_snapshots
```

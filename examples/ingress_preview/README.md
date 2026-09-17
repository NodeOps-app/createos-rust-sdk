# Ingress preview

Creates an ingress-enabled sandbox, serves an HTML page with a managed process, and fetches it through a preview URL. The example destroys the sandbox when it finishes. See [main.rs](main.rs) for the code.

From the repository root:

```sh
export CREATEOS_API_KEY="your-api-key"
cargo run --example ingress_preview
```

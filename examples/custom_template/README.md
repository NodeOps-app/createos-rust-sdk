# Custom template

Builds a Docker-enabled template, follows its build logs, starts a sandbox from it, and runs `hello-world` in Docker. The example destroys the sandbox and deletes the template when it finishes. See [main.rs](main.rs) for the code.

From the repository root:

```sh
export CREATEOS_API_KEY="your-api-key"
cargo run --example custom_template
```

The template build installs Docker and can take several minutes.

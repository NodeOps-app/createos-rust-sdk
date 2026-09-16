use createos::{Client, CreateSandboxRequest};

#[tokio::main]
async fn main() -> createos::Result<()> {
    let client = Client::from_env()?;
    let sandbox = client
        .create_sandbox(CreateSandboxRequest {
            shape: "s-1vcpu-1gb".into(),
            rootfs: Some("devbox:1".into()),
            ..Default::default()
        })
        .await?;
    let output = sandbox.shell("echo Hello from Rust").await;

    let cleanup = sandbox.destroy().await;
    print!("{}", output?.result.standard_output);
    cleanup
}

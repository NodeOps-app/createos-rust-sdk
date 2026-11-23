use createos::{Client, CreateSandboxRequest};

#[tokio::main]
async fn main() -> createos::Result<()> {
    let client = Client::from_env()?;
    let sandbox = client
        .create_sandbox(CreateSandboxRequest {
            shape: "s-1vcpu-1gb".into(),
            rootfs: Some("devbox:1".into()),
            timeout_minutes: Some(10),
            sigterm_before_expiry: true,
            ..Default::default()
        })
        .await?;
    let expiry = sandbox.data();
    println!(
        "expires_at={:?} sigterm_before_expiry={}",
        expiry.expires_at, expiry.sigterm_before_expiry
    );
    let output = sandbox.shell("echo Hello from Rust").await;

    let cleanup = sandbox.destroy().await;
    print!("{}", output?.result.standard_output);
    cleanup
}

use createos::{Client, CreateSandboxRequest, ExecOptions, RunCommandRequest};

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
    println!("created: {}", sandbox.id());

    let result = sandbox
        .run_command(
            RunCommandRequest {
                command: "sh".into(),
                arguments: vec![
                    "-c".into(),
                    "printf 'Rust says hello from '; uname -m".into(),
                ],
                ..Default::default()
            },
            ExecOptions::default(),
        )
        .await;

    let cleanup = sandbox.destroy().await;
    let response = result?;
    print!("{}", response.result.standard_output);
    cleanup?;
    println!("destroyed");
    Ok(())
}

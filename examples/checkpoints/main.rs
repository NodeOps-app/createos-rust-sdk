use createos::{Client, CreateCheckpointRequest, CreateSandboxRequest};

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

    let result = async {
        sandbox
            .shell("mkdir -p /root/project && printf version-1 > /root/project/version")
            .await?;
        let checkpoint = sandbox
            .checkpoints()
            .create(CreateCheckpointRequest {
                path: "/root/project".into(),
                name: Some("before-update".into()),
            })
            .await?;

        sandbox
            .shell("printf version-2 > /root/project/version")
            .await?;
        sandbox.checkpoints().restore(&checkpoint.id).await?;
        let restored = sandbox.shell("cat /root/project/version").await?;
        println!("restored {}", restored.result.standard_output.trim());

        sandbox.checkpoints().delete(&checkpoint.id).await
    }
    .await;

    let cleanup = sandbox.destroy().await;
    result?;
    cleanup
}

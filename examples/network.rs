use createos::{Client, CreateSandboxRequest, NetworkCreateRequest};
use std::time::{SystemTime, UNIX_EPOCH};

#[tokio::main]
async fn main() -> createos::Result<()> {
    let client = Client::from_env()?;
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let network = client
        .networks()
        .create(NetworkCreateRequest {
            name: format!("rust-sdk-{stamp}"),
        })
        .await?;
    println!("created network: {}", network.id);
    let sandbox = match client
        .create_sandbox(CreateSandboxRequest {
            shape: "s-1vcpu-1gb".into(),
            rootfs: Some("devbox:1".into()),
            ..Default::default()
        })
        .await
    {
        Ok(sandbox) => sandbox,
        Err(error) => {
            let _ = client.networks().delete(&network.id).await;
            return Err(error);
        }
    };
    println!("created sandbox: {}", sandbox.id());

    let result = async {
        sandbox.attach_network(&network.id).await?;
        let connected = client.networks().get(&network.id).await?;
        let member = connected
            .members
            .iter()
            .find(|member| member.sandbox_id == sandbox.id())
            .ok_or_else(|| {
                createos::Error::Protocol("sandbox missing from network membership".into())
            })?;
        println!(
            "verified member: sandbox={} ip={} status={}",
            member.sandbox_id, member.ip_address, member.status
        );
        sandbox.detach_network(&network.id).await
    }
    .await;

    let sandbox_cleanup = sandbox.destroy().await;
    let network_cleanup = client.networks().delete(&network.id).await;
    result?;
    sandbox_cleanup?;
    network_cleanup?;
    println!("destroyed sandbox and network");
    Ok(())
}

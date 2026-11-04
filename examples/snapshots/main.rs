use createos::{Client, CreateSandboxRequest, CreateSnapshotRequest, SnapshotStatus, SnapshotType};
use std::time::Duration;

#[tokio::main]
async fn main() -> createos::Result<()> {
    let client = Client::from_env()?;
    let source = client
        .create_sandbox(CreateSandboxRequest {
            shape: "s-1vcpu-1gb".into(),
            rootfs: Some("alpine:3.20".into()),
            ..Default::default()
        })
        .await?;

    let mut snapshot_id = None;
    let result = async {
        let mut snapshot = source
            .create_snapshot(CreateSnapshotRequest {
                kind: SnapshotType::DISK.into(),
                name: Some("configured-workspace".into()),
            })
            .await?;
        snapshot_id = Some(snapshot.id.clone());
        let deadline = tokio::time::Instant::now() + Duration::from_secs(300);
        while snapshot.status.as_str() == SnapshotStatus::CREATING {
            if tokio::time::Instant::now() >= deadline {
                return Err(createos::Error::InvalidArgument(
                    "timed out waiting for snapshot".into(),
                ));
            }
            tokio::time::sleep(Duration::from_secs(1)).await;
            snapshot = client.snapshots().get(&snapshot.id).await?;
        }
        if snapshot.status.as_str() != SnapshotStatus::READY {
            return Err(createos::Error::InvalidArgument(format!(
                "snapshot entered {}",
                snapshot.status
            )));
        }

        let restored = client
            .create_sandbox(CreateSandboxRequest {
                snapshot_id: Some(snapshot.id.clone()),
                ..Default::default()
            })
            .await?;
        println!(
            "restored sandbox {} from snapshot {}",
            restored.id(),
            snapshot.id
        );
        restored.destroy().await?;
        Ok::<(), createos::Error>(())
    }
    .await;

    source.destroy().await?;
    if let Some(id) = snapshot_id {
        let snapshot = client.snapshots().get(&id).await?;
        if matches!(
            snapshot.status.as_str(),
            SnapshotStatus::READY | SnapshotStatus::FAILED
        ) {
            client.snapshots().delete(&id).await?;
        }
    }
    result
}

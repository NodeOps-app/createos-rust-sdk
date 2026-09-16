use createos::{
    Client, CreateSandboxRequest, ExecOptions, ForkSandboxRequest, RequestOptions,
    RunCommandRequest, WaitOptions,
};
use std::time::Duration;

const BASE_PATH: &str = "/root/seed.txt";
const FORK_PATH: &str = "/root/fork-only.txt";

async fn read_file(sandbox: &createos::Instance, path: &str) -> createos::Result<String> {
    let response = sandbox
        .run_command(
            RunCommandRequest {
                command: "sh".into(),
                arguments: vec!["-c".into(), format!("cat {path} 2>&1")],
                ..Default::default()
            },
            ExecOptions::default(),
        )
        .await?;
    Ok(response.result.standard_output.trim().to_owned())
}

#[tokio::main]
async fn main() -> createos::Result<()> {
    let client = Client::from_env()?;
    let base = client
        .create_sandbox(CreateSandboxRequest {
            shape: "s-1vcpu-1gb".into(),
            rootfs: Some("devbox:1".into()),
            ..Default::default()
        })
        .await?;
    println!("base created: {}", base.id());

    let result = async {
        base.files()
            .upload(
                BASE_PATH,
                b"seed written by Rust\n".to_vec(),
                &RequestOptions::default(),
            )
            .await?;
        println!("wrote {BASE_PATH}: {}", read_file(&base, BASE_PATH).await?);
        base.pause().await?;
        base.wait_until_paused(WaitOptions {
            timeout: Duration::from_secs(600),
            ..Default::default()
        })
        .await?;

        let fork = base
            .fork(ForkSandboxRequest {
                start_paused: true,
                ..Default::default()
            })
            .await?;
        println!("fork created: {}", fork.id());
        let fork_result = async {
            fork.wait_until_paused(WaitOptions {
                timeout: Duration::from_secs(600),
                ..Default::default()
            })
            .await?;
            fork.resume().await?;
            fork.wait_until_running(WaitOptions {
                timeout: Duration::from_secs(300),
                ..Default::default()
            })
            .await?;
            println!(
                "fork inherited {BASE_PATH}: {}",
                read_file(&fork, BASE_PATH).await?
            );
            fork.files()
                .upload(
                    FORK_PATH,
                    b"written only in fork\n".to_vec(),
                    &RequestOptions::default(),
                )
                .await?;

            base.resume().await?;
            base.wait_until_running(WaitOptions {
                timeout: Duration::from_secs(300),
                ..Default::default()
            })
            .await?;
            let missing = read_file(&base, FORK_PATH).await?;
            println!("base does not see fork-only file: {missing:?}");
            Ok::<_, createos::Error>(())
        }
        .await;
        let fork_cleanup = fork.destroy().await;
        fork_result?;
        fork_cleanup
    }
    .await;

    let cleanup = base.destroy().await;
    result?;
    cleanup?;
    println!("destroyed base and fork");
    Ok(())
}

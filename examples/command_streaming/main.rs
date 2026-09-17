use createos::{
    Client, CreateSandboxRequest, ExecOptions, ExecStreamEventType, RequestOptions,
    RunCommandRequest,
};

const SCRIPT: &str = r#"import time
for number in range(1, 6):
    print(f"result {number}", flush=True)
    time.sleep(1)
"#;

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

    let result = async {
        sandbox
            .files()
            .upload(
                "/tmp/script.py",
                SCRIPT.as_bytes().to_vec(),
                &RequestOptions::default(),
            )
            .await?;
        let mut stream = sandbox
            .stream_command(
                RunCommandRequest {
                    command: "python3".into(),
                    arguments: vec!["/tmp/script.py".into()],
                    ..Default::default()
                },
                ExecOptions::default(),
            )
            .await?;
        while let Some(event) = stream.next().await? {
            match event.kind.as_str() {
                ExecStreamEventType::STDOUT => print!("{}", event.data),
                ExecStreamEventType::STDERR => eprint!("{}", event.data),
                ExecStreamEventType::ERROR => eprintln!("agent error: {}", event.message),
                ExecStreamEventType::EXIT => println!("exited: {:?}", event.exit_code),
                _ => {}
            }
        }
        Ok::<_, createos::Error>(())
    }
    .await;

    let cleanup = sandbox.destroy().await;
    result?;
    cleanup?;
    println!("destroyed");
    Ok(())
}

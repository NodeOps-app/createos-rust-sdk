use createos::{
    Client, CreateSandboxRequest, ExecOptions, ManagedProcessCreateRequest, RequestOptions,
    RunCommandRequest,
};
use std::time::Duration;

#[tokio::main]
async fn main() -> createos::Result<()> {
    let client = Client::from_env()?;
    let sandbox = client
        .create_sandbox(CreateSandboxRequest {
            shape: "s-1vcpu-1gb".into(),
            rootfs: Some("devbox:1".into()),
            ingress_enabled: true,
            ..Default::default()
        })
        .await?;
    println!("created: {}", sandbox.id());

    let result = async {
        sandbox
            .run_command(
                RunCommandRequest {
                    command: "mkdir".into(),
                    arguments: vec!["-p".into(), "/srv".into()],
                    ..Default::default()
                },
                ExecOptions::default(),
            )
            .await?;
        sandbox
            .files()
            .upload(
                "/srv/index.html",
                b"<h1>hello from CreateOS Rust SDK</h1>".to_vec(),
                &RequestOptions::default(),
            )
            .await?;
        sandbox
            .processes()
            .create(ManagedProcessCreateRequest {
                command: "python3".into(),
                arguments: vec![
                    "-m".into(),
                    "http.server".into(),
                    "8080".into(),
                    "--bind".into(),
                    "0.0.0.0".into(),
                ],
                working_directory: Some("/srv".into()),
                ..Default::default()
            })
            .await?;
        sandbox
            .wait_for_port(None, 8080, Duration::from_secs(15))
            .await?;
        let url = sandbox.preview_url(8080)?;
        println!("URL: {url}");
        let body = reqwest::Client::builder()
            .danger_accept_invalid_certs(true)
            .timeout(Duration::from_secs(30))
            .build()?
            .get(url)
            .send()
            .await?
            .error_for_status()?
            .text()
            .await?;
        println!("--- response ---\n{body}");
        Ok::<_, createos::Error>(())
    }
    .await;

    let cleanup = sandbox.destroy().await;
    result?;
    cleanup?;
    println!("destroyed");
    Ok(())
}

use createos::{
    Client, CreateSandboxRequest, ExecOptions, GetTemplateOptions, RequestOptions,
    RunCommandRequest, TemplateCreateRequest, TemplateLogsOptions, TemplateStatus,
};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const DOCKERFILE: &str = "FROM nodeops/sandbox:debian
RUN apt-get update -qq \
 && apt-get install -y --no-install-recommends curl ca-certificates \
 && curl -fsSL https://get.docker.com | sh \
 && rm -rf /var/lib/apt/lists/*
";

#[tokio::main]
async fn main() -> createos::Result<()> {
    let client = Client::from_env()?;
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    let template = client
        .templates()
        .create(TemplateCreateRequest {
            name: format!("docker-ce-rust-{stamp}"),
            dockerfile: DOCKERFILE.into(),
            base: None,
        })
        .await?;
    println!("template submitted: {} ({})", template.id, template.status);

    let result = async {
        let options = TemplateLogsOptions {
            request: RequestOptions {
                timeout: Some(Duration::from_secs(600)),
                ..Default::default()
            },
            ..Default::default()
        };
        if let Ok(mut logs) = client.templates().follow_logs(&template.id, options).await {
            while let Some(event) = logs.next().await? {
                if !event.line.is_empty() {
                    println!("{}", event.line);
                }
                if event.final_ {
                    break;
                }
            }
        }
        loop {
            let current = client
                .templates()
                .get(&template.id, GetTemplateOptions::default())
                .await?;
            match current.status.as_str() {
                TemplateStatus::READY => break,
                TemplateStatus::PENDING | TemplateStatus::BUILDING => {
                    tokio::time::sleep(Duration::from_secs(2)).await;
                }
                _ => {
                    return Err(createos::Error::Protocol(format!(
                        "template build ended in {}",
                        current.status
                    )));
                }
            }
        }

        let sandbox = client
            .create_sandbox(CreateSandboxRequest {
                shape: "s-1vcpu-1gb".into(),
                rootfs: Some(template.id.clone()),
                ..Default::default()
            })
            .await?;
        println!("sandbox created: {}", sandbox.id());
        let sandbox_result = async {
            sandbox
                .shell("nohup setsid dockerd > /var/log/dockerd.log 2>&1 &")
                .await?;
            let mut ready = false;
            for _ in 0..30 {
                let response = sandbox
                    .run_command(
                        RunCommandRequest {
                            command: "docker".into(),
                            arguments: vec![
                                "info".into(),
                                "--format".into(),
                                "{{.ServerVersion}}".into(),
                            ],
                            ..Default::default()
                        },
                        ExecOptions {
                            request: RequestOptions {
                                timeout: Some(Duration::from_secs(5)),
                                ..Default::default()
                            },
                            ..Default::default()
                        },
                    )
                    .await;
                if response.is_ok_and(|response| response.result.exit_code == 0) {
                    ready = true;
                    break;
                }
                tokio::time::sleep(Duration::from_secs(2)).await;
            }
            if !ready {
                return Err(createos::Error::Timeout(Duration::from_secs(60)));
            }
            let response = sandbox
                .run_command(
                    RunCommandRequest {
                        command: "docker".into(),
                        arguments: vec!["run".into(), "--rm".into(), "hello-world".into()],
                        ..Default::default()
                    },
                    ExecOptions {
                        request: RequestOptions {
                            timeout: Some(Duration::from_secs(120)),
                            ..Default::default()
                        },
                        ..Default::default()
                    },
                )
                .await?;
            println!("{}", response.result.standard_output.trim());
            if response.result.exit_code != 0 {
                return Err(createos::Error::Protocol(response.result.standard_error));
            }
            Ok::<_, createos::Error>(())
        }
        .await;
        let cleanup = sandbox.destroy().await;
        sandbox_result?;
        cleanup
    }
    .await;

    let cleanup = client.templates().delete(&template.id).await;
    result?;
    cleanup?;
    println!("deleted template");
    Ok(())
}

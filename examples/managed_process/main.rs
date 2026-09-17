use createos::{
    Client, CreateSandboxRequest, ManagedProcessConnectEventType, ManagedProcessConnectOptions,
    ManagedProcessCreateRequest, ManagedProcessDeleteOptions, ManagedProcessStream,
    ManagedProcessWaitOptions, PtySize,
};
use std::{collections::HashMap, time::Duration};

#[tokio::main]
async fn main() -> createos::Result<()> {
    let client = Client::from_env()?;
    let sandbox = client
        .create_sandbox(CreateSandboxRequest {
            shape: "s-1vcpu-1gb".into(),
            rootfs: Some("devbox:1".into()),
            environment_variables: HashMap::from([(
                "PROCESS_DEMO_BASE".into(),
                "from-sandbox-env".into(),
            )]),
            ..Default::default()
        })
        .await?;
    println!("created: {}", sandbox.id());

    let result = async {
        let processes = sandbox.processes();
        let process = processes.create(ManagedProcessCreateRequest {
            command: "/bin/sh".into(),
            arguments: vec!["-c".into(), "printf 'base:%s\\n' \"$PROCESS_DEMO_BASE\"; IFS= read -r line; printf 'stdin:%s\\n' \"$line\"".into()],
            working_directory: Some("/root".into()), ..Default::default()
        }).await?;
        processes.input(&process.process_id, "hello managed process\n").await?;
        processes.close_stdin(&process.process_id).await?;
        processes.wait(&process.process_id, ManagedProcessWaitOptions {
            scope: Some(createos::ManagedProcessWaitScope::TREE.into()), wait_timeout: Some(Duration::from_secs(5)), ..Default::default()
        }).await?;
        let output = collect(&mut processes.connect(&process.process_id, ManagedProcessConnectOptions::default()).await?).await?;
        println!("pipe output:\n{output}");

        let pty = processes.create(ManagedProcessCreateRequest {
            pty: Some(PtySize { rows: 24, cols: 80 }), working_directory: Some("/root".into()), ..Default::default()
        }).await?;
        processes.input(&pty.process_id, "echo terminal-ready; stty size\n").await?;
        processes.resize(&pty.process_id, PtySize { rows: 32, cols: 100 }).await?;
        processes.input(&pty.process_id, "echo after-resize; stty size; exit\n").await?;
        processes.wait(&pty.process_id, ManagedProcessWaitOptions { scope: Some(createos::ManagedProcessWaitScope::TREE.into()), wait_timeout: Some(Duration::from_secs(5)), ..Default::default() }).await?;
        let terminal = collect(&mut processes.connect(&pty.process_id, ManagedProcessConnectOptions::default()).await?).await?;
        println!("PTY output:\n{terminal}");

        let long = processes.create(ManagedProcessCreateRequest { command: "/bin/sh".into(), arguments: vec!["-c".into(), "trap '' TERM; sleep 300 & wait".into()], ..Default::default() }).await?;
        let terminated = processes.delete(&long.process_id, ManagedProcessDeleteOptions { grace_period: Some(Duration::from_millis(100)), ..Default::default() }).await?;
        if !output.contains("stdin:hello managed process") || !terminal.contains("terminal-ready") || !terminal.contains("after-resize") || !terminated.tree_exited {
            return Err(createos::Error::Protocol("managed process verification failed".into()));
        }
        Ok::<_, createos::Error>(())
    }.await;

    let cleanup = sandbox.destroy().await;
    result?;
    cleanup?;
    println!("destroyed");
    Ok(())
}

async fn collect(stream: &mut createos::ProcessStream) -> createos::Result<String> {
    let mut output = String::new();
    while let Some(event) = stream.next().await? {
        match event.kind.as_str() {
            ManagedProcessConnectEventType::DATA => match event.stream.as_str() {
                ManagedProcessStream::STDOUT
                | ManagedProcessStream::STDERR
                | ManagedProcessStream::PTY => {
                    output.push_str(&String::from_utf8_lossy(&event.data));
                }
                _ => {}
            },
            ManagedProcessConnectEventType::EXIT => break,
            ManagedProcessConnectEventType::ERROR => {
                return Err(createos::Error::Protocol(event.message));
            }
            _ => {}
        }
    }
    Ok(output)
}

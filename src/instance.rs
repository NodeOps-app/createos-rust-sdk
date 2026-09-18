use crate::{
    AttachDiskOptions, BandwidthView, ComputerService, DetachDiskOptions, DiskAttachment,
    DiskDetachedResponse, EgressView, Error, ExecOptions, FilesService, ForkSandboxRequest,
    PaginationOptions, ProcessesService, RequestOptions, ResizeSandboxResponse, Result,
    RunCommandRequest, RunCommandResponse, Sandbox, SandboxAccessTokenCreateResponse,
    SandboxAccessTokenMetadata, SandboxDisk, SandboxStatus, WaitOptions, client::encode,
    client::fetch_all, transport::Transport,
};
use reqwest::Method;
use serde::Serialize;
use std::{
    sync::{Arc, RwLock},
    time::Duration,
};
use url::Url;

/// Stateful handle to one sandbox.
#[derive(Clone)]
pub struct Instance {
    pub(crate) transport: Arc<Transport>,
    data: Arc<RwLock<Sandbox>>,
}

impl Instance {
    pub(crate) fn new(transport: Arc<Transport>, data: Sandbox) -> Self {
        Self {
            transport,
            data: Arc::new(RwLock::new(data)),
        }
    }
    /// Returns the sandbox identifier.
    pub fn id(&self) -> String {
        self.data().id
    }
    /// Returns the last observed name.
    pub fn name(&self) -> Option<String> {
        self.data().name
    }
    /// Returns the last observed status.
    pub fn status(&self) -> SandboxStatus {
        self.data().status
    }
    /// Returns the last observed private IP.
    pub fn ip_address(&self) -> Option<String> {
        self.data().ip_address
    }
    /// Returns a snapshot of the last observed server projection.
    pub fn data(&self) -> Sandbox {
        self.data
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }
    /// Returns file-transfer operations.
    pub fn files(&self) -> FilesService {
        FilesService::new(self.clone())
    }
    /// Returns managed-process operations.
    pub fn processes(&self) -> ProcessesService {
        ProcessesService::new(self.clone())
    }
    /// Returns desktop computer-use operations.
    pub fn computer(&self) -> ComputerService {
        ComputerService::new(self.clone())
    }

    /// Returns a separate handle that authenticates with a delegated sandbox token.
    ///
    /// Keep the original owner handle for token management. The server rejects
    /// management operations made with a delegated credential.
    pub fn with_access_token(&self, token: &str) -> Result<Self> {
        let token = token.trim();
        if token.is_empty() {
            return Err(Error::InvalidArgument(
                "sandbox access token must not be empty".into(),
            ));
        }
        Ok(Self::new(
            self.transport.with_api_key(token.to_owned()),
            self.data(),
        ))
    }

    /// Creates a delegated token and returns its plaintext value once.
    pub async fn create_access_token(&self) -> Result<SandboxAccessTokenCreateResponse> {
        self.transport
            .empty(
                Method::POST,
                &self.path("/access-token"),
                &[],
                &RequestOptions::default(),
            )
            .await
    }

    /// Returns delegated token state and a redacted hint.
    pub async fn get_access_token(&self) -> Result<SandboxAccessTokenMetadata> {
        self.transport
            .get(
                &self.path("/access-token"),
                &[],
                &RequestOptions::default(),
                true,
            )
            .await
    }

    /// Replaces an existing delegated token and returns its new plaintext value.
    pub async fn rotate_access_token(&self) -> Result<SandboxAccessTokenCreateResponse> {
        self.transport
            .empty(
                Method::POST,
                &self.path("/access-token/rotate"),
                &[],
                &RequestOptions::default(),
            )
            .await
    }

    /// Revokes the delegated token, if present.
    pub async fn disable_access_token(&self) -> Result<SandboxAccessTokenMetadata> {
        self.transport
            .empty(
                Method::DELETE,
                &self.path("/access-token"),
                &[],
                &RequestOptions::default(),
            )
            .await
    }
    pub(crate) fn path(&self, suffix: &str) -> String {
        format!("/v1/sandboxes/{}{suffix}", encode(&self.id()))
    }
    fn update(&self, data: Sandbox) {
        *self
            .data
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = data;
    }

    /// Reloads the sandbox projection.
    pub async fn refresh(&self) -> Result<()> {
        let data = self
            .transport
            .get(&self.path(""), &[], &RequestOptions::default(), true)
            .await?;
        self.update(data);
        Ok(())
    }
    /// Snapshots and pauses the sandbox.
    pub async fn pause(&self) -> Result<()> {
        self.lifecycle("/pause").await
    }
    /// Restores a paused sandbox.
    pub async fn resume(&self) -> Result<()> {
        self.lifecycle("/resume").await
    }
    async fn lifecycle(&self, suffix: &str) -> Result<()> {
        let data = self
            .transport
            .empty(
                Method::POST,
                &self.path(suffix),
                &[],
                &RequestOptions::default(),
            )
            .await?;
        self.update(data);
        Ok(())
    }
    /// Creates an independent sandbox fork.
    pub async fn fork(&self, request: ForkSandboxRequest) -> Result<Self> {
        let data = self
            .transport
            .send(
                Method::POST,
                &self.path("/fork"),
                &[],
                &request,
                &RequestOptions::default(),
            )
            .await?;
        Ok(Self::new(self.transport.clone(), data))
    }
    /// Starts sandbox destruction.
    pub async fn destroy(&self) -> Result<()> {
        #[derive(serde::Deserialize)]
        struct Destroyed {
            status: SandboxStatus,
        }
        let result: Destroyed = self
            .transport
            .empty(
                Method::DELETE,
                &self.path(""),
                &[],
                &RequestOptions::default(),
            )
            .await?;
        let mut data = self.data();
        data.status = result.status;
        self.update(data);
        Ok(())
    }
    /// Grows the overlay disk.
    pub async fn resize(&self, disk_mib: i64) -> Result<ResizeSandboxResponse> {
        #[derive(Serialize)]
        struct Body {
            disk_mib: i64,
        }
        let result: ResizeSandboxResponse = self
            .transport
            .send(
                Method::POST,
                &self.path("/resize"),
                &[],
                &Body { disk_mib },
                &RequestOptions::default(),
            )
            .await?;
        let mut data = self.data();
        data.disk_mib = result.disk_mib;
        self.update(data);
        Ok(result)
    }
    /// Enables or disables public ingress.
    pub async fn set_ingress(&self, enabled: bool) -> Result<()> {
        #[derive(Serialize)]
        struct Body {
            ingress_enabled: bool,
        }
        let data = self
            .transport
            .send(
                Method::PATCH,
                &self.path(""),
                &[],
                &Body {
                    ingress_enabled: enabled,
                },
                &RequestOptions::default(),
            )
            .await?;
        self.update(data);
        Ok(())
    }
    /// Sets the idle timeout. `None` disables auto-pause.
    pub async fn set_auto_pause(&self, timeout: Option<Duration>) -> Result<()> {
        #[derive(Serialize)]
        struct Body {
            #[serde(skip_serializing_if = "Option::is_none")]
            auto_pause_after_seconds: Option<u64>,
            disable_auto_pause: bool,
        }
        if timeout.is_some_and(|value| {
            value < Duration::from_secs(60)
                || value > Duration::from_secs(86_400)
                || value.subsec_nanos() != 0
        }) {
            return Err(Error::InvalidArgument(
                "auto-pause timeout must be whole seconds between 1 minute and 24 hours".into(),
            ));
        }
        let body = Body {
            auto_pause_after_seconds: timeout.map(|value| value.as_secs()),
            disable_auto_pause: timeout.is_none(),
        };
        let data = self
            .transport
            .send(
                Method::PATCH,
                &self.path(""),
                &[],
                &body,
                &RequestOptions::default(),
            )
            .await?;
        self.update(data);
        Ok(())
    }
    /// Adds OpenSSH public keys, returning the resulting count.
    pub async fn add_ssh_public_keys(&self, keys: Vec<String>) -> Result<usize> {
        #[derive(Serialize)]
        struct Body {
            keys: Vec<String>,
        }
        #[derive(serde::Deserialize)]
        struct Reply {
            count: usize,
        }
        let reply: Reply = self
            .transport
            .send(
                Method::POST,
                &self.path("/ssh-pubkeys"),
                &[],
                &Body { keys },
                &RequestOptions::default(),
            )
            .await?;
        Ok(reply.count)
    }

    /// Executes a command and buffers its output.
    pub async fn run_command(
        &self,
        mut request: RunCommandRequest,
        options: ExecOptions,
    ) -> Result<RunCommandResponse> {
        if let Some(standard_input) = options.standard_input {
            request.standard_input = Some(standard_input);
        }
        if let Some(environment_variables) = options.environment_variables {
            request.environment_variables = environment_variables;
        }
        request.stream = false;
        self.transport
            .send(
                Method::POST,
                &self.path("/exec"),
                &[],
                &request,
                &options.request,
            )
            .await
    }
    /// Runs a Bash script with default execution options.
    pub async fn shell(&self, script: impl Into<String>) -> Result<RunCommandResponse> {
        self.shell_with(script, ExecOptions::default()).await
    }
    /// Runs a Bash script with execution options and treats a nonzero exit as an error.
    pub async fn shell_with(
        &self,
        script: impl Into<String>,
        options: ExecOptions,
    ) -> Result<RunCommandResponse> {
        let response = self
            .run_command(
                RunCommandRequest {
                    command: "bash".into(),
                    arguments: vec!["-lc".into(), script.into()],
                    ..RunCommandRequest::default()
                },
                options,
            )
            .await?;
        if response.result.exit_code == 0 && response.result.error_message.is_empty() {
            return Ok(response);
        }
        Err(crate::CommandError { response }.into())
    }
    /// Starts a streaming command.
    pub async fn stream_command(
        &self,
        mut request: RunCommandRequest,
        options: ExecOptions,
    ) -> Result<crate::CommandStream> {
        if let Some(standard_input) = options.standard_input {
            request.standard_input = Some(standard_input);
        }
        if let Some(environment_variables) = options.environment_variables {
            request.environment_variables = environment_variables;
        }
        request.stream = true;
        let response = self
            .transport
            .stream_json(
                Method::POST,
                &self.path("/exec"),
                &[("stream".into(), "true".into())],
                Some(&request),
                &options.request,
            )
            .await?;
        Ok(crate::CommandStream::new(response))
    }

    /// Returns the public ingress URL for a sandbox port.
    pub fn preview_url(&self, port: u16) -> Result<Url> {
        if port == 0 {
            return Err(Error::InvalidArgument(
                "port must be between 1 and 65535".into(),
            ));
        }
        let data = self.data();
        if !data.ingress_enabled || data.ingress_url_template.is_empty() {
            return Err(Error::InvalidArgument(
                "sandbox ingress is not enabled".into(),
            ));
        }
        Url::parse(
            &data
                .ingress_url_template
                .replace("<port>", &port.to_string()),
        )
        .map_err(|error| Error::Protocol(format!("invalid ingress URL: {error}")))
    }
    /// Waits for a TCP port by executing an in-sandbox probe.
    pub async fn wait_for_port(
        &self,
        host: Option<&str>,
        port: u16,
        timeout: Duration,
    ) -> Result<()> {
        if port == 0 {
            return Err(Error::InvalidArgument(
                "port must be between 1 and 65535".into(),
            ));
        }
        let host = host.unwrap_or("127.0.0.1");
        if host.is_empty()
            || !host
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | ':' | '-'))
        {
            return Err(Error::InvalidArgument(format!("invalid host {host:?}")));
        }
        let timeout = if timeout.is_zero() {
            Duration::from_secs(30)
        } else {
            timeout
        };
        let seconds = timeout.as_secs().max(1);
        let script = format!(
            "timeout {seconds} bash -c 'until (echo > /dev/tcp/{host}/{port}) 2>/dev/null; do sleep 0.25; done'"
        );
        let response = self
            .run_command(
                RunCommandRequest {
                    command: "bash".into(),
                    arguments: vec!["-c".into(), script],
                    ..RunCommandRequest::default()
                },
                ExecOptions {
                    request: RequestOptions {
                        timeout: Some(timeout + Duration::from_secs(5)),
                        ..RequestOptions::default()
                    },
                    ..ExecOptions::default()
                },
            )
            .await?;
        if response.result.exit_code == 0 {
            Ok(())
        } else {
            Err(Error::Timeout(timeout))
        }
    }

    /// Waits until the sandbox is running.
    pub async fn wait_until_running(&self, options: WaitOptions) -> Result<()> {
        self.wait_for(
            SandboxStatus::RUNNING,
            &[
                SandboxStatus::ERROR,
                SandboxStatus::FAILED,
                SandboxStatus::DESTROYING,
                SandboxStatus::DESTROYED,
            ],
            options,
        )
        .await
    }
    /// Waits until the sandbox is paused.
    pub async fn wait_until_paused(&self, options: WaitOptions) -> Result<()> {
        self.wait_for(
            SandboxStatus::PAUSED,
            &[
                SandboxStatus::ERROR,
                SandboxStatus::FAILED,
                SandboxStatus::DESTROYING,
                SandboxStatus::DESTROYED,
            ],
            options,
        )
        .await
    }
    /// Waits until the sandbox is destroyed.
    pub async fn wait_until_destroyed(&self, options: WaitOptions) -> Result<()> {
        self.wait_for(
            SandboxStatus::DESTROYED,
            &[SandboxStatus::ERROR, SandboxStatus::FAILED],
            options,
        )
        .await
    }
    async fn wait_for(&self, desired: &str, terminal: &[&str], options: WaitOptions) -> Result<()> {
        let timeout = if options.timeout.is_zero() {
            Duration::from_secs(120)
        } else {
            options.timeout
        };
        let started = tokio::time::Instant::now();
        loop {
            if started.elapsed() >= timeout {
                return Err(Error::Timeout(timeout));
            }
            let data: Sandbox = self
                .transport
                .get(&self.path(""), &[], &options.request, true)
                .await?;
            let status = data.status.clone();
            self.update(data);
            if status.as_str() == desired {
                return Ok(());
            }
            if terminal.contains(&status.as_str()) {
                return Err(Error::Protocol(format!(
                    "sandbox entered terminal state {status}"
                )));
            }
            tokio::time::sleep(Duration::from_millis(500)).await;
        }
    }

    /// Returns the egress allowlist.
    pub async fn egress(&self) -> Result<EgressView> {
        self.transport
            .get(&self.path("/egress"), &[], &RequestOptions::default(), true)
            .await
    }
    /// Replaces the egress allowlist; an empty list allows all egress.
    pub async fn set_egress(&self, rules: Vec<String>) -> Result<EgressView> {
        #[derive(Serialize)]
        struct Body {
            #[serde(rename = "egress")]
            rules: Vec<String>,
        }
        self.transport
            .send(
                Method::PUT,
                &self.path("/egress"),
                &[],
                &Body { rules },
                &RequestOptions::default(),
            )
            .await
    }
    /// Returns bandwidth quota and usage.
    pub async fn bandwidth(&self) -> Result<BandwidthView> {
        self.transport
            .get(
                &self.path("/bandwidth"),
                &[],
                &RequestOptions::default(),
                true,
            )
            .await
    }
    /// Adds bytes to the bandwidth quota.
    pub async fn recharge_bandwidth(&self, bytes: i64) -> Result<BandwidthView> {
        #[derive(Serialize)]
        struct Body {
            add_bytes: i64,
        }
        self.transport
            .send(
                Method::POST,
                &self.path("/bandwidth/recharge"),
                &[],
                &Body { add_bytes: bytes },
                &RequestOptions::default(),
            )
            .await
    }
    /// Connects the sandbox to an overlay network.
    pub async fn attach_network(&self, id: &str) -> Result<()> {
        let _: ValueAck = self
            .transport
            .send(
                Method::POST,
                &self.path("/networks"),
                &[],
                &crate::NetworkEntry { id: id.into() },
                &RequestOptions::default(),
            )
            .await?;
        Ok(())
    }
    /// Disconnects the sandbox from an overlay network.
    pub async fn detach_network(&self, id: &str) -> Result<()> {
        let _: ValueAck = self
            .transport
            .empty(
                Method::DELETE,
                &self.path(&format!("/networks/{}", encode(id))),
                &[],
                &RequestOptions::default(),
            )
            .await?;
        Ok(())
    }
    /// Lists attached persistent disks.
    pub async fn disks(&self, options: PaginationOptions) -> Result<Vec<SandboxDisk>> {
        fetch_all(
            &self.transport,
            &self.path("/disks"),
            if options.offset == 0 {
                vec![]
            } else {
                vec![("offset".into(), options.offset.to_string())]
            },
            options.limit,
            Some("disks"),
            &RequestOptions::default(),
        )
        .await
    }
    /// Attaches a registered disk.
    pub async fn attach_disk(&self, options: AttachDiskOptions) -> Result<()> {
        #[derive(serde::Deserialize)]
        struct Id {
            id: String,
        }
        let result: Id = self
            .transport
            .send(
                Method::POST,
                &self.path("/disks"),
                &[],
                &DiskAttachment {
                    disk_id: options.disk_id,
                    mount_path: options.mount_path,
                    sub_path: options.sub_path,
                },
                &RequestOptions::default(),
            )
            .await?;
        if result.id == self.id() {
            Ok(())
        } else {
            Err(Error::Protocol(format!(
                "server acknowledged sandbox {:?}",
                result.id
            )))
        }
    }
    /// Detaches one persistent disk mount.
    pub async fn detach_disk(&self, options: DetachDiskOptions) -> Result<DiskDetachedResponse> {
        self.transport
            .empty(
                Method::DELETE,
                &self.path(&format!("/disks/{}", encode(&options.disk_id))),
                &[("mount_path".into(), options.mount_path)],
                &RequestOptions::default(),
            )
            .await
    }
}

#[derive(serde::Deserialize)]
struct ValueAck {
    #[allow(dead_code)]
    #[serde(default)]
    ok: bool,
}
/// Asks the local sandbox agent to pause its own sandbox.
pub async fn self_pause(reason: Option<&str>) -> Result<()> {
    self_signal("pause", reason).await
}
/// Asks the local sandbox agent to irreversibly delete its own sandbox.
pub async fn self_delete(reason: Option<&str>) -> Result<()> {
    self_signal("delete", reason).await
}
async fn self_signal(action: &str, reason: Option<&str>) -> Result<()> {
    let response = reqwest::Client::new()
        .post(format!("http://127.0.0.1:1029/self/{action}"))
        .query(&reason.map(|reason| [("reason", reason)]))
        .send()
        .await?;
    if response.status() == reqwest::StatusCode::ACCEPTED {
        Ok(())
    } else {
        Err(Error::Protocol(format!(
            "self-{action} returned HTTP {}",
            response.status()
        )))
    }
}

#[cfg(test)]
mod access_token_tests {
    use super::*;
    use std::{
        io::{Read as _, Write as _},
        net::TcpListener,
        sync::mpsc,
    };

    #[tokio::test]
    async fn token_lifecycle_keeps_owner_and_worker_credentials_separate() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let (sender, receiver) = mpsc::channel();
        let responses = [
            r#"{"status":"success","data":{"token":"skp_sb_first","enabled":true,"created_at":"2026-09-18T10:00:00Z"}}"#,
            r#"{"status":"success","data":{"enabled":true,"token_hint":"skp_sb...irst","created_at":"2026-09-18T10:00:00Z"}}"#,
            r#"{"status":"success","data":{"result":{"stdout":"hello\n","stderr":"","exit_code":0},"exec_ms":1}}"#,
            r#"{"status":"success","data":{"token":"skp_sb_second","enabled":true,"created_at":"2026-09-18T10:00:00Z","rotated_at":"2026-09-18T11:00:00Z"}}"#,
            r#"{"status":"success","data":{"enabled":false}}"#,
        ];
        std::thread::spawn(move || {
            for body in responses {
                let (mut connection, _) = listener.accept().unwrap();
                let mut request = [0_u8; 8192];
                let length = connection.read(&mut request).unwrap();
                sender
                    .send(String::from_utf8_lossy(&request[..length]).into_owned())
                    .unwrap();
                write!(
                    connection,
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                )
                .unwrap();
            }
        });
        let transport = Transport::new(
            Url::parse(&format!("http://{address}")).unwrap(),
            Some("owner".into()),
            None,
            None,
            crate::RetryOptions::default(),
            "test",
        )
        .unwrap();
        let owner = Instance::new(
            transport,
            Sandbox {
                id: "sb-1".into(),
                status: SandboxStatus::from("running"),
                ..Sandbox::default()
            },
        );
        assert!(matches!(
            owner.with_access_token("  "),
            Err(Error::InvalidArgument(_))
        ));

        let created = owner.create_access_token().await.unwrap();
        assert_eq!(created.token, "skp_sb_first");
        assert!(!format!("{created:?}").contains(&created.token));
        assert!(created.enabled && created.rotated_at.is_none());
        assert_eq!(
            owner
                .get_access_token()
                .await
                .unwrap()
                .token_hint
                .as_deref(),
            Some("skp_sb...irst")
        );
        let worker = owner.with_access_token(&created.token).unwrap();
        assert!(!Arc::ptr_eq(&owner.data, &worker.data));
        let result = worker
            .run_command(
                RunCommandRequest {
                    command: "echo".into(),
                    arguments: vec!["hello".into()],
                    ..RunCommandRequest::default()
                },
                ExecOptions::default(),
            )
            .await
            .unwrap();
        assert_eq!(result.result.standard_output, "hello\n");
        assert_eq!(
            owner.rotate_access_token().await.unwrap().token,
            "skp_sb_second"
        );
        assert!(!owner.disable_access_token().await.unwrap().enabled);

        let expected = [
            ("POST /v1/sandboxes/sb%2D1/access-token ", "owner"),
            ("GET /v1/sandboxes/sb%2D1/access-token ", "owner"),
            ("POST /v1/sandboxes/sb%2D1/exec ", "skp_sb_first"),
            ("POST /v1/sandboxes/sb%2D1/access-token/rotate ", "owner"),
            ("DELETE /v1/sandboxes/sb%2D1/access-token ", "owner"),
        ];
        for (line, credential) in expected {
            let request = receiver.recv().unwrap();
            assert!(request.starts_with(line), "{request}");
            assert!(
                request
                    .to_ascii_lowercase()
                    .contains(&format!("x-api-key: {credential}"))
            );
        }
    }
}

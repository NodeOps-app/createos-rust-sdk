use crate::{
    ApiError, ComputerButtonRequest, ComputerClickRequest, ComputerCreateScreenRequest,
    ComputerDragRequest, ComputerLaunchRequest, ComputerListWindowsOptions, ComputerOpenRequest,
    ComputerPoint, ComputerScreen, ComputerScreenConnection, ComputerScreenGeometry,
    ComputerScreenId, ComputerScreenOptions, ComputerScreenshotOptions, ComputerScrollRequest,
    ComputerTypeRequest, ComputerWindow, ComputerWindowGeometry, ComputerWindowMoveRequest,
    ComputerWindowResizeRequest, Disk, DiskCreateRequest, DiskCredentials, DiskDeletedResponse,
    Error, ExecStreamEventType, GetTemplateOptions, Instance, ManagedProcess,
    ManagedProcessConnectEvent, ManagedProcessConnectFrame, ManagedProcessConnectOptions,
    ManagedProcessCreateRequest, ManagedProcessDeleteOptions, ManagedProcessSignal,
    ManagedProcessWaitOptions, Network, NetworkCreateRequest, PaginationOptions, PtySize,
    RequestOptions, Result, Template, TemplateLogEvent, TemplateLogsOptions,
    client::{encode, fetch_all},
    transport::Transport,
};
use base64::Engine as _;
use bytes::{Bytes, BytesMut};
use futures_util::{StreamExt as _, stream::BoxStream};
use reqwest::{Method, Response};
use serde::{Serialize, de::DeserializeOwned};
use std::{collections::VecDeque, sync::Arc};

struct NdjsonStream {
    stream: BoxStream<'static, std::result::Result<Bytes, reqwest::Error>>,
    buffer: BytesMut,
    finished: bool,
}

impl NdjsonStream {
    fn new(response: Response) -> Self {
        Self {
            stream: response.bytes_stream().boxed(),
            buffer: BytesMut::new(),
            finished: false,
        }
    }
    async fn next<T: DeserializeOwned>(&mut self) -> Result<Option<T>> {
        loop {
            if let Some(position) = self.buffer.iter().position(|byte| *byte == b'\n') {
                let line = self.buffer.split_to(position + 1);
                if let Some(value) = decode_stream_line(&line)? {
                    return Ok(Some(value));
                }
                continue;
            }
            if self.finished {
                if self.buffer.is_empty() {
                    return Ok(None);
                }
                let line = self.buffer.split();
                return decode_stream_line(&line);
            }
            match self.stream.next().await {
                Some(Ok(chunk)) => self.buffer.extend_from_slice(&chunk),
                Some(Err(error)) => return Err(error.into()),
                None => self.finished = true,
            }
            if self.buffer.len() > 16 * 1024 * 1024 {
                return Err(Error::Protocol(
                    "stream event exceeds the 16 MiB limit".into(),
                ));
            }
        }
    }
}

fn decode_stream_line<T: DeserializeOwned>(line: &[u8]) -> Result<Option<T>> {
    let line = std::str::from_utf8(line)
        .map_err(|error| Error::Protocol(format!("stream is not UTF-8: {error}")))?
        .trim();
    if line.is_empty()
        || line.starts_with(':')
        || line.starts_with("event:")
        || line.starts_with("id:")
        || line.starts_with("retry:")
    {
        return Ok(None);
    }
    let line = line.strip_prefix("data:").map_or(line, str::trim);
    if line.is_empty() {
        return Ok(None);
    }
    Ok(Some(serde_json::from_str(line).map_err(|error| {
        Error::Protocol(format!("decode stream event: {error}"))
    })?))
}

/// Streaming command output.
pub struct CommandStream {
    inner: NdjsonStream,
    queued: VecDeque<crate::CommandStreamEvent>,
}
impl CommandStream {
    pub(crate) fn new(response: Response) -> Self {
        Self {
            inner: NdjsonStream::new(response),
            queued: VecDeque::new(),
        }
    }
    /// Receives the next event, or `None` at a clean end of stream.
    pub async fn next(&mut self) -> Result<Option<crate::CommandStreamEvent>> {
        loop {
            if let Some(event) = self.queued.pop_front() {
                return Ok(Some(event));
            }
            let Some(frame) = self.inner.next::<crate::CommandStreamFrame>().await? else {
                return Ok(None);
            };
            if frame.heartbeat {
                self.queued.push_back(event(ExecStreamEventType::HEARTBEAT));
            }
            if !frame.standard_output.is_empty() {
                let mut value = event(ExecStreamEventType::STDOUT);
                value.data = frame.standard_output;
                self.queued.push_back(value);
            }
            if !frame.standard_error.is_empty() {
                let mut value = event(ExecStreamEventType::STDERR);
                value.data = frame.standard_error;
                self.queued.push_back(value);
            }
            if !frame.error_message.is_empty() {
                let mut value = event(ExecStreamEventType::ERROR);
                value.message = frame.error_message;
                self.queued.push_back(value);
            }
            if let Some(code) = frame.exit_code {
                let mut value = event(ExecStreamEventType::EXIT);
                value.exit_code = Some(code);
                self.queued.push_back(value);
            }
        }
    }
}
fn event(kind: &str) -> crate::CommandStreamEvent {
    crate::CommandStreamEvent {
        kind: kind.into(),
        ..crate::CommandStreamEvent::default()
    }
}

/// File-transfer operations for one sandbox.
#[derive(Clone)]
pub struct FilesService {
    instance: Instance,
}
impl FilesService {
    pub(crate) fn new(instance: Instance) -> Self {
        Self { instance }
    }
    /// Uploads bytes to an absolute sandbox path. Uploads are never retried.
    pub async fn upload(
        &self,
        path: &str,
        data: impl Into<reqwest::Body>,
        options: &RequestOptions,
    ) -> Result<()> {
        let endpoint = self.instance.path("/files");
        let response = self
            .instance
            .transport
            .upload(
                Method::PUT,
                &endpoint,
                &[("path".into(), path.into())],
                data.into(),
                options,
            )
            .await?;
        if response.status().is_success() {
            Ok(())
        } else {
            Err(ApiError::from_response(&Method::PUT, &endpoint, response)
                .await
                .into())
        }
    }
    /// Opens a file download. The response body may be consumed as bytes or a stream.
    pub async fn download(&self, path: &str, options: &RequestOptions) -> Result<Response> {
        let endpoint = self.instance.path("/files");
        let response = self
            .instance
            .transport
            .raw(
                Method::GET,
                &endpoint,
                &[("path".into(), path.into())],
                options,
                true,
            )
            .await?;
        if response.status().is_success() {
            Ok(response)
        } else {
            Err(ApiError::from_response(&Method::GET, &endpoint, response)
                .await
                .into())
        }
    }
}

/// Managed process and PTY operations.
#[derive(Clone)]
pub struct ProcessesService {
    instance: Instance,
}
impl ProcessesService {
    pub(crate) fn new(instance: Instance) -> Self {
        Self { instance }
    }
    fn path(&self, id: Option<&str>, suffix: &str) -> String {
        let mut path = self.instance.path("/processes");
        if let Some(id) = id {
            path.push('/');
            path.push_str(&encode(id));
        }
        path.push_str(suffix);
        path
    }
    /// Starts a persistent process or PTY.
    pub async fn create(&self, request: ManagedProcessCreateRequest) -> Result<ManagedProcess> {
        self.instance
            .transport
            .send(
                Method::POST,
                &self.path(None, ""),
                &[],
                &request,
                &RequestOptions::default(),
            )
            .await
    }
    /// Lists retained processes.
    pub async fn list(&self) -> Result<Vec<ManagedProcess>> {
        #[derive(serde::Deserialize)]
        struct Reply {
            processes: Vec<ManagedProcess>,
        }
        let result: Reply = self
            .instance
            .transport
            .get(&self.path(None, ""), &[], &RequestOptions::default(), true)
            .await?;
        Ok(result.processes)
    }
    /// Gets one process.
    pub async fn get(&self, id: &str) -> Result<ManagedProcess> {
        self.instance
            .transport
            .get(
                &self.path(Some(id), ""),
                &[],
                &RequestOptions::default(),
                true,
            )
            .await
    }
    /// Opens a replayable output stream.
    pub async fn connect(
        &self,
        id: &str,
        options: ManagedProcessConnectOptions,
    ) -> Result<ProcessStream> {
        let response = self
            .instance
            .transport
            .stream_json(
                Method::GET,
                &self.path(Some(id), "/connect"),
                &[("after".into(), options.after_sequence.to_string())],
                Option::<&()>::None,
                &options.request,
            )
            .await?;
        Ok(ProcessStream {
            inner: NdjsonStream::new(response),
        })
    }
    /// Writes UTF-8 input, returning its input sequence.
    pub async fn input(&self, id: &str, data: &str) -> Result<i64> {
        self.input_bytes(id, data.as_bytes()).await
    }
    /// Writes binary input, returning its input sequence.
    pub async fn input_bytes(&self, id: &str, data: &[u8]) -> Result<i64> {
        #[derive(Serialize)]
        struct Body {
            data_base64: String,
        }
        #[derive(serde::Deserialize)]
        struct Reply {
            input_seq: i64,
        }
        let reply: Reply = self
            .instance
            .transport
            .send(
                Method::POST,
                &self.path(Some(id), "/input"),
                &[],
                &Body {
                    data_base64: base64::engine::general_purpose::STANDARD.encode(data),
                },
                &RequestOptions::default(),
            )
            .await?;
        Ok(reply.input_seq)
    }
    /// Closes a pipe process's standard input.
    pub async fn close_stdin(&self, id: &str) -> Result<()> {
        self.action(id, "/stdin/close", &()).await
    }
    /// Resizes a PTY.
    pub async fn resize(&self, id: &str, size: PtySize) -> Result<()> {
        self.action(id, "/resize", &size).await
    }
    /// Sends a signal.
    pub async fn signal(&self, id: &str, signal: ManagedProcessSignal) -> Result<()> {
        #[derive(Serialize)]
        struct Body {
            signal: ManagedProcessSignal,
        }
        self.action(id, "/signal", &Body { signal }).await
    }
    /// Long-polls until a leader or process tree exits.
    pub async fn wait(
        &self,
        id: &str,
        options: ManagedProcessWaitOptions,
    ) -> Result<ManagedProcess> {
        let mut query = vec![];
        if let Some(scope) = options.scope {
            query.push(("scope".into(), scope.to_string()));
        }
        if let Some(timeout) = options.wait_timeout {
            query.push(("timeout_ms".into(), timeout.as_millis().to_string()));
        }
        self.instance
            .transport
            .get(
                &self.path(Some(id), "/wait"),
                &query,
                &options.request,
                true,
            )
            .await
    }
    /// Terminates a process tree.
    pub async fn delete(
        &self,
        id: &str,
        options: ManagedProcessDeleteOptions,
    ) -> Result<ManagedProcess> {
        let query = options.grace_period.map_or_else(Vec::new, |value| {
            vec![("grace_ms".into(), value.as_millis().to_string())]
        });
        self.instance
            .transport
            .empty(
                Method::DELETE,
                &self.path(Some(id), ""),
                &query,
                &options.request,
            )
            .await
    }
    async fn action<B: Serialize + ?Sized>(&self, id: &str, suffix: &str, body: &B) -> Result<()> {
        let _: Ack = self
            .instance
            .transport
            .send(
                Method::POST,
                &self.path(Some(id), suffix),
                &[],
                body,
                &RequestOptions::default(),
            )
            .await?;
        Ok(())
    }
}

/// Replayable managed-process output.
pub struct ProcessStream {
    inner: NdjsonStream,
}
impl ProcessStream {
    /// Receives the next decoded event, or `None` at a clean end of stream.
    pub async fn next(&mut self) -> Result<Option<ManagedProcessConnectEvent>> {
        let Some(frame) = self.inner.next::<ManagedProcessConnectFrame>().await? else {
            return Ok(None);
        };
        let data = if frame.data_base64.is_empty() {
            vec![]
        } else {
            base64::engine::general_purpose::STANDARD.decode(frame.data_base64)?
        };
        Ok(Some(ManagedProcessConnectEvent {
            kind: frame.kind,
            sequence: frame.sequence,
            stream: frame.stream,
            data,
            exit_code: frame.exit_code,
            signal: frame.signal,
            message: frame.error_message,
            oldest_available_sequence: frame.oldest_available_seq,
        }))
    }
}

/// Custom root filesystem template operations.
#[derive(Clone)]
pub struct TemplatesService {
    transport: Arc<Transport>,
}
impl TemplatesService {
    pub(crate) fn new(transport: Arc<Transport>) -> Self {
        Self { transport }
    }
    /// Lists templates.
    pub async fn list(&self, options: PaginationOptions) -> Result<Vec<Template>> {
        fetch_all(
            &self.transport,
            "/v1/templates",
            offset_query(options.offset),
            options.limit,
            Some("templates"),
            &RequestOptions::default(),
        )
        .await
    }
    /// Creates a Dockerfile template.
    pub async fn create(&self, request: crate::TemplateCreateRequest) -> Result<Template> {
        self.transport
            .send(
                Method::POST,
                "/v1/templates",
                &[],
                &request,
                &RequestOptions::default(),
            )
            .await
    }
    /// Gets a template by ID.
    pub async fn get(&self, id: &str, options: GetTemplateOptions) -> Result<Template> {
        let query = options.include.map_or_else(Vec::new, |value| {
            vec![("include".into(), value.to_string())]
        });
        self.transport
            .get(
                &format!("/v1/templates/{}", encode(id)),
                &query,
                &options.request,
                true,
            )
            .await
    }
    /// Deletes a template.
    pub async fn delete(&self, id: &str) -> Result<()> {
        let _: Ack = self
            .transport
            .empty(
                Method::DELETE,
                &format!("/v1/templates/{}", encode(id)),
                &[],
                &RequestOptions::default(),
            )
            .await?;
        Ok(())
    }
    /// Returns collected build logs as plain text.
    pub async fn logs(&self, id: &str, options: TemplateLogsOptions) -> Result<String> {
        let path = format!("/v1/templates/{}/logs", encode(id));
        let query = attempt_query(options.attempt);
        let response = self
            .transport
            .raw(Method::GET, &path, &query, &options.request, true)
            .await?;
        if !response.status().is_success() {
            return Err(ApiError::from_response(&Method::GET, &path, response)
                .await
                .into());
        }
        Ok(response.text().await?)
    }
    /// Follows template build-log events.
    pub async fn follow_logs(
        &self,
        id: &str,
        options: TemplateLogsOptions,
    ) -> Result<TemplateLogStream> {
        let path = format!("/v1/templates/{}/logs", encode(id));
        let mut query = attempt_query(options.attempt);
        query.push(("follow".into(), "true".into()));
        let response = self
            .transport
            .stream_json(
                Method::GET,
                &path,
                &query,
                Option::<&()>::None,
                &options.request,
            )
            .await?;
        Ok(TemplateLogStream {
            inner: NdjsonStream::new(response),
        })
    }
}
/// Streaming template build logs.
pub struct TemplateLogStream {
    inner: NdjsonStream,
}
impl TemplateLogStream {
    /// Receives the next event, or `None` at end of stream.
    pub async fn next(&mut self) -> Result<Option<TemplateLogEvent>> {
        self.inner.next().await
    }
}

/// Overlay-network operations.
#[derive(Clone)]
pub struct NetworksService {
    transport: Arc<Transport>,
}
impl NetworksService {
    pub(crate) fn new(transport: Arc<Transport>) -> Self {
        Self { transport }
    }
    /// Lists overlay networks.
    pub async fn list(&self, options: PaginationOptions) -> Result<Vec<Network>> {
        fetch_all(
            &self.transport,
            "/v1/networks",
            offset_query(options.offset),
            options.limit,
            None,
            &RequestOptions::default(),
        )
        .await
    }
    /// Creates an overlay network.
    pub async fn create(&self, request: NetworkCreateRequest) -> Result<Network> {
        self.transport
            .send(
                Method::POST,
                "/v1/networks",
                &[],
                &request,
                &RequestOptions::default(),
            )
            .await
    }
    /// Gets an overlay network.
    pub async fn get(&self, id: &str) -> Result<Network> {
        self.transport
            .get(
                &format!("/v1/networks/{}", encode(id)),
                &[],
                &RequestOptions::default(),
                true,
            )
            .await
    }
    /// Deletes an overlay network.
    pub async fn delete(&self, id: &str) -> Result<()> {
        let _: Ack = self
            .transport
            .empty(
                Method::DELETE,
                &format!("/v1/networks/{}", encode(id)),
                &[],
                &RequestOptions::default(),
            )
            .await?;
        Ok(())
    }
}

/// Registered persistent-disk operations.
#[derive(Clone)]
pub struct DisksService {
    transport: Arc<Transport>,
}
impl DisksService {
    pub(crate) fn new(transport: Arc<Transport>) -> Self {
        Self { transport }
    }
    /// Lists registered disks.
    pub async fn list(&self, options: PaginationOptions) -> Result<Vec<Disk>> {
        fetch_all(
            &self.transport,
            "/v1/disks",
            offset_query(options.offset),
            options.limit,
            Some("disks"),
            &RequestOptions::default(),
        )
        .await
    }
    /// Registers an S3-backed disk.
    pub async fn create(&self, request: DiskCreateRequest) -> Result<Disk> {
        self.transport
            .send(
                Method::POST,
                "/v1/disks",
                &[],
                &request,
                &RequestOptions::default(),
            )
            .await
    }
    /// Gets a disk by ID or user-scoped name.
    pub async fn get(&self, id_or_name: &str) -> Result<Disk> {
        self.transport
            .get(
                &format!("/v1/disks/{}", encode(id_or_name)),
                &[],
                &RequestOptions::default(),
                true,
            )
            .await
    }
    /// Deletes a disk registration without modifying bucket contents.
    pub async fn delete(&self, id_or_name: &str) -> Result<DiskDeletedResponse> {
        self.transport
            .empty(
                Method::DELETE,
                &format!("/v1/disks/{}", encode(id_or_name)),
                &[],
                &RequestOptions::default(),
            )
            .await
    }
    /// Replaces stored disk credentials.
    pub async fn rotate_credentials(
        &self,
        id_or_name: &str,
        credentials: DiskCredentials,
    ) -> Result<Disk> {
        #[derive(Serialize)]
        struct Body {
            credentials: DiskCredentials,
        }
        self.transport
            .send(
                Method::PATCH,
                &format!("/v1/disks/{}", encode(id_or_name)),
                &[],
                &Body { credentials },
                &RequestOptions::default(),
            )
            .await
    }
}

/// Desktop computer-use operations.
#[derive(Clone)]
pub struct ComputerService {
    instance: Instance,
}
impl ComputerService {
    pub(crate) fn new(instance: Instance) -> Self {
        Self { instance }
    }
    fn path(&self, suffix: &str) -> String {
        self.instance.path(&format!("/computer{suffix}"))
    }
    /// Returns mouse operations.
    pub fn mouse(&self) -> MouseService {
        MouseService {
            computer: self.clone(),
        }
    }
    /// Returns keyboard operations.
    pub fn keyboard(&self) -> KeyboardService {
        KeyboardService {
            computer: self.clone(),
        }
    }
    /// Returns window operations.
    pub fn windows(&self) -> WindowsService {
        WindowsService {
            computer: self.clone(),
        }
    }
    /// Returns screen operations.
    pub fn screens(&self) -> ScreensService {
        ScreensService {
            computer: self.clone(),
        }
    }
    /// Captures a PNG screenshot.
    pub async fn screenshot(&self, options: ComputerScreenshotOptions) -> Result<Response> {
        let mut query = screen_query(options.screen.screen_id.as_ref());
        add_opt(&mut query, "window_id", options.window_id);
        add_opt(&mut query, "x", options.x);
        add_opt(&mut query, "y", options.y);
        add_opt(&mut query, "width", options.width);
        add_opt(&mut query, "height", options.height);
        let path = self.path("/screenshot");
        let response = self
            .instance
            .transport
            .raw(Method::GET, &path, &query, &options.screen.request, true)
            .await?;
        if response.status().is_success() {
            Ok(response)
        } else {
            Err(ApiError::from_response(&Method::GET, &path, response)
                .await
                .into())
        }
    }
    /// Returns active screen dimensions.
    pub async fn screen(&self, options: ComputerScreenOptions) -> Result<ComputerScreenGeometry> {
        self.get("/screen", options).await
    }
    /// Returns cursor coordinates.
    pub async fn cursor(&self, options: ComputerScreenOptions) -> Result<ComputerPoint> {
        self.get("/cursor", options).await
    }
    /// Returns clipboard text.
    pub async fn clipboard(
        &self,
        options: ComputerScreenOptions,
    ) -> Result<crate::ComputerClipboard> {
        self.get("/clipboard", options).await
    }
    /// Replaces clipboard text.
    pub async fn set_clipboard(
        &self,
        text: impl Into<String>,
        options: ComputerScreenOptions,
    ) -> Result<()> {
        self.action(
            Method::PUT,
            "/clipboard",
            options,
            &crate::ComputerClipboard { text: text.into() },
        )
        .await
    }
    /// Opens a URL or desktop target.
    pub async fn open(
        &self,
        request: ComputerOpenRequest,
        options: ComputerScreenOptions,
    ) -> Result<()> {
        self.action(Method::POST, "/open", options, &request).await
    }
    /// Launches an installed desktop application.
    pub async fn launch(
        &self,
        request: ComputerLaunchRequest,
        options: ComputerScreenOptions,
    ) -> Result<()> {
        self.action(Method::POST, "/launch", options, &request)
            .await
    }
    async fn get<T: DeserializeOwned>(
        &self,
        suffix: &str,
        options: ComputerScreenOptions,
    ) -> Result<T> {
        self.instance
            .transport
            .get(
                &self.path(suffix),
                &screen_query(options.screen_id.as_ref()),
                &options.request,
                true,
            )
            .await
    }
    async fn action<B: Serialize + ?Sized>(
        &self,
        method: Method,
        suffix: &str,
        options: ComputerScreenOptions,
        body: &B,
    ) -> Result<()> {
        let _: Ack = self
            .instance
            .transport
            .send(
                method,
                &self.path(suffix),
                &screen_query(options.screen_id.as_ref()),
                body,
                &options.request,
            )
            .await?;
        Ok(())
    }
}

/// Mouse operations.
#[derive(Clone)]
pub struct MouseService {
    computer: ComputerService,
}
impl MouseService {
    /// Moves the cursor.
    pub async fn move_to(
        &self,
        point: ComputerPoint,
        options: ComputerScreenOptions,
    ) -> Result<()> {
        self.computer
            .action(Method::POST, "/mouse/move", options, &point)
            .await
    }
    /// Clicks a mouse button.
    pub async fn click(
        &self,
        request: ComputerClickRequest,
        options: ComputerScreenOptions,
    ) -> Result<()> {
        self.computer
            .action(Method::POST, "/mouse/click", options, &request)
            .await
    }
    /// Scrolls the desktop.
    pub async fn scroll(
        &self,
        request: ComputerScrollRequest,
        options: ComputerScreenOptions,
    ) -> Result<()> {
        self.computer
            .action(Method::POST, "/mouse/scroll", options, &request)
            .await
    }
    /// Drags between two points.
    pub async fn drag(
        &self,
        request: ComputerDragRequest,
        options: ComputerScreenOptions,
    ) -> Result<()> {
        self.computer
            .action(Method::POST, "/mouse/drag", options, &request)
            .await
    }
    /// Presses a mouse button.
    pub async fn down(
        &self,
        request: ComputerButtonRequest,
        options: ComputerScreenOptions,
    ) -> Result<()> {
        self.computer
            .action(Method::POST, "/mouse/down", options, &request)
            .await
    }
    /// Releases a mouse button.
    pub async fn up(
        &self,
        request: ComputerButtonRequest,
        options: ComputerScreenOptions,
    ) -> Result<()> {
        self.computer
            .action(Method::POST, "/mouse/up", options, &request)
            .await
    }
}
/// Keyboard operations.
#[derive(Clone)]
pub struct KeyboardService {
    computer: ComputerService,
}
impl KeyboardService {
    /// Types text.
    pub async fn type_text(
        &self,
        request: ComputerTypeRequest,
        options: ComputerScreenOptions,
    ) -> Result<()> {
        self.computer
            .action(Method::POST, "/keyboard/type", options, &request)
            .await
    }
    /// Presses and releases keys.
    pub async fn press(&self, keys: Vec<String>, options: ComputerScreenOptions) -> Result<()> {
        self.keys("/keyboard/press", keys, options).await
    }
    /// Presses keys without releasing them.
    pub async fn down(&self, keys: Vec<String>, options: ComputerScreenOptions) -> Result<()> {
        self.keys("/keyboard/down", keys, options).await
    }
    /// Releases keys.
    pub async fn up(&self, keys: Vec<String>, options: ComputerScreenOptions) -> Result<()> {
        self.keys("/keyboard/up", keys, options).await
    }
    async fn keys(
        &self,
        suffix: &str,
        keys: Vec<String>,
        options: ComputerScreenOptions,
    ) -> Result<()> {
        #[derive(Serialize)]
        struct Body {
            keys: Vec<String>,
        }
        self.computer
            .action(Method::POST, suffix, options, &Body { keys })
            .await
    }
}

/// Desktop window operations.
#[derive(Clone)]
pub struct WindowsService {
    computer: ComputerService,
}
impl WindowsService {
    fn path(id: &str, suffix: &str) -> String {
        format!("/windows/{}{suffix}", encode(id))
    }
    /// Lists visible windows.
    pub async fn list(&self, options: ComputerListWindowsOptions) -> Result<Vec<ComputerWindow>> {
        let mut query = screen_query(options.screen.screen_id.as_ref());
        add_opt(&mut query, "application", options.application);
        self.computer
            .instance
            .transport
            .get(
                &self.computer.path("/windows"),
                &query,
                &options.screen.request,
                true,
            )
            .await
    }
    /// Returns the active window.
    pub async fn current(&self, options: ComputerScreenOptions) -> Result<ComputerWindow> {
        self.computer.get("/windows/current", options).await
    }
    /// Returns a window by ID.
    pub async fn get(&self, id: &str, options: ComputerScreenOptions) -> Result<ComputerWindow> {
        self.computer.get(&Self::path(id, ""), options).await
    }
    /// Returns a window's geometry.
    pub async fn geometry(
        &self,
        id: &str,
        options: ComputerScreenOptions,
    ) -> Result<ComputerWindowGeometry> {
        self.computer
            .get(&Self::path(id, "/geometry"), options)
            .await
    }
    /// Focuses a window.
    pub async fn focus(&self, id: &str, options: ComputerScreenOptions) -> Result<()> {
        self.action(id, "focus", options, &()).await
    }
    /// Moves a window.
    pub async fn move_to(
        &self,
        id: &str,
        request: ComputerWindowMoveRequest,
        options: ComputerScreenOptions,
    ) -> Result<()> {
        self.action(id, "move", options, &request).await
    }
    /// Resizes a window.
    pub async fn resize(
        &self,
        id: &str,
        request: ComputerWindowResizeRequest,
        options: ComputerScreenOptions,
    ) -> Result<()> {
        self.action(id, "resize", options, &request).await
    }
    /// Maximizes a window.
    pub async fn maximize(&self, id: &str, options: ComputerScreenOptions) -> Result<()> {
        self.action(id, "maximize", options, &()).await
    }
    /// Minimizes a window.
    pub async fn minimize(&self, id: &str, options: ComputerScreenOptions) -> Result<()> {
        self.action(id, "minimize", options, &()).await
    }
    /// Restores a window.
    pub async fn restore(&self, id: &str, options: ComputerScreenOptions) -> Result<()> {
        self.action(id, "restore", options, &()).await
    }
    /// Closes a window.
    pub async fn close(&self, id: &str, options: ComputerScreenOptions) -> Result<()> {
        let _: Ack = self
            .computer
            .instance
            .transport
            .empty(
                Method::DELETE,
                &self.computer.path(&Self::path(id, "")),
                &screen_query(options.screen_id.as_ref()),
                &options.request,
            )
            .await?;
        Ok(())
    }
    async fn action<B: Serialize + ?Sized>(
        &self,
        id: &str,
        action: &str,
        options: ComputerScreenOptions,
        body: &B,
    ) -> Result<()> {
        self.computer
            .action(
                Method::POST,
                &Self::path(id, &format!("/{action}")),
                options,
                body,
            )
            .await
    }
}

/// Desktop screen operations.
#[derive(Clone)]
pub struct ScreensService {
    computer: ComputerService,
}
impl ScreensService {
    fn path(id: &ComputerScreenId, suffix: &str) -> String {
        format!("/screens/{}{suffix}", encode(id.as_str()))
    }
    /// Lists configured screens.
    pub async fn list(&self) -> Result<Vec<ComputerScreen>> {
        self.computer
            .instance
            .transport
            .get(
                &self.computer.path("/screens"),
                &[],
                &RequestOptions::default(),
                true,
            )
            .await
    }
    /// Creates a screen.
    pub async fn create(&self, request: ComputerCreateScreenRequest) -> Result<ComputerScreen> {
        self.computer
            .instance
            .transport
            .send(
                Method::POST,
                &self.computer.path("/screens"),
                &[],
                &request,
                &RequestOptions::default(),
            )
            .await
    }
    /// Gets one screen.
    pub async fn get(&self, id: &ComputerScreenId) -> Result<ComputerScreen> {
        self.computer
            .instance
            .transport
            .get(
                &self.computer.path(&Self::path(id, "")),
                &[],
                &RequestOptions::default(),
                true,
            )
            .await
    }
    /// Returns a temporary noVNC connection.
    pub async fn connect(&self, id: &ComputerScreenId) -> Result<ComputerScreenConnection> {
        self.computer
            .instance
            .transport
            .get(
                &self.computer.path(&Self::path(id, "/connect")),
                &[],
                &RequestOptions::default(),
                true,
            )
            .await
    }
    /// Resizes a screen.
    pub async fn resize(
        &self,
        id: &ComputerScreenId,
        request: ComputerCreateScreenRequest,
    ) -> Result<ComputerScreen> {
        self.computer
            .instance
            .transport
            .send(
                Method::POST,
                &self.computer.path(&Self::path(id, "/resize")),
                &[],
                &request,
                &RequestOptions::default(),
            )
            .await
    }
    /// Deletes a screen.
    pub async fn delete(&self, id: &ComputerScreenId) -> Result<()> {
        let _: Ack = self
            .computer
            .instance
            .transport
            .empty(
                Method::DELETE,
                &self.computer.path(&Self::path(id, "")),
                &[],
                &RequestOptions::default(),
            )
            .await?;
        Ok(())
    }
}

#[derive(serde::Deserialize)]
struct Ack {
    #[allow(dead_code)]
    #[serde(default)]
    ok: bool,
}
fn offset_query(offset: usize) -> Vec<(String, String)> {
    if offset == 0 {
        vec![]
    } else {
        vec![("offset".into(), offset.to_string())]
    }
}
fn attempt_query(attempt: Option<u32>) -> Vec<(String, String)> {
    attempt.map_or_else(Vec::new, |value| {
        vec![("attempt".into(), value.to_string())]
    })
}
fn screen_query(id: Option<&ComputerScreenId>) -> Vec<(String, String)> {
    id.map_or_else(Vec::new, |value| {
        vec![("screen_id".into(), value.to_string())]
    })
}
fn add_opt<T: ToString>(query: &mut Vec<(String, String)>, key: &str, value: Option<T>) {
    if let Some(value) = value {
        query.push((key.into(), value.to_string()));
    }
}

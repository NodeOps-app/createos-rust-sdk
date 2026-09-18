//! Public request, response, option, and enum types.

use chrono::{DateTime, Utc};
use reqwest::header::HeaderMap;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{collections::HashMap, fmt, time::Duration};

macro_rules! string_type {
    ($(#[$meta:meta])* $name:ident { $($(#[$vmeta:meta])* $constant:ident = $value:literal),+ $(,)? }) => {
        $(#[$meta])*
        #[derive(Clone, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $name(pub String);

        impl $name {
            $($(#[$vmeta])* #[doc = concat!("Wire value `", $value, "`.")] pub const $constant: &'static str = $value;)+
            /// Creates a wire value. Unknown server values are retained for forward compatibility.
            pub fn new(value: impl Into<String>) -> Self { Self(value.into()) }
            /// Returns the wire representation.
            pub fn as_str(&self) -> &str { &self.0 }
            /// Returns whether the wire representation is empty.
            pub fn is_empty(&self) -> bool { self.0.is_empty() }
        }
        impl From<&str> for $name { fn from(value: &str) -> Self { Self(value.to_owned()) } }
        impl From<String> for $name { fn from(value: String) -> Self { Self(value) } }
        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result { f.write_str(&self.0) }
        }
    };
}

string_type!(/// Worker scheduling state.
    HostStatus { ACTIVE = "active", DRAINING = "draining", DEAD = "dead" });
string_type!(/// Sandbox lifecycle state.
SandboxStatus {
    CREATING = "creating", RUNNING = "running", PAUSING = "pausing", PAUSED = "paused",
    RESUMING = "resuming", FORKING = "forking", ERROR = "error", DESTROYING = "destroying",
    DESTROYED = "destroyed", FAILED = "failed"
});
string_type!(/// Command stream event kind.
    ExecStreamEventType { STDOUT = "stdout", STDERR = "stderr", EXIT = "exit", ERROR = "error", HEARTBEAT = "heartbeat" });
string_type!(/// Managed process execution kind.
    ManagedProcessKind { PROCESS = "process", PTY = "pty" });
string_type!(/// Managed process lifecycle state.
    ManagedProcessState { STARTING = "starting", RUNNING = "running", TERMINATING = "terminating", EXITED = "exited", FAILED = "failed" });
string_type!(/// Signal accepted by the process API.
ManagedProcessSignal {
    HANGUP = "SIGHUP", INTERRUPT = "SIGINT", QUIT = "SIGQUIT", KILL = "SIGKILL",
    TERMINATE = "SIGTERM", USER_DEFINED_1 = "SIGUSR1", USER_DEFINED_2 = "SIGUSR2",
    WINDOW_CHANGE = "SIGWINCH"
});
string_type!(/// Managed process output stream.
    ManagedProcessStream { STDOUT = "stdout", STDERR = "stderr", PTY = "pty" });
string_type!(/// Managed process connection event kind.
    ManagedProcessConnectEventType { DATA = "data", EXIT = "exit", HEARTBEAT = "heartbeat", ERROR = "error" });
string_type!(/// One of the eight desktop screens.
ComputerScreenId {
    SCREEN_0 = "screen-0", SCREEN_1 = "screen-1", SCREEN_2 = "screen-2", SCREEN_3 = "screen-3",
    SCREEN_4 = "screen-4", SCREEN_5 = "screen-5", SCREEN_6 = "screen-6", SCREEN_7 = "screen-7"
});
string_type!(/// Mouse button.
    ComputerMouseButton { LEFT = "left", MIDDLE = "middle", RIGHT = "right" });
string_type!(/// Vertical scroll direction.
    ComputerScrollDirection { UP = "up", DOWN = "down" });
string_type!(/// Template build state.
    TemplateStatus { PENDING = "pending", BUILDING = "building", READY = "ready", FAILED = "failed" });
string_type!(/// Optional template detail field.
    TemplateInclude { DOCKERFILE = "dockerfile" });
string_type!(/// Registered disk backend.
    DiskKind { S3 = "s3" });
string_type!(/// Sandbox disk mount state.
    DiskMountStatus { PENDING = "pending", MOUNTED = "mounted", ERROR = "error", UNMOUNTING = "unmounting" });
string_type!(/// Process boundary used when waiting.
    ManagedProcessWaitScope { LEADER = "leader", TREE = "tree" });

/// Retry policy.
#[derive(Clone, Debug)]
pub struct RetryOptions {
    /// Attempts after the initial request.
    pub max_retries: u32,
    /// Initial exponential-backoff delay.
    pub base_delay: Duration,
    /// Maximum delay between attempts.
    pub max_delay: Duration,
}

impl Default for RetryOptions {
    fn default() -> Self {
        Self {
            max_retries: 2,
            base_delay: Duration::from_millis(500),
            max_delay: Duration::from_secs(30),
        }
    }
}

/// Per-request transport overrides.
#[derive(Clone, Debug, Default)]
pub struct RequestOptions {
    /// Extra headers. Authentication cannot be overridden.
    pub headers: HeaderMap,
    /// Complete-request timeout.
    pub timeout: Option<Duration>,
    /// Retry override.
    pub retry: Option<RetryOptions>,
    /// Disables retries for this request.
    pub disable_retry: bool,
}

/// Sandbox list filters.
#[derive(Clone, Debug, Default)]
pub struct ListSandboxesOptions {
    /// Request transport overrides.
    pub request: RequestOptions,
    /// Maximum returned items; `None` walks all pages.
    pub limit: Option<usize>,
    /// Optional lifecycle filter.
    pub status: Option<SandboxStatus>,
}

/// Buffered or streaming command overrides.
#[derive(Clone, Debug, Default)]
pub struct ExecOptions {
    /// Request transport overrides.
    pub request: RequestOptions,
    /// Replaces request stdin when present.
    pub standard_input: Option<String>,
    /// Replaces request environment when present.
    pub environment_variables: Option<HashMap<String, String>>,
}

/// Process output replay options.
#[derive(Clone, Debug, Default)]
pub struct ManagedProcessConnectOptions {
    /// Request transport overrides.
    pub request: RequestOptions,
    /// Last sequence already consumed; replay starts after this value.
    pub after_sequence: i64,
}
/// Process wait options.
#[derive(Clone, Debug, Default)]
pub struct ManagedProcessWaitOptions {
    /// Request transport overrides.
    pub request: RequestOptions,
    /// Whether to wait for the leader or the complete process tree.
    pub scope: Option<ManagedProcessWaitScope>,
    /// Maximum server-side long-poll duration.
    pub wait_timeout: Option<Duration>,
}
/// Process deletion options.
#[derive(Clone, Debug, Default)]
pub struct ManagedProcessDeleteOptions {
    /// Request transport overrides.
    pub request: RequestOptions,
    /// Grace period before the process is forcefully terminated.
    pub grace_period: Option<Duration>,
}
/// Desktop operation options.
#[derive(Clone, Debug, Default)]
pub struct ComputerScreenOptions {
    /// Request transport overrides.
    pub request: RequestOptions,
    /// Screen to target, or the default screen when omitted.
    pub screen_id: Option<ComputerScreenId>,
}
/// Screenshot capture options.
#[derive(Clone, Debug, Default)]
pub struct ComputerScreenshotOptions {
    /// Screen and transport options.
    pub screen: ComputerScreenOptions,
    /// Optional window to capture.
    pub window_id: Option<String>,
    /// Optional crop origin on the x-axis.
    pub x: Option<i32>,
    /// Optional crop origin on the y-axis.
    pub y: Option<i32>,
    /// Optional crop width.
    pub width: Option<u32>,
    /// Optional crop height.
    pub height: Option<u32>,
}
/// Window-list filters.
#[derive(Clone, Debug, Default)]
pub struct ComputerListWindowsOptions {
    /// Screen and transport options.
    pub screen: ComputerScreenOptions,
    /// Optional application-name filter.
    pub application: Option<String>,
}
/// Template detail options.
#[derive(Clone, Debug, Default)]
pub struct GetTemplateOptions {
    /// Request transport overrides.
    pub request: RequestOptions,
    /// Optional additional template field to return.
    pub include: Option<TemplateInclude>,
}
/// Template log filters.
#[derive(Clone, Debug, Default)]
pub struct TemplateLogsOptions {
    /// Request transport overrides.
    pub request: RequestOptions,
    /// Optional build attempt to inspect.
    pub attempt: Option<u32>,
}
/// Pagination controls.
#[derive(Clone, Copy, Debug, Default)]
pub struct PaginationOptions {
    /// Maximum number of items to return; `None` walks every page.
    pub limit: Option<usize>,
    /// Initial result offset.
    pub offset: usize,
}
/// Sandbox status-polling options.
#[derive(Clone, Debug)]
pub struct WaitOptions {
    /// Total client-side polling budget.
    pub timeout: Duration,
    /// Request transport overrides used by each poll.
    pub request: RequestOptions,
}
impl Default for WaitOptions {
    fn default() -> Self {
        Self {
            timeout: Duration::from_secs(120),
            request: RequestOptions::default(),
        }
    }
}

macro_rules! model {
    ($(#[$meta:meta])* $name:ident { $($(#[$fmeta:meta])* $field:ident : $ty:ty $(=> $wire:literal)?),* $(,)? }) => {
        $(#[$meta])*
        #[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
        pub struct $name {
            $($(#[$fmeta])* $(#[serde(rename = $wire)])?
            #[doc = concat!("Value of the `", stringify!($field), "` API field.")]
            pub $field: $ty),*
        }
    };
}

model!(/// Overlay network reference.
    NetworkEntry { id: String });
model!(/// Persistent disk attachment.
    DiskAttachment { disk_id: String, mount_path: String, #[serde(default, skip_serializing_if = "Option::is_none")] sub_path: Option<String> });
model!(/// Sandbox creation body.
CreateSandboxRequest {
    shape: String,
    #[serde(default, skip_serializing_if = "Option::is_none")] rootfs: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")] name: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")] networks: Vec<NetworkEntry>,
    #[serde(default, skip_serializing_if = "Option::is_none")] disk_mib: Option<i64>,
    #[serde(default, skip_serializing_if = "Vec::is_empty", rename = "egress")] egress_rules: Vec<String>,
        #[serde(default, skip_serializing_if = "HashMap::is_empty", rename = "envs")] environment_variables: HashMap<String, String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty", rename = "ssh_pubkeys")] ssh_public_keys: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")] host_id: Option<String>,
    #[serde(default, skip_serializing_if = "HashMap::is_empty")] node_selector: HashMap<String, String>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")] ingress_enabled: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")] disks: Vec<DiskAttachment>,
    #[serde(default, skip_serializing_if = "Option::is_none")] region: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")] auto_pause_after_seconds: Option<u64>
});
model!(/// Optional overrides applied to a sandbox fork.
ForkSandboxRequest {
    #[serde(default, skip_serializing_if = "std::ops::Not::not")] start_paused: bool,
    #[serde(default, skip_serializing_if = "Option::is_none", rename = "ssh_pubkeys")] ssh_public_keys: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none", rename = "egress")] egress_rules: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")] ingress_enabled: Option<bool>,
        #[serde(default, skip_serializing_if = "Option::is_none", rename = "envs")] environment_variables: Option<HashMap<String, String>>
});
model!(/// Command execution request.
    RunCommandRequest {
        #[serde(rename = "cmd")] command: String,
        #[serde(default, skip_serializing_if = "Vec::is_empty", rename = "args")] arguments: Vec<String>,
        #[serde(default, skip_serializing_if = "Option::is_none", rename = "stdin")] standard_input: Option<String>,
        #[serde(default, skip_serializing_if = "HashMap::is_empty", rename = "env")] environment_variables: HashMap<String, String>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")] stream: bool
});
model!(/// PTY dimensions.
PtySize {
    #[serde(default, skip_serializing_if = "is_zero")] rows: u32,
    #[serde(default, skip_serializing_if = "is_zero")] cols: u32
});
model!(/// Persistent managed-process request.
    ManagedProcessCreateRequest {
        #[serde(default, skip_serializing_if = "String::is_empty", rename = "cmd")] command: String,
        #[serde(default, skip_serializing_if = "Vec::is_empty", rename = "args")] arguments: Vec<String>,
        #[serde(default, skip_serializing_if = "Option::is_none", rename = "cwd")] working_directory: Option<String>,
        #[serde(default, skip_serializing_if = "HashMap::is_empty", rename = "env")] environment_variables: HashMap<String, String>,
    #[serde(default, skip_serializing_if = "Option::is_none", rename = "pty")] pty: Option<PtySize>
});
model!(/// Mouse click request.
    ComputerClickRequest { #[serde(default, skip_serializing_if = "Option::is_none")] button: Option<ComputerMouseButton>, #[serde(default, skip_serializing_if = "Option::is_none")] x: Option<i32>, #[serde(default, skip_serializing_if = "Option::is_none")] y: Option<i32>, #[serde(default, skip_serializing_if = "Option::is_none")] count: Option<u32> });
model!(/// Mouse scroll request.
ComputerScrollRequest {
    #[serde(default, skip_serializing_if = "ComputerScrollDirection::is_empty")] direction: ComputerScrollDirection,
    #[serde(default, skip_serializing_if = "is_zero")] amount: i32
});
model!(/// Desktop coordinate.
    ComputerPoint { x: i32, y: i32 });
model!(/// Mouse drag request.
    ComputerDragRequest { from: ComputerPoint, to: ComputerPoint });
model!(/// Mouse button request.
ComputerButtonRequest {
    #[serde(default, skip_serializing_if = "ComputerMouseButton::is_empty")] button: ComputerMouseButton
});
model!(/// Keyboard typing request.
    ComputerTypeRequest { text: String, #[serde(default, skip_serializing_if = "Option::is_none", rename = "delay_in_ms")] delay_ms: Option<u64> });
model!(/// Desktop target request.
    ComputerOpenRequest { target: String });
model!(/// Desktop application launch request.
    ComputerLaunchRequest { application: String, #[serde(default, skip_serializing_if = "Option::is_none")] uri: Option<String> });
model!(/// Window move request.
    ComputerWindowMoveRequest { x: i32, y: i32 });
model!(/// Window resize request.
    ComputerWindowResizeRequest { width: u32, height: u32 });
model!(/// Screen create or resize request.
ComputerCreateScreenRequest {
    #[serde(default, skip_serializing_if = "is_zero")] width: u32,
    #[serde(default, skip_serializing_if = "is_zero")] height: u32
});
model!(/// Template creation request.
    TemplateCreateRequest { name: String, dockerfile: String, #[serde(default, skip_serializing_if = "Option::is_none")] base: Option<String> });
model!(/// Non-secret S3 disk configuration.
    DiskConfig { bucket: String, endpoint: String, #[serde(default, skip_serializing_if = "Option::is_none")] region: Option<String>, #[serde(default, skip_serializing_if = "std::ops::Not::not")] use_path_style: bool });
model!(/// Write-only disk credentials.
    DiskCredentials { access_key: String, secret_key: String });
model!(/// Disk registration request.
    DiskCreateRequest { name: String, kind: DiskKind, config: DiskConfig, credentials: DiskCredentials });
model!(/// Overlay network creation request.
    NetworkCreateRequest { name: String });

model!(/// Sandbox size preset.
    Shape { id: String, vcpu: u32, #[serde(rename = "mem_mib")] memory_mib: u64, default_disk_mib: i64, #[serde(default)] cpu_quota_pct: u32 });
model!(/// Built-in root filesystem entry.
    RootFsEntry { name: String, description: Option<String>, #[serde(default)] deprecated: bool, successor: Option<String> });
model!(/// Built-in root filesystem catalog.
    RootFsData { #[serde(rename = "rootfs")] root_file_systems: Vec<String>, default: String, #[serde(default)] entries: Vec<RootFsEntry> });
model!(/// Public worker-host projection.
    HostPublic { id: String, status: HostStatus, #[serde(rename = "free_mib")] free_memory_mib: i64, #[serde(rename = "vm_count")] sandbox_count: u64, #[serde(default, rename = "rootfses")] root_file_systems: Vec<String> });
model!(/// Sandbox creation result.
    CreateSandboxResponse { id: String, status: SandboxStatus, name: Option<String>, #[serde(rename = "ip")] ip_address: String, shape: String, rootfs: Option<String>, vcpu: u32, #[serde(rename = "mem_mib")] memory_mib: u64, disk_mib: i64, #[serde(rename = "spawn_ms")] spawn_milliseconds: f64, #[serde(default, rename = "egress")] egress_rules: Vec<String>, bandwidth_quota_bytes: i64, #[serde(default)] ingress_url_template: String });
model!(/// Full sandbox projection.
Sandbox {
    id: String, status: SandboxStatus, #[serde(rename = "ip")] ip_address: Option<String>, vcpu: u32,
        #[serde(rename = "mem_mib")] memory_mib: u64, disk_mib: i64, created_at: Option<DateTime<Utc>>, #[serde(default)] ingress_enabled: bool,
    #[serde(default)] ingress_url_template: String, name: Option<String>, running_at: Option<DateTime<Utc>>,
    destroyed_at: Option<DateTime<Utc>>, #[serde(default, rename = "spawn_ms")] spawn_milliseconds: f64,
    #[serde(default)] shape: String, rootfs: Option<String>, #[serde(default)] region: String,
        #[serde(default, rename = "egress")] egress_rules: Vec<String>, #[serde(default, rename = "envs")] environment_variables: Vec<String>,
    #[serde(default, rename = "ssh_pubkeys")] ssh_public_keys: Vec<String>, #[serde(default)] created_by: String,
    #[serde(default)] bandwidth_ingress_bytes: i64, paused_at: Option<DateTime<Utc>>, last_resumed_at: Option<DateTime<Utc>>,
    forked_from: Option<String>, auto_pause_after_seconds: Option<u64>
});

/// Plaintext delegated token returned only when created or rotated.
#[derive(Clone, Debug, Deserialize)]
pub struct SandboxAccessTokenCreateResponse {
    /// Delegated credential. Store it securely; it cannot be read again.
    pub token: String,
    /// Whether the token is enabled.
    pub enabled: bool,
    /// Time the token was first created.
    pub created_at: DateTime<Utc>,
    /// Time of the most recent rotation, if any.
    pub rotated_at: Option<DateTime<Utc>>,
}

/// Delegated token state without plaintext credential material.
#[derive(Clone, Debug, Deserialize)]
pub struct SandboxAccessTokenMetadata {
    /// Whether a delegated token is enabled.
    pub enabled: bool,
    /// Redacted token hint, when one exists.
    pub token_hint: Option<String>,
    /// Time the token was first created, when one exists.
    pub created_at: Option<DateTime<Utc>>,
    /// Time of the most recent rotation, if any.
    pub rotated_at: Option<DateTime<Utc>>,
}
model!(/// Buffered command result.
    CommandResult { #[serde(rename = "stdout")] standard_output: String, #[serde(rename = "stderr")] standard_error: String, exit_code: i32, #[serde(default, rename = "error")] error_message: String });
model!(/// Buffered command response.
    RunCommandResponse { result: CommandResult, #[serde(rename = "exec_ms")] execution_milliseconds: f64 });
model!(/// Raw command stream frame.
    CommandStreamFrame { #[serde(default, rename = "stdout")] standard_output: String, #[serde(default, rename = "stderr")] standard_error: String, exit_code: Option<i32>, #[serde(default, rename = "error")] error_message: String, #[serde(default, rename = "hb")] heartbeat: bool });
model!(/// Decoded command stream event.
    CommandStreamEvent { #[serde(rename = "type")] kind: ExecStreamEventType, #[serde(default)] data: String, exit_code: Option<i32>, #[serde(default)] message: String });
model!(/// Retained process-output bounds.
    ManagedProcessOutputWindow { oldest_seq: i64, newest_seq: i64, bytes: i64 });
model!(/// Foreground command in a PTY.
    ManagedProcessForeground { #[serde(default, rename = "pid")] pid: i32, #[serde(rename = "cmd")] command: String, #[serde(default, rename = "args")] arguments: Vec<String> });
model!(/// Persistent managed process.
    ManagedProcess { process_id: String, kind: ManagedProcessKind, #[serde(rename = "pid")] pid: i32, state: ManagedProcessState, leader_exited: bool, tree_exited: bool, created_at: Option<DateTime<Utc>>, finished_at: Option<DateTime<Utc>>, exit_code: Option<i32>, signal: Option<String>, #[serde(default, rename = "cmd")] command: String, #[serde(default, rename = "args")] arguments: Vec<String>, #[serde(default, rename = "cwd")] working_directory: String, foreground: Option<ManagedProcessForeground>, output: ManagedProcessOutputWindow });
model!(/// Raw process connection frame.
    ManagedProcessConnectFrame { #[serde(rename = "type")] kind: ManagedProcessConnectEventType, #[serde(default, rename = "seq")] sequence: i64, #[serde(default)] stream: ManagedProcessStream, #[serde(default)] data_base64: String, exit_code: Option<i32>, signal: Option<String>, #[serde(default, rename = "error")] error_message: String, #[serde(default)] oldest_available_seq: i64 });
/// Decoded process connection event.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ManagedProcessConnectEvent {
    /// Event kind.
    pub kind: ManagedProcessConnectEventType,
    /// Monotonic output sequence.
    pub sequence: i64,
    /// Process output stream that produced the data.
    pub stream: ManagedProcessStream,
    /// Decoded binary event payload.
    pub data: Vec<u8>,
    /// Exit code for an exit event.
    pub exit_code: Option<i32>,
    /// Terminating signal, when present.
    pub signal: Option<String>,
    /// Error message for an error event.
    pub message: String,
    /// Oldest sequence still retained by the server.
    pub oldest_available_sequence: i64,
}
model!(/// Screen dimensions.
    ComputerScreenGeometry { width: u32, height: u32 });
model!(/// Desktop clipboard contents.
    ComputerClipboard { text: String });
model!(/// Visible desktop window.
    ComputerWindow { id: String, #[serde(default)] title: String });
model!(/// Window position and dimensions.
    ComputerWindowGeometry { id: String, x: i32, y: i32, width: u32, height: u32, screen: i32 });
model!(/// Configured desktop screen.
    ComputerScreen { screen_id: ComputerScreenId, display: String, width: u32, height: u32, vnc_port: u16, novnc_port: u16 });
model!(/// Temporary noVNC connection.
    ComputerScreenConnection { screen_id: ComputerScreenId, port: u16, path: String, token: String, expires_at: Option<DateTime<Utc>>, #[serde(default)] url: String });
model!(/// Sandbox egress allowlist.
    EgressView { id: String, #[serde(rename = "egress")] rules: Vec<String> });
model!(/// Bandwidth quota and counters.
    BandwidthView { id: String, quota_bytes: i64, used_bytes: i64, ingress_bytes: i64, remaining_bytes: i64, capped: bool });
model!(/// Sandbox resize result.
    ResizeSandboxResponse { id: String, disk_mib: i64 });
model!(/// Identity sandbox statistics.
    WhoAmIStats { running: u64, paused: u64, other: u64, total: u64 });
model!(/// Authenticated identity.
    WhoAmI { user_id: String, stats: WhoAmIStats });
model!(/// Custom root filesystem template.
    Template { id: String, name: String, base: String, status: TemplateStatus, ext4_size_bytes: i64, created_at: Option<DateTime<Utc>>, built_at: Option<DateTime<Utc>>, #[serde(default)] dockerfile: String });
/// One template build-log event. Unknown fields are retained in `extra`.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct TemplateLogEvent {
    /// Event timestamp.
    pub ts: Option<DateTime<Utc>>,
    #[serde(default)]
    /// Log severity.
    pub level: String,
    #[serde(default)]
    /// Build-log line.
    pub line: String,
    #[serde(default)]
    /// Build attempt number.
    pub attempt: u32,
    #[serde(default, rename = "final")]
    /// Whether this is the terminal event for the build.
    pub final_: bool,
    #[serde(default)]
    /// Template status reported by this event.
    pub status: String,
    #[serde(flatten)]
    /// Unknown fields retained for forward compatibility.
    pub extra: HashMap<String, Value>,
}
model!(/// Registered persistent disk.
    Disk { id: String, name: String, kind: DiskKind, config: DiskConfig, created_at: Option<DateTime<Utc>> });
model!(/// Disk attached to a sandbox.
    SandboxDisk { disk_id: String, name: String, kind: DiskKind, config: DiskConfig, mount_path: String, #[serde(default)] sub_path: String, mount_status: DiskMountStatus, #[serde(default)] mount_error: String });
model!(/// Disk deletion result.
    DiskDeletedResponse { deleted: bool });
model!(/// Disk detachment result.
    DiskDetachedResponse { detached: bool });
model!(/// Overlay-network member.
    NetworkMember { sandbox_id: String, status: String, #[serde(default, rename = "ip")] ip_address: String, #[serde(default)] name: String });
model!(/// Overlay network.
    Network { id: String, name: String, created_at: Option<DateTime<Utc>>, #[serde(default)] member_count: u64, #[serde(default)] members: Vec<NetworkMember> });
model!(/// Control-plane liveness response.
    Health { up: bool });
model!(/// Control-plane readiness response.
    Readiness { ready: bool, #[serde(default)] reason: String, #[serde(default)] scheduler_last_ok_ms_ago: i64 });

/// Identifies a disk attachment.
#[derive(Clone, Debug, Default)]
pub struct AttachDiskOptions {
    /// Registered disk identifier.
    pub disk_id: String,
    /// Absolute mount path inside the sandbox.
    pub mount_path: String,
    /// Optional path within the registered disk.
    pub sub_path: Option<String>,
}
/// Identifies a disk detachment.
#[derive(Clone, Debug, Default)]
pub struct DetachDiskOptions {
    /// Registered disk identifier.
    pub disk_id: String,
    /// Mount path currently used inside the sandbox.
    pub mount_path: String,
}

fn is_zero<T: Default + PartialEq>(value: &T) -> bool {
    value == &T::default()
}

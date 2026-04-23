use std::collections::BTreeMap;
use std::fs::{File, OpenOptions, create_dir_all, remove_file};
use std::hash::{DefaultHasher, Hash, Hasher};
use std::net::SocketAddr;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};

use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use tokio::task::JoinHandle;
use tonic::transport::{Channel, Endpoint};

use crate::rpc::controller_service_client::ControllerServiceClient;
use crate::rpc::{
    AttachSpaceRequest, CreateLanceDbTableRequest, DeleteLanceDbRequest,
    DeleteSqliteFtsDocumentRequest, DetachSpaceRequest, DisableBackendRequest,
    DropLanceDbTableRequest, EnableLanceDbRequest, EnableSqliteRequest,
    EnsureSqliteFtsIndexRequest, ExecuteSqliteBatchItem, ExecuteSqliteBatchRequest,
    ExecuteSqliteScriptRequest, GetStatusRequest, LanceDbColumnDef as ProtoLanceDbColumnDef,
    LanceDbColumnType as ProtoLanceDbColumnType, LanceDbInputFormat as ProtoLanceDbInputFormat,
    LanceDbOutputFormat as ProtoLanceDbOutputFormat, ListClientsRequest, ListSpacesRequest,
    ListSqliteCustomWordsRequest, QuerySqliteJsonRequest, QuerySqliteStreamChunkRequest,
    QuerySqliteStreamCloseRequest, QuerySqliteStreamRequest, QuerySqliteStreamWaitMetricsRequest,
    RebuildSqliteFtsIndexRequest, RegisterClientRequest, RemoveSqliteCustomWordRequest,
    RenewClientLeaseRequest, SearchLanceDbRequest, SearchSqliteFtsRequest,
    SqliteTokenizerMode as ProtoSqliteTokenizerMode, SqliteValue, TokenizeSqliteTextRequest,
    UnregisterClientRequest, UpsertLanceDbRequest, UpsertSqliteCustomWordRequest,
    UpsertSqliteFtsDocumentRequest,
};
use crate::types::{
    BoxError, ClientRegistration, ControllerLanceDbColumnDef, ControllerLanceDbColumnType,
    ControllerLanceDbCreateTableResult, ControllerLanceDbDeleteResult,
    ControllerLanceDbDropTableResult, ControllerLanceDbEnableRequest, ControllerLanceDbInputFormat,
    ControllerLanceDbOutputFormat, ControllerLanceDbSearchResult, ControllerLanceDbUpsertResult,
    ControllerProcessMode, ControllerServerConfig, ControllerSqliteCustomWordEntry,
    ControllerSqliteDictionaryMutationResult, ControllerSqliteEnableRequest,
    ControllerSqliteEnsureFtsIndexResult, ControllerSqliteExecuteBatchResult,
    ControllerSqliteExecuteResult, ControllerSqliteFtsMutationResult,
    ControllerSqliteListCustomWordsResult, ControllerSqliteQueryResult,
    ControllerSqliteQueryStreamMetrics, ControllerSqliteQueryStreamOpenResult,
    ControllerSqliteRebuildFtsIndexResult, ControllerSqliteSearchFtsHit,
    ControllerSqliteSearchFtsResult, ControllerSqliteTokenizeResult, ControllerSqliteTokenizerMode,
    ControllerSqliteValue, ControllerStatusSnapshot, SpaceKind, SpaceRegistration, SpaceSnapshot,
    VldbControllerError,
};

// pub type BoxError defined in types.rs, imported above
// 控制器客户端代理使用的共享盒装错误类型。

/// Parameter-driven client configuration used to discover or spawn one controller endpoint.
/// 用于发现或唤起某个控制器端点的参数驱动客户端配置。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ControllerClientConfig {
    /// Target controller endpoint URL or bare `HOST:PORT` bind address.
    /// 目标控制器端点 URL 或裸 `HOST:PORT` 绑定地址。
    pub endpoint: String,
    /// Whether the client may spawn the controller automatically when the endpoint is unavailable.
    /// 当端点不可用时客户端是否允许自动唤起控制器。
    pub auto_spawn: bool,
    /// Optional controller executable path; falls back to `vldb-controller` from PATH.
    /// 可选的控制器可执行文件路径；缺失时回退到 PATH 中的 `vldb-controller`。
    pub spawn_executable: Option<String>,
    /// Process mode used when the client auto-spawns one controller instance.
    /// 客户端自动唤起控制器实例时使用的进程模式。
    pub spawn_process_mode: ControllerProcessMode,
    /// Minimum uptime passed to one auto-spawned controller.
    /// 传递给自动唤起控制器的最小存活时长。
    pub minimum_uptime_secs: u64,
    /// Idle timeout passed to one auto-spawned controller.
    /// 传递给自动唤起控制器的空闲超时时长。
    pub idle_timeout_secs: u64,
    /// Default lease TTL passed to one auto-spawned controller.
    /// 传递给自动唤起控制器的默认租约 TTL。
    pub default_lease_ttl_secs: u64,
    /// Per-attempt transport connect timeout.
    /// 每次传输连接尝试的超时时长。
    pub connect_timeout_secs: u64,
    /// Total wait timeout used after spawning one controller process.
    /// 唤起控制器进程后的总体等待超时时长。
    pub startup_timeout_secs: u64,
    /// Retry interval used while waiting for one spawned controller to become ready.
    /// 等待唤起的控制器就绪时使用的重试间隔。
    pub startup_retry_interval_ms: u64,
    /// Background lease renewal interval.
    /// 后台租约续约间隔。
    pub lease_renew_interval_secs: u64,
}

impl Default for ControllerClientConfig {
    /// Build the default client configuration targeting the shared controller endpoint.
    /// 构建面向共享控制器端点的默认客户端配置。
    fn default() -> Self {
        let defaults = ControllerServerConfig::default();
        Self {
            endpoint: format!("http://{}", defaults.bind_addr),
            auto_spawn: true,
            spawn_executable: None,
            spawn_process_mode: ControllerProcessMode::Managed,
            minimum_uptime_secs: defaults.runtime.minimum_uptime_secs,
            idle_timeout_secs: defaults.runtime.idle_timeout_secs,
            default_lease_ttl_secs: defaults.runtime.default_lease_ttl_secs,
            connect_timeout_secs: 5,
            startup_timeout_secs: 15,
            startup_retry_interval_ms: 250,
            lease_renew_interval_secs: 30,
        }
    }
}

impl ControllerClientConfig {
    /// Return one normalized HTTP endpoint URL accepted by tonic.
    /// 返回一个可被 tonic 接受的标准化 HTTP 端点 URL。
    pub fn endpoint_url(&self) -> Result<String, BoxError> {
        let trimmed = self.endpoint.trim();
        if trimmed.starts_with("http://") || trimmed.starts_with("https://") {
            normalize_http_endpoint(trimmed)
        } else if let Some(port) = trimmed.strip_prefix(':') {
            let connect_addr = normalize_socket_addr("127.0.0.1", port, trimmed)?;
            Ok(format!("http://{connect_addr}"))
        } else if trimmed.chars().all(|value| value.is_ascii_digit()) {
            let connect_addr = normalize_socket_addr("127.0.0.1", trimmed, trimmed)?;
            Ok(format!("http://{connect_addr}"))
        } else {
            normalize_http_endpoint(&format!("http://{trimmed}"))
        }
    }

    /// Return one bind address suitable for the controller startup parameters.
    /// 返回一个适合控制器启动参数使用的绑定地址。
    pub fn bind_addr(&self) -> Result<String, BoxError> {
        let trimmed = self.endpoint.trim();
        if let Some(port) = trimmed.strip_prefix(':') {
            Ok(normalize_socket_addr("0.0.0.0", port, trimmed)?.to_string())
        } else if trimmed.chars().all(|value| value.is_ascii_digit()) {
            Ok(normalize_socket_addr("127.0.0.1", trimmed, trimmed)?.to_string())
        } else {
            normalize_bind_endpoint(trimmed)
        }
    }

    /// Return the executable path or command name used for automatic spawning.
    /// 返回用于自动唤起的可执行文件路径或命令名。
    pub fn spawn_executable(&self) -> &str {
        self.spawn_executable
            .as_deref()
            .unwrap_or("vldb-controller")
    }
}

/// One long-lived controller client proxy used by hosts.
/// 宿主使用的一个长生命周期控制器客户端代理。
#[derive(Clone)]
pub struct ControllerClient {
    inner: Arc<ControllerClientInner>,
}

/// Shared controller client inner state.
/// 共享的控制器客户端内部状态。
struct ControllerClientInner {
    config: ControllerClientConfig,
    registration: ClientRegistration,
    renew_state: Mutex<RenewTaskState>,
    session_state: Mutex<ClientSessionState>,
    session_init_lock: tokio::sync::Mutex<()>,
}

/// Background renew-task state.
/// 后台续约任务状态。
struct RenewTaskState {
    shutdown_sender: Option<tokio::sync::oneshot::Sender<()>>,
    join_handle: Option<JoinHandle<()>>,
}

/// Stable space-and-binding key used by the SDK desired-state cache.
/// SDK 期望状态缓存使用的稳定空间与绑定键。
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct BindingKey {
    space_id: String,
    binding_id: String,
}

/// Desired controller session state cached for automatic recovery and replay.
/// 为自动恢复与重放缓存的控制器会话期望状态。
#[derive(Debug, Default)]
struct ClientSessionState {
    client_session_id: Option<String>,
    session_needs_replay: bool,
    attached_spaces: BTreeMap<String, SpaceRegistration>,
    sqlite_bindings: BTreeMap<BindingKey, ControllerSqliteEnableRequest>,
    lancedb_bindings: BTreeMap<BindingKey, ControllerLanceDbEnableRequest>,
}

/// Desired-state snapshot tuple reused during one session recovery replay.
/// 会话恢复重放过程中复用的期望状态快照元组。
type DesiredReplayState = (
    BTreeMap<String, SpaceRegistration>,
    BTreeMap<BindingKey, ControllerSqliteEnableRequest>,
    BTreeMap<BindingKey, ControllerLanceDbEnableRequest>,
);

/// Local startup lock held while one host coordinates shared controller spawning.
/// 一个宿主协调共享控制器启动时持有的本地启动锁。
struct StartupSpawnGuard {
    path: PathBuf,
    _file: File,
}

impl Drop for StartupSpawnGuard {
    /// Remove the startup lock file when the coordinating scope ends.
    /// 在协调作用域结束时移除启动锁文件。
    fn drop(&mut self) {
        let _ = remove_file(&self.path);
    }
}

/// JSON create-table input used by compatibility wrappers.
/// 兼容包装层使用的 JSON 建表输入。
#[derive(Debug, Deserialize)]
struct CreateTableJsonInput {
    table_name: String,
    columns: Vec<CreateTableJsonColumn>,
    #[serde(default)]
    overwrite_if_exists: bool,
}

/// JSON create-table column used by compatibility wrappers.
/// 兼容包装层使用的 JSON 建表列定义。
#[derive(Debug, Deserialize)]
struct CreateTableJsonColumn {
    name: String,
    column_type: String,
    #[serde(default)]
    vector_dim: u32,
    #[serde(default = "default_nullable")]
    nullable: bool,
}

/// JSON upsert input used by compatibility wrappers.
/// 兼容包装层使用的 JSON 写入输入。
#[derive(Debug, Deserialize)]
struct UpsertJsonInput {
    table_name: String,
    input_format: String,
    #[serde(default)]
    key_columns: Vec<String>,
}

/// JSON search input used by compatibility wrappers.
/// 兼容包装层使用的 JSON 检索输入。
#[derive(Debug, Deserialize)]
struct SearchJsonInput {
    table_name: String,
    vector: Vec<f32>,
    #[serde(default = "default_search_limit")]
    limit: u32,
    #[serde(default)]
    filter: String,
    #[serde(default)]
    vector_column: String,
    #[serde(default)]
    output_format: String,
}

/// JSON delete input used by compatibility wrappers.
/// 兼容包装层使用的 JSON 删除输入。
#[derive(Debug, Deserialize)]
struct DeleteJsonInput {
    table_name: String,
    condition: String,
}

/// JSON bytes wrapper accepted by SQLite compatibility request payloads.
/// SQLite 兼容请求负载接受的 JSON 字节包装对象。
#[derive(Debug, Deserialize)]
struct JsonSqliteBytesValue {
    #[serde(default)]
    r#type: String,
    #[serde(default)]
    __type: String,
    base64: String,
}

impl ControllerClient {
    /// Create one controller client proxy without performing network actions yet.
    /// 创建一个控制器客户端代理，但暂不执行网络动作。
    pub fn new(config: ControllerClientConfig, registration: ClientRegistration) -> Self {
        Self {
            inner: Arc::new(ControllerClientInner {
                config,
                registration,
                renew_state: Mutex::new(RenewTaskState {
                    shutdown_sender: None,
                    join_handle: None,
                }),
                session_state: Mutex::new(ClientSessionState::default()),
                session_init_lock: tokio::sync::Mutex::new(()),
            }),
        }
    }

    /// Connect to the controller endpoint, spawn it when permitted, and start lease maintenance.
    /// 连接控制器端点，在允许时自动唤起，并启动租约维护。
    pub async fn connect(&self) -> Result<(), BoxError> {
        let _client = self.connect_client().await?;
        self.ensure_renew_task()?;
        Ok(())
    }

    /// Return one fresh controller status snapshot.
    /// 返回一份最新的控制器状态快照。
    pub async fn get_status(&self) -> Result<ControllerStatusSnapshot, BoxError> {
        let mut client = self.connect_transport_client().await?;
        let response = client.get_status(GetStatusRequest {}).await?.into_inner();
        let status = response
            .status
            .ok_or_else(|| invalid_input("controller status payload is missing"))?;
        Ok(ControllerStatusSnapshot {
            process_mode: map_process_mode(status.process_mode)?,
            bind_addr: status.bind_addr,
            started_at_unix_ms: status.started_at_unix_ms,
            last_request_at_unix_ms: status.last_request_at_unix_ms,
            minimum_uptime_secs: status.minimum_uptime_secs,
            idle_timeout_secs: status.idle_timeout_secs,
            default_lease_ttl_secs: status.default_lease_ttl_secs,
            active_clients: status.active_clients as usize,
            attached_spaces: status.attached_spaces as usize,
            inflight_requests: status.inflight_requests as usize,
            shutdown_candidate: status.shutdown_candidate,
        })
    }

    /// Return all currently active client leases.
    /// 返回所有当前活跃的客户端租约。
    pub async fn list_clients(&self) -> Result<Vec<crate::types::ClientLeaseSnapshot>, BoxError> {
        let mut client = self.connect_transport_client().await?;
        let response = client
            .list_clients(ListClientsRequest {})
            .await?
            .into_inner();
        Ok(response
            .clients
            .into_iter()
            .map(map_client_snapshot)
            .collect())
    }

    /// Attach one runtime space through the controller.
    /// 通过控制器附着一个运行时空间。
    pub async fn attach_space(
        &self,
        registration: SpaceRegistration,
    ) -> Result<SpaceSnapshot, BoxError> {
        let desired_registration = registration.clone();
        let mut client = self.connect_client().await?;
        let response = client
            .attach_space(AttachSpaceRequest {
                client_session_id: self.current_client_session_id()?,
                space_id: registration.space_id,
                space_label: registration.space_label,
                space_kind: map_space_kind(registration.space_kind) as i32,
                space_root: registration.space_root,
            })
            .await?
            .into_inner();
        let space = response
            .space
            .ok_or_else(|| invalid_input("attach_space response is missing the space snapshot"))?;
        self.remember_attached_space(desired_registration)?;
        map_space_snapshot(space)
    }

    /// Detach one runtime space from the current client.
    /// 将一个运行时空间从当前客户端解除附着。
    pub async fn detach_space(&self, space_id: impl Into<String>) -> Result<bool, BoxError> {
        let space_id = space_id.into();
        let mut client = self.connect_client().await?;
        let response = client
            .detach_space(DetachSpaceRequest {
                client_session_id: self.current_client_session_id()?,
                space_id: space_id.clone(),
            })
            .await?
            .into_inner();
        if response.detached {
            self.forget_attached_space(&space_id)?;
        }
        Ok(response.detached)
    }

    /// Return snapshots for all spaces currently known by the controller.
    /// 返回控制器当前已知的全部空间快照。
    pub async fn list_spaces(&self) -> Result<Vec<SpaceSnapshot>, BoxError> {
        let mut client = self.connect_transport_client().await?;
        let response = client.list_spaces(ListSpacesRequest {}).await?.into_inner();
        response
            .spaces
            .into_iter()
            .map(map_space_snapshot)
            .collect()
    }

    /// Enable one SQLite backend through the controller.
    /// 通过控制器启用一个 SQLite 后端。
    pub async fn enable_sqlite(
        &self,
        request: ControllerSqliteEnableRequest,
    ) -> Result<(), BoxError> {
        let desired_request = request.clone();
        let mut client = self.connect_client().await?;
        client
            .enable_sqlite(EnableSqliteRequest {
                client_session_id: self.current_client_session_id()?,
                space_id: request.space_id,
                binding_id: request.binding_id,
                db_path: request.db_path,
                connection_pool_size: request.connection_pool_size as u32,
                busy_timeout_ms: request.busy_timeout_ms,
                journal_mode: request.journal_mode,
                synchronous: request.synchronous,
                foreign_keys: request.foreign_keys,
                temp_store: request.temp_store,
                wal_autocheckpoint_pages: request.wal_autocheckpoint_pages,
                cache_size_kib: request.cache_size_kib,
                mmap_size_bytes: request.mmap_size_bytes,
                enforce_db_file_lock: request.enforce_db_file_lock,
                read_only: request.read_only,
                allow_uri_filenames: request.allow_uri_filenames,
                trusted_schema: request.trusted_schema,
                defensive: request.defensive,
            })
            .await?;
        self.remember_sqlite_binding(desired_request)?;
        Ok(())
    }

    /// Disable one SQLite backend through the controller.
    /// 通过控制器关闭一个 SQLite 后端。
    pub async fn disable_sqlite(
        &self,
        space_id: impl Into<String>,
        binding_id: impl Into<String>,
    ) -> Result<bool, BoxError> {
        let space_id = space_id.into();
        let binding_id = binding_id.into();
        let mut client = self.connect_client().await?;
        let response = client
            .disable_sqlite(DisableBackendRequest {
                client_session_id: self.current_client_session_id()?,
                space_id: space_id.clone(),
                binding_id: binding_id.clone(),
            })
            .await?
            .into_inner();
        if response.disabled {
            self.forget_sqlite_binding(&space_id, &binding_id)?;
        }
        Ok(response.disabled)
    }

    /// Execute one SQLite script with typed parameters through the controller data plane.
    /// 通过控制器数据面使用类型化参数执行一条 SQLite 脚本。
    pub async fn execute_sqlite_script_typed(
        &self,
        space_id: impl Into<String>,
        binding_id: impl Into<String>,
        sql: impl Into<String>,
        params: Vec<ControllerSqliteValue>,
    ) -> Result<ControllerSqliteExecuteResult, BoxError> {
        let mut client = self.connect_client().await?;
        let response = client
            .execute_sqlite_script(ExecuteSqliteScriptRequest {
                client_session_id: self.current_client_session_id()?,
                space_id: space_id.into(),
                binding_id: binding_id.into(),
                sql: sql.into(),
                params: params.into_iter().map(map_sqlite_value).collect(),
            })
            .await?
            .into_inner();
        Ok(ControllerSqliteExecuteResult {
            success: response.success,
            message: response.message,
            rows_changed: response.rows_changed,
            last_insert_rowid: response.last_insert_rowid,
        })
    }

    /// Execute one SQLite batch with typed parameter groups through the controller data plane.
    /// 通过控制器数据面使用类型化参数组执行一组 SQLite 批量语句。
    pub async fn execute_sqlite_batch_typed(
        &self,
        space_id: impl Into<String>,
        binding_id: impl Into<String>,
        sql: impl Into<String>,
        items: Vec<Vec<ControllerSqliteValue>>,
    ) -> Result<ControllerSqliteExecuteBatchResult, BoxError> {
        let mut client = self.connect_client().await?;
        let response = client
            .execute_sqlite_batch(ExecuteSqliteBatchRequest {
                client_session_id: self.current_client_session_id()?,
                space_id: space_id.into(),
                binding_id: binding_id.into(),
                sql: sql.into(),
                items: items
                    .into_iter()
                    .map(|params| ExecuteSqliteBatchItem {
                        params: params.into_iter().map(map_sqlite_value).collect(),
                    })
                    .collect(),
            })
            .await?
            .into_inner();
        Ok(ControllerSqliteExecuteBatchResult {
            success: response.success,
            message: response.message,
            rows_changed: response.rows_changed,
            last_insert_rowid: response.last_insert_rowid,
            statements_executed: response.statements_executed,
        })
    }

    /// Execute one SQLite JSON query with typed parameters through the controller data plane.
    /// 通过控制器数据面使用类型化参数执行一条 SQLite JSON 查询。
    pub async fn query_sqlite_json_typed(
        &self,
        space_id: impl Into<String>,
        binding_id: impl Into<String>,
        sql: impl Into<String>,
        params: Vec<ControllerSqliteValue>,
    ) -> Result<ControllerSqliteQueryResult, BoxError> {
        let mut client = self.connect_client().await?;
        let response = client
            .query_sqlite_json(QuerySqliteJsonRequest {
                client_session_id: self.current_client_session_id()?,
                space_id: space_id.into(),
                binding_id: binding_id.into(),
                sql: sql.into(),
                params: params.into_iter().map(map_sqlite_value).collect(),
            })
            .await?
            .into_inner();
        Ok(ControllerSqliteQueryResult {
            json_data: response.json_data,
            row_count: response.row_count,
        })
    }

    /// Execute one SQLite streaming query with typed parameters through the controller data plane.
    /// 通过控制器数据面使用类型化参数执行一条 SQLite 流式查询。
    pub async fn open_sqlite_query_stream_typed(
        &self,
        space_id: impl Into<String>,
        binding_id: impl Into<String>,
        sql: impl Into<String>,
        params: Vec<ControllerSqliteValue>,
        target_chunk_size: Option<u64>,
    ) -> Result<ControllerSqliteQueryStreamOpenResult, BoxError> {
        let mut client = self.connect_client().await?;
        let response = client
            .query_sqlite_stream(QuerySqliteStreamRequest {
                client_session_id: self.current_client_session_id()?,
                space_id: space_id.into(),
                binding_id: binding_id.into(),
                sql: sql.into(),
                params: params.into_iter().map(map_sqlite_value).collect(),
                target_chunk_size: target_chunk_size.unwrap_or(0),
            })
            .await?
            .into_inner();
        Ok(ControllerSqliteQueryStreamOpenResult {
            stream_id: response.stream_id,
            metrics_ready: response.metrics_ready,
        })
    }

    /// Wait for terminal metrics of one controller-managed SQLite query stream.
    /// 等待一条由控制器管理的 SQLite 查询流终态指标。
    pub async fn wait_sqlite_query_stream_metrics(
        &self,
        stream_id: u64,
    ) -> Result<ControllerSqliteQueryStreamMetrics, BoxError> {
        let mut client = self.connect_client().await?;
        let response = client
            .query_sqlite_stream_wait_metrics(QuerySqliteStreamWaitMetricsRequest {
                client_session_id: self.current_client_session_id()?,
                stream_id,
            })
            .await?
            .into_inner();
        Ok(ControllerSqliteQueryStreamMetrics {
            row_count: response.row_count,
            chunk_count: response.chunk_count,
            total_bytes: response.total_bytes,
        })
    }

    /// Read one chunk from one controller-managed SQLite query stream.
    /// 从一条由控制器管理的 SQLite 查询流读取一个分块。
    pub async fn read_sqlite_query_stream_chunk(
        &self,
        stream_id: u64,
        index: u64,
    ) -> Result<Vec<u8>, BoxError> {
        let mut client = self.connect_client().await?;
        let response = client
            .query_sqlite_stream_chunk(QuerySqliteStreamChunkRequest {
                client_session_id: self.current_client_session_id()?,
                stream_id,
                index,
            })
            .await?
            .into_inner();
        Ok(response.chunk)
    }

    /// Close one controller-managed SQLite query stream and release its spool resources.
    /// 关闭一条由控制器管理的 SQLite 查询流并释放其暂存资源。
    pub async fn close_sqlite_query_stream(&self, stream_id: u64) -> Result<bool, BoxError> {
        let mut client = self.connect_client().await?;
        let response = client
            .query_sqlite_stream_close(QuerySqliteStreamCloseRequest {
                client_session_id: self.current_client_session_id()?,
                stream_id,
            })
            .await?
            .into_inner();
        Ok(response.closed)
    }

    /// Tokenize text through the controller-managed SQLite backend.
    /// 通过控制器管理的 SQLite 后端对文本执行分词。
    pub async fn tokenize_sqlite_text(
        &self,
        space_id: impl Into<String>,
        binding_id: impl Into<String>,
        tokenizer_mode: ControllerSqliteTokenizerMode,
        text: impl Into<String>,
        search_mode: bool,
    ) -> Result<ControllerSqliteTokenizeResult, BoxError> {
        let mut client = self.connect_client().await?;
        let response = client
            .tokenize_sqlite_text(TokenizeSqliteTextRequest {
                client_session_id: self.current_client_session_id()?,
                space_id: space_id.into(),
                binding_id: binding_id.into(),
                tokenizer_mode: map_sqlite_tokenizer_mode(tokenizer_mode) as i32,
                text: text.into(),
                search_mode,
            })
            .await?
            .into_inner();
        Ok(ControllerSqliteTokenizeResult {
            tokenizer_mode: response.tokenizer_mode,
            normalized_text: response.normalized_text,
            tokens: response.tokens,
            fts_query: response.fts_query,
        })
    }

    /// Return all SQLite custom words from the target space backend.
    /// 返回目标空间后端中的全部 SQLite 自定义词。
    pub async fn list_sqlite_custom_words(
        &self,
        space_id: impl Into<String>,
        binding_id: impl Into<String>,
    ) -> Result<ControllerSqliteListCustomWordsResult, BoxError> {
        let mut client = self.connect_client().await?;
        let response = client
            .list_sqlite_custom_words(ListSqliteCustomWordsRequest {
                client_session_id: self.current_client_session_id()?,
                space_id: space_id.into(),
                binding_id: binding_id.into(),
            })
            .await?
            .into_inner();
        Ok(ControllerSqliteListCustomWordsResult {
            success: response.success,
            message: response.message,
            words: response
                .words
                .into_iter()
                .map(|item| ControllerSqliteCustomWordEntry {
                    word: item.word,
                    weight: item.weight as usize,
                })
                .collect(),
        })
    }

    /// Insert or update one SQLite custom word through the controller backend.
    /// 通过控制器后端插入或更新一条 SQLite 自定义词。
    pub async fn upsert_sqlite_custom_word(
        &self,
        space_id: impl Into<String>,
        binding_id: impl Into<String>,
        word: impl Into<String>,
        weight: u32,
    ) -> Result<ControllerSqliteDictionaryMutationResult, BoxError> {
        let mut client = self.connect_client().await?;
        let response = client
            .upsert_sqlite_custom_word(UpsertSqliteCustomWordRequest {
                client_session_id: self.current_client_session_id()?,
                space_id: space_id.into(),
                binding_id: binding_id.into(),
                word: word.into(),
                weight,
            })
            .await?
            .into_inner();
        Ok(ControllerSqliteDictionaryMutationResult {
            success: response.success,
            message: response.message,
            affected_rows: response.affected_rows,
        })
    }

    /// Remove one SQLite custom word through the controller backend.
    /// 通过控制器后端删除一条 SQLite 自定义词。
    pub async fn remove_sqlite_custom_word(
        &self,
        space_id: impl Into<String>,
        binding_id: impl Into<String>,
        word: impl Into<String>,
    ) -> Result<ControllerSqliteDictionaryMutationResult, BoxError> {
        let mut client = self.connect_client().await?;
        let response = client
            .remove_sqlite_custom_word(RemoveSqliteCustomWordRequest {
                client_session_id: self.current_client_session_id()?,
                space_id: space_id.into(),
                binding_id: binding_id.into(),
                word: word.into(),
            })
            .await?
            .into_inner();
        Ok(ControllerSqliteDictionaryMutationResult {
            success: response.success,
            message: response.message,
            affected_rows: response.affected_rows,
        })
    }

    /// Ensure one SQLite FTS index through the controller backend.
    /// 通过控制器后端确认一条 SQLite FTS 索引。
    pub async fn ensure_sqlite_fts_index(
        &self,
        space_id: impl Into<String>,
        binding_id: impl Into<String>,
        index_name: impl Into<String>,
        tokenizer_mode: ControllerSqliteTokenizerMode,
    ) -> Result<ControllerSqliteEnsureFtsIndexResult, BoxError> {
        let mut client = self.connect_client().await?;
        let response = client
            .ensure_sqlite_fts_index(EnsureSqliteFtsIndexRequest {
                client_session_id: self.current_client_session_id()?,
                space_id: space_id.into(),
                binding_id: binding_id.into(),
                index_name: index_name.into(),
                tokenizer_mode: map_sqlite_tokenizer_mode(tokenizer_mode) as i32,
            })
            .await?
            .into_inner();
        Ok(ControllerSqliteEnsureFtsIndexResult {
            success: response.success,
            message: response.message,
            index_name: response.index_name,
            tokenizer_mode: response.tokenizer_mode,
        })
    }

    /// Rebuild one SQLite FTS index through the controller backend.
    /// 通过控制器后端重建一条 SQLite FTS 索引。
    pub async fn rebuild_sqlite_fts_index(
        &self,
        space_id: impl Into<String>,
        binding_id: impl Into<String>,
        index_name: impl Into<String>,
        tokenizer_mode: ControllerSqliteTokenizerMode,
    ) -> Result<ControllerSqliteRebuildFtsIndexResult, BoxError> {
        let mut client = self.connect_client().await?;
        let response = client
            .rebuild_sqlite_fts_index(RebuildSqliteFtsIndexRequest {
                client_session_id: self.current_client_session_id()?,
                space_id: space_id.into(),
                binding_id: binding_id.into(),
                index_name: index_name.into(),
                tokenizer_mode: map_sqlite_tokenizer_mode(tokenizer_mode) as i32,
            })
            .await?
            .into_inner();
        Ok(ControllerSqliteRebuildFtsIndexResult {
            success: response.success,
            message: response.message,
            index_name: response.index_name,
            tokenizer_mode: response.tokenizer_mode,
            reindexed_rows: response.reindexed_rows,
        })
    }

    /// Upsert one SQLite FTS document through the controller backend.
    /// 通过控制器后端写入一条 SQLite FTS 文档。
    #[allow(clippy::too_many_arguments)]
    pub async fn upsert_sqlite_fts_document(
        &self,
        space_id: impl Into<String>,
        binding_id: impl Into<String>,
        index_name: impl Into<String>,
        tokenizer_mode: ControllerSqliteTokenizerMode,
        id: impl Into<String>,
        file_path: impl Into<String>,
        title: impl Into<String>,
        content: impl Into<String>,
    ) -> Result<ControllerSqliteFtsMutationResult, BoxError> {
        let mut client = self.connect_client().await?;
        let response = client
            .upsert_sqlite_fts_document(UpsertSqliteFtsDocumentRequest {
                client_session_id: self.current_client_session_id()?,
                space_id: space_id.into(),
                binding_id: binding_id.into(),
                index_name: index_name.into(),
                tokenizer_mode: map_sqlite_tokenizer_mode(tokenizer_mode) as i32,
                id: id.into(),
                file_path: file_path.into(),
                title: title.into(),
                content: content.into(),
            })
            .await?
            .into_inner();
        Ok(ControllerSqliteFtsMutationResult {
            success: response.success,
            message: response.message,
            affected_rows: response.affected_rows,
            index_name: response.index_name,
        })
    }

    /// Delete one SQLite FTS document through the controller backend.
    /// 通过控制器后端删除一条 SQLite FTS 文档。
    pub async fn delete_sqlite_fts_document(
        &self,
        space_id: impl Into<String>,
        binding_id: impl Into<String>,
        index_name: impl Into<String>,
        id: impl Into<String>,
    ) -> Result<ControllerSqliteFtsMutationResult, BoxError> {
        let mut client = self.connect_client().await?;
        let response = client
            .delete_sqlite_fts_document(DeleteSqliteFtsDocumentRequest {
                client_session_id: self.current_client_session_id()?,
                space_id: space_id.into(),
                binding_id: binding_id.into(),
                index_name: index_name.into(),
                id: id.into(),
            })
            .await?
            .into_inner();
        Ok(ControllerSqliteFtsMutationResult {
            success: response.success,
            message: response.message,
            affected_rows: response.affected_rows,
            index_name: response.index_name,
        })
    }

    /// Search one SQLite FTS index through the controller backend.
    /// 通过控制器后端检索一条 SQLite FTS 索引。
    #[allow(clippy::too_many_arguments)]
    pub async fn search_sqlite_fts(
        &self,
        space_id: impl Into<String>,
        binding_id: impl Into<String>,
        index_name: impl Into<String>,
        tokenizer_mode: ControllerSqliteTokenizerMode,
        query: impl Into<String>,
        limit: u32,
        offset: u32,
    ) -> Result<ControllerSqliteSearchFtsResult, BoxError> {
        let mut client = self.connect_client().await?;
        let response = client
            .search_sqlite_fts(SearchSqliteFtsRequest {
                client_session_id: self.current_client_session_id()?,
                space_id: space_id.into(),
                binding_id: binding_id.into(),
                index_name: index_name.into(),
                tokenizer_mode: map_sqlite_tokenizer_mode(tokenizer_mode) as i32,
                query: query.into(),
                limit,
                offset,
            })
            .await?
            .into_inner();
        Ok(ControllerSqliteSearchFtsResult {
            success: response.success,
            message: response.message,
            index_name: response.index_name,
            tokenizer_mode: response.tokenizer_mode,
            normalized_query: response.normalized_query,
            fts_query: response.fts_query,
            source: response.source,
            query_mode: response.query_mode,
            total: response.total,
            hits: response
                .hits
                .into_iter()
                .map(|hit| ControllerSqliteSearchFtsHit {
                    id: hit.id,
                    file_path: hit.file_path,
                    title: hit.title,
                    title_highlight: hit.title_highlight,
                    content_snippet: hit.content_snippet,
                    score: hit.score,
                    rank: hit.rank,
                    raw_score: hit.raw_score,
                })
                .collect(),
        })
    }

    /// Execute one SQLite script through the compatibility JSON wrapper path.
    /// 通过兼容 JSON 包装路径执行一条 SQLite 脚本。
    pub async fn execute_sqlite_script(
        &self,
        space_id: impl Into<String>,
        binding_id: impl Into<String>,
        sql: impl Into<String>,
        params_json: impl Into<String>,
    ) -> Result<ControllerSqliteExecuteResult, BoxError> {
        self.execute_sqlite_script_typed(
            space_id,
            binding_id,
            sql,
            parse_sqlite_params_json(&params_json.into())?,
        )
        .await
    }

    /// Execute one SQLite batch through the compatibility JSON wrapper path.
    /// 通过兼容 JSON 包装路径执行一组 SQLite 批量语句。
    pub async fn execute_sqlite_batch(
        &self,
        space_id: impl Into<String>,
        binding_id: impl Into<String>,
        sql: impl Into<String>,
        batch_params_json: Vec<String>,
    ) -> Result<ControllerSqliteExecuteBatchResult, BoxError> {
        let items = batch_params_json
            .iter()
            .map(|value| parse_sqlite_params_json(value))
            .collect::<Result<Vec<_>, _>>()?;
        self.execute_sqlite_batch_typed(space_id, binding_id, sql, items)
            .await
    }

    /// Execute one SQLite JSON query through the compatibility JSON wrapper path.
    /// 通过兼容 JSON 包装路径执行一条 SQLite JSON 查询。
    pub async fn query_sqlite_json(
        &self,
        space_id: impl Into<String>,
        binding_id: impl Into<String>,
        sql: impl Into<String>,
        params_json: impl Into<String>,
    ) -> Result<ControllerSqliteQueryResult, BoxError> {
        self.query_sqlite_json_typed(
            space_id,
            binding_id,
            sql,
            parse_sqlite_params_json(&params_json.into())?,
        )
        .await
    }

    /// Execute one SQLite streaming query through the compatibility JSON wrapper path.
    /// 通过兼容 JSON 包装路径执行一条 SQLite 流式查询。
    pub async fn open_sqlite_query_stream(
        &self,
        space_id: impl Into<String>,
        binding_id: impl Into<String>,
        sql: impl Into<String>,
        params_json: impl Into<String>,
        target_chunk_size: Option<u64>,
    ) -> Result<ControllerSqliteQueryStreamOpenResult, BoxError> {
        self.open_sqlite_query_stream_typed(
            space_id,
            binding_id,
            sql,
            parse_sqlite_params_json(&params_json.into())?,
            target_chunk_size,
        )
        .await
    }

    /// Enable one LanceDB backend through the controller.
    /// 通过控制器启用一个 LanceDB 后端。
    pub async fn enable_lancedb(
        &self,
        request: ControllerLanceDbEnableRequest,
    ) -> Result<(), BoxError> {
        let desired_request = request.clone();
        let mut client = self.connect_client().await?;
        client
            .enable_lance_db(EnableLanceDbRequest {
                client_session_id: self.current_client_session_id()?,
                space_id: request.space_id,
                binding_id: request.binding_id,
                default_db_path: request.default_db_path,
                db_root: request.db_root.unwrap_or_default(),
                read_consistency_interval_ms: request.read_consistency_interval_ms.unwrap_or(0),
                max_upsert_payload: request.max_upsert_payload as u64,
                max_search_limit: request.max_search_limit as u64,
                max_concurrent_requests: request.max_concurrent_requests as u64,
            })
            .await?;
        self.remember_lancedb_binding(desired_request)?;
        Ok(())
    }

    /// Disable one LanceDB backend through the controller.
    /// 通过控制器关闭一个 LanceDB 后端。
    pub async fn disable_lancedb(
        &self,
        space_id: impl Into<String>,
        binding_id: impl Into<String>,
    ) -> Result<bool, BoxError> {
        let space_id = space_id.into();
        let binding_id = binding_id.into();
        let mut client = self.connect_client().await?;
        let response = client
            .disable_lance_db(DisableBackendRequest {
                client_session_id: self.current_client_session_id()?,
                space_id: space_id.clone(),
                binding_id: binding_id.clone(),
            })
            .await?
            .into_inner();
        if response.disabled {
            self.forget_lancedb_binding(&space_id, &binding_id)?;
        }
        Ok(response.disabled)
    }

    /// Create one LanceDB table with typed schema metadata.
    /// 使用类型化 schema 元信息创建一张 LanceDB 表。
    pub async fn create_lancedb_table_typed(
        &self,
        space_id: impl Into<String>,
        binding_id: impl Into<String>,
        table_name: impl Into<String>,
        columns: Vec<ControllerLanceDbColumnDef>,
        overwrite_if_exists: bool,
    ) -> Result<ControllerLanceDbCreateTableResult, BoxError> {
        let mut client = self.connect_client().await?;
        let response = client
            .create_lance_db_table(CreateLanceDbTableRequest {
                client_session_id: self.current_client_session_id()?,
                space_id: space_id.into(),
                binding_id: binding_id.into(),
                table_name: table_name.into(),
                columns: columns.into_iter().map(map_lancedb_column_def).collect(),
                overwrite_if_exists,
            })
            .await?
            .into_inner();
        Ok(ControllerLanceDbCreateTableResult {
            message: response.message,
        })
    }

    /// Upsert LanceDB rows with typed metadata and payload.
    /// 使用类型化元信息与载荷写入 LanceDB 行数据。
    pub async fn upsert_lancedb_typed(
        &self,
        space_id: impl Into<String>,
        binding_id: impl Into<String>,
        table_name: impl Into<String>,
        input_format: ControllerLanceDbInputFormat,
        data: Vec<u8>,
        key_columns: Vec<String>,
    ) -> Result<ControllerLanceDbUpsertResult, BoxError> {
        let mut client = self.connect_client().await?;
        let response = client
            .upsert_lance_db(UpsertLanceDbRequest {
                client_session_id: self.current_client_session_id()?,
                space_id: space_id.into(),
                binding_id: binding_id.into(),
                table_name: table_name.into(),
                input_format: map_lancedb_input_format(input_format) as i32,
                data,
                key_columns,
            })
            .await?
            .into_inner();
        Ok(ControllerLanceDbUpsertResult {
            message: response.message,
            version: response.version,
            input_rows: response.input_rows,
            inserted_rows: response.inserted_rows,
            updated_rows: response.updated_rows,
            deleted_rows: response.deleted_rows,
        })
    }

    /// Search LanceDB rows with typed search parameters.
    /// 使用类型化检索参数检索 LanceDB 行数据。
    #[allow(clippy::too_many_arguments)]
    pub async fn search_lancedb_typed(
        &self,
        space_id: impl Into<String>,
        binding_id: impl Into<String>,
        table_name: impl Into<String>,
        vector: Vec<f32>,
        limit: u32,
        filter: impl Into<String>,
        vector_column: impl Into<String>,
        output_format: ControllerLanceDbOutputFormat,
    ) -> Result<ControllerLanceDbSearchResult, BoxError> {
        let mut client = self.connect_client().await?;
        let response = client
            .search_lance_db(SearchLanceDbRequest {
                client_session_id: self.current_client_session_id()?,
                space_id: space_id.into(),
                binding_id: binding_id.into(),
                table_name: table_name.into(),
                vector,
                limit,
                filter: filter.into(),
                vector_column: vector_column.into(),
                output_format: map_lancedb_output_format(output_format) as i32,
            })
            .await?
            .into_inner();
        Ok(ControllerLanceDbSearchResult {
            message: response.message,
            format: response.format,
            rows: response.rows,
            data: response.data,
        })
    }

    /// Delete LanceDB rows with typed predicate parameters.
    /// 使用类型化谓词参数删除 LanceDB 行数据。
    pub async fn delete_lancedb_typed(
        &self,
        space_id: impl Into<String>,
        binding_id: impl Into<String>,
        table_name: impl Into<String>,
        condition: impl Into<String>,
    ) -> Result<ControllerLanceDbDeleteResult, BoxError> {
        let mut client = self.connect_client().await?;
        let response = client
            .delete_lance_db(DeleteLanceDbRequest {
                client_session_id: self.current_client_session_id()?,
                space_id: space_id.into(),
                binding_id: binding_id.into(),
                table_name: table_name.into(),
                condition: condition.into(),
            })
            .await?
            .into_inner();
        Ok(ControllerLanceDbDeleteResult {
            message: response.message,
            version: response.version,
            deleted_rows: response.deleted_rows,
        })
    }

    /// Drop one LanceDB table with a typed request.
    /// 使用类型化请求删除一张 LanceDB 表。
    pub async fn drop_lancedb_table(
        &self,
        space_id: impl Into<String>,
        binding_id: impl Into<String>,
        table_name: impl Into<String>,
    ) -> Result<ControllerLanceDbDropTableResult, BoxError> {
        let mut client = self.connect_client().await?;
        let response = client
            .drop_lance_db_table(DropLanceDbTableRequest {
                client_session_id: self.current_client_session_id()?,
                space_id: space_id.into(),
                binding_id: binding_id.into(),
                table_name: table_name.into(),
            })
            .await?
            .into_inner();
        Ok(ControllerLanceDbDropTableResult {
            message: response.message,
        })
    }

    /// Create one LanceDB table through the compatibility JSON wrapper path.
    /// 通过兼容 JSON 包装路径创建一张 LanceDB 表。
    pub async fn create_lancedb_table(
        &self,
        space_id: impl Into<String>,
        binding_id: impl Into<String>,
        request_json: impl Into<String>,
    ) -> Result<ControllerLanceDbCreateTableResult, BoxError> {
        let input = serde_json::from_str::<CreateTableJsonInput>(&request_json.into()).map_err(
            |error| {
                invalid_input(format!(
                    "failed to parse create_lancedb_table request_json: {error}"
                ))
            },
        )?;
        self.create_lancedb_table_typed(
            space_id,
            binding_id,
            input.table_name,
            input
                .columns
                .into_iter()
                .map(map_create_table_json_column)
                .collect::<Result<Vec<_>, _>>()?,
            input.overwrite_if_exists,
        )
        .await
    }

    /// Upsert LanceDB rows through the compatibility JSON wrapper path.
    /// 通过兼容 JSON 包装路径写入 LanceDB 行数据。
    pub async fn upsert_lancedb(
        &self,
        space_id: impl Into<String>,
        binding_id: impl Into<String>,
        request_json: impl Into<String>,
        data: Vec<u8>,
    ) -> Result<ControllerLanceDbUpsertResult, BoxError> {
        let input =
            serde_json::from_str::<UpsertJsonInput>(&request_json.into()).map_err(|error| {
                invalid_input(format!(
                    "failed to parse upsert_lancedb request_json: {error}"
                ))
            })?;
        self.upsert_lancedb_typed(
            space_id,
            binding_id,
            input.table_name,
            parse_lancedb_input_format(&input.input_format)?,
            data,
            input.key_columns,
        )
        .await
    }

    /// Search LanceDB rows through the compatibility JSON wrapper path.
    /// 通过兼容 JSON 包装路径检索 LanceDB 行数据。
    pub async fn search_lancedb(
        &self,
        space_id: impl Into<String>,
        binding_id: impl Into<String>,
        request_json: impl Into<String>,
    ) -> Result<ControllerLanceDbSearchResult, BoxError> {
        let input =
            serde_json::from_str::<SearchJsonInput>(&request_json.into()).map_err(|error| {
                invalid_input(format!(
                    "failed to parse search_lancedb request_json: {error}"
                ))
            })?;
        self.search_lancedb_typed(
            space_id,
            binding_id,
            input.table_name,
            input.vector,
            input.limit,
            input.filter,
            input.vector_column,
            parse_lancedb_output_format(&input.output_format)?,
        )
        .await
    }

    /// Delete LanceDB rows through the compatibility JSON wrapper path.
    /// 通过兼容 JSON 包装路径删除 LanceDB 行数据。
    pub async fn delete_lancedb(
        &self,
        space_id: impl Into<String>,
        binding_id: impl Into<String>,
        request_json: impl Into<String>,
    ) -> Result<ControllerLanceDbDeleteResult, BoxError> {
        let input =
            serde_json::from_str::<DeleteJsonInput>(&request_json.into()).map_err(|error| {
                invalid_input(format!(
                    "failed to parse delete_lancedb request_json: {error}"
                ))
            })?;
        self.delete_lancedb_typed(space_id, binding_id, input.table_name, input.condition)
            .await
    }

    /// Stop background lease maintenance and unregister the current client explicitly.
    /// 停止后台租约维护，并显式注销当前客户端。
    pub async fn shutdown(&self) -> Result<(), BoxError> {
        self.stop_renew_task();
        if let Some(client_session_id) = self.current_client_session_id_opt()?
            && let Ok(mut client) = self.try_connect().await
        {
            let _ = client
                .unregister_client(UnregisterClientRequest { client_session_id })
                .await;
        }
        self.clear_desired_session_state()?;
        Ok(())
    }

    /// Ensure the target controller is reachable, spawning one when allowed.
    /// 确保目标控制器可达，并在允许时自动唤起。
    async fn ensure_ready(&self) -> Result<(), BoxError> {
        if self.try_connect().await.is_ok() {
            return Ok(());
        }

        if !self.inner.config.auto_spawn {
            return Err(invalid_input(format!(
                "controller endpoint `{}` is unavailable and auto_spawn is disabled",
                self.inner.config.endpoint_url()?
            )));
        }

        let deadline = tokio::time::Instant::now()
            + Duration::from_secs(self.inner.config.startup_timeout_secs);
        let retry_interval =
            Duration::from_millis(self.inner.config.startup_retry_interval_ms.max(50));

        loop {
            if self.try_connect().await.is_ok() {
                return Ok(());
            }

            if let Some(_startup_guard) = self.try_acquire_startup_guard()? {
                if self.try_connect().await.is_ok() {
                    return Ok(());
                }
                self.spawn_controller()?;
            }

            if tokio::time::Instant::now() >= deadline {
                return Err(invalid_input(format!(
                    "controller endpoint `{}` did not become ready within {} seconds",
                    self.inner.config.endpoint_url()?,
                    self.inner.config.startup_timeout_secs
                )));
            }

            tokio::time::sleep(retry_interval).await;
        }
    }

    /// Try to acquire one local startup coordination lock for the configured endpoint.
    /// 为当前配置端点尝试获取一个本地启动协调锁。
    fn try_acquire_startup_guard(&self) -> Result<Option<StartupSpawnGuard>, BoxError> {
        let path = startup_lock_path(&self.inner.config);
        if let Some(parent) = path.parent() {
            create_dir_all(parent)?;
        }

        let stale_after = Duration::from_secs(self.inner.config.startup_timeout_secs.max(15) * 2);
        for _ in 0..2 {
            match OpenOptions::new().write(true).create_new(true).open(&path) {
                Ok(file) => {
                    return Ok(Some(StartupSpawnGuard { path, _file: file }));
                }
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                    if startup_lock_is_stale(&path, stale_after) {
                        let _ = remove_file(&path);
                        continue;
                    }
                    return Ok(None);
                }
                Err(error) => return Err(Box::new(error)),
            }
        }

        Ok(None)
    }

    /// Connect one tonic client after ensuring the endpoint is ready.
    /// 在确保端点就绪后连接一个 tonic 客户端。
    async fn connect_transport_client(&self) -> Result<ControllerServiceClient<Channel>, BoxError> {
        self.ensure_ready().await?;
        self.try_connect().await
    }

    /// Connect one tonic client with one validated controller session.
    /// 连接一个带有已校验控制器会话的 tonic 客户端。
    async fn connect_client(&self) -> Result<ControllerServiceClient<Channel>, BoxError> {
        let mut client = self.connect_transport_client().await?;
        self.ensure_registered_session(&mut client).await?;
        Ok(client)
    }

    /// Perform one direct transport connection attempt.
    /// 执行一次直接的传输连接尝试。
    async fn try_connect(&self) -> Result<ControllerServiceClient<Channel>, BoxError> {
        let endpoint = Endpoint::from_shared(self.inner.config.endpoint_url()?)?
            .connect_timeout(Duration::from_secs(self.inner.config.connect_timeout_secs))
            .timeout(Duration::from_secs(self.inner.config.connect_timeout_secs));
        let channel = endpoint.connect().await?;
        Ok(ControllerServiceClient::new(channel))
    }

    /// Register the current host client lease and return the allocated session snapshot.
    /// 注册当前宿主客户端租约并返回分配后的会话快照。
    async fn register_client(
        &self,
        client: &mut ControllerServiceClient<Channel>,
    ) -> Result<crate::types::ClientLeaseSnapshot, BoxError> {
        let response = client
            .register_client(RegisterClientRequest {
                client_name: self.inner.registration.client_name.clone(),
                host_kind: self.inner.registration.host_kind.clone(),
                process_id: self.inner.registration.process_id,
                process_name: self.inner.registration.process_name.clone(),
                lease_ttl_secs: self.inner.registration.lease_ttl_secs.unwrap_or(0),
            })
            .await?
            .into_inner();
        let snapshot = response.client.ok_or_else(|| {
            invalid_input("register_client response is missing the client snapshot")
        })?;
        Ok(map_client_snapshot(snapshot))
    }

    /// Ensure that one valid controller session exists and replay desired state after recovery.
    /// 确保存在一个有效控制器会话，并在恢复后重放期望状态。
    async fn ensure_registered_session(
        &self,
        client: &mut ControllerServiceClient<Channel>,
    ) -> Result<(), BoxError> {
        let _session_init_guard = self.inner.session_init_lock.lock().await;
        let (maybe_client_session_id, session_needs_replay) =
            self.current_session_replay_state()?;
        let client_session_id = if let Some(client_session_id) = maybe_client_session_id {
            let renew_result = client
                .renew_client_lease(RenewClientLeaseRequest {
                    client_session_id: client_session_id.clone(),
                    lease_ttl_secs: self.inner.registration.lease_ttl_secs.unwrap_or(0),
                })
                .await;
            match renew_result {
                Ok(_) if !session_needs_replay => return Ok(()),
                Ok(_) => client_session_id,
                Err(status) if is_missing_client_session_status(&status) => {
                    self.clear_client_session_id()?;
                    let snapshot = self.register_client(client).await?;
                    self.store_client_session_id(snapshot.client_session_id.clone())?;
                    self.mark_session_replay_pending()?;
                    snapshot.client_session_id
                }
                Err(status) => return Err(Box::new(status)),
            }
        } else {
            let snapshot = self.register_client(client).await?;
            self.store_client_session_id(snapshot.client_session_id.clone())?;
            self.mark_session_replay_pending()?;
            snapshot.client_session_id
        };

        let (attached_spaces, sqlite_bindings, lancedb_bindings) = self.snapshot_desired_state()?;
        for registration in attached_spaces.into_values() {
            client
                .attach_space(AttachSpaceRequest {
                    client_session_id: client_session_id.clone(),
                    space_id: registration.space_id,
                    space_label: registration.space_label,
                    space_kind: map_space_kind(registration.space_kind) as i32,
                    space_root: registration.space_root,
                })
                .await?;
        }
        for request in sqlite_bindings.into_values() {
            client
                .enable_sqlite(EnableSqliteRequest {
                    client_session_id: client_session_id.clone(),
                    space_id: request.space_id,
                    binding_id: request.binding_id,
                    db_path: request.db_path,
                    connection_pool_size: request.connection_pool_size as u32,
                    busy_timeout_ms: request.busy_timeout_ms,
                    journal_mode: request.journal_mode,
                    synchronous: request.synchronous,
                    foreign_keys: request.foreign_keys,
                    temp_store: request.temp_store,
                    wal_autocheckpoint_pages: request.wal_autocheckpoint_pages,
                    cache_size_kib: request.cache_size_kib,
                    mmap_size_bytes: request.mmap_size_bytes,
                    enforce_db_file_lock: request.enforce_db_file_lock,
                    read_only: request.read_only,
                    allow_uri_filenames: request.allow_uri_filenames,
                    trusted_schema: request.trusted_schema,
                    defensive: request.defensive,
                })
                .await?;
        }
        for request in lancedb_bindings.into_values() {
            client
                .enable_lance_db(EnableLanceDbRequest {
                    client_session_id: client_session_id.clone(),
                    space_id: request.space_id,
                    binding_id: request.binding_id,
                    default_db_path: request.default_db_path,
                    db_root: request.db_root.unwrap_or_default(),
                    read_consistency_interval_ms: request.read_consistency_interval_ms.unwrap_or(0),
                    max_upsert_payload: request.max_upsert_payload as u64,
                    max_search_limit: request.max_search_limit as u64,
                    max_concurrent_requests: request.max_concurrent_requests as u64,
                })
                .await?;
        }
        self.mark_session_replay_complete()?;
        Ok(())
    }

    /// Return the current client session identifier or fail when no session is active.
    /// 返回当前客户端会话标识符；如果没有活动会话则返回错误。
    fn current_client_session_id(&self) -> Result<String, BoxError> {
        self.current_client_session_id_opt()?
            .ok_or_else(|| invalid_input("controller client session is not initialized"))
    }

    /// Return the current client session identifier when it exists.
    /// 在当前客户端会话标识符存在时返回它。
    fn current_client_session_id_opt(&self) -> Result<Option<String>, BoxError> {
        Ok(self
            .inner
            .session_state
            .lock()
            .map_err(|_| invalid_input("controller client session state lock is poisoned"))?
            .client_session_id
            .clone())
    }

    /// Return the current session identifier together with the replay-pending flag.
    /// 返回当前会话标识符以及是否仍需重放的标记。
    fn current_session_replay_state(&self) -> Result<(Option<String>, bool), BoxError> {
        let state = self
            .inner
            .session_state
            .lock()
            .map_err(|_| invalid_input("controller client session state lock is poisoned"))?;
        Ok((state.client_session_id.clone(), state.session_needs_replay))
    }

    /// Persist the current client session identifier inside the recovery cache.
    /// 在恢复缓存中持久化当前客户端会话标识符。
    fn store_client_session_id(&self, client_session_id: String) -> Result<(), BoxError> {
        self.inner
            .session_state
            .lock()
            .map_err(|_| invalid_input("controller client session state lock is poisoned"))?
            .client_session_id = Some(client_session_id);
        Ok(())
    }

    /// Clear only the active session identifier while preserving desired replay state.
    /// 仅清除当前活动会话标识符，同时保留期望重放状态。
    fn clear_client_session_id(&self) -> Result<(), BoxError> {
        self.inner
            .session_state
            .lock()
            .map_err(|_| invalid_input("controller client session state lock is poisoned"))?
            .client_session_id = None;
        Ok(())
    }

    /// Mark that the current session still requires desired-state replay.
    /// 标记当前会话仍然需要执行期望状态重放。
    fn mark_session_replay_pending(&self) -> Result<(), BoxError> {
        self.inner
            .session_state
            .lock()
            .map_err(|_| invalid_input("controller client session state lock is poisoned"))?
            .session_needs_replay = true;
        Ok(())
    }

    /// Mark that the current session has completed desired-state replay.
    /// 标记当前会话已经完成期望状态重放。
    fn mark_session_replay_complete(&self) -> Result<(), BoxError> {
        self.inner
            .session_state
            .lock()
            .map_err(|_| invalid_input("controller client session state lock is poisoned"))?
            .session_needs_replay = false;
        Ok(())
    }

    /// Clear both the active session identifier and all desired replay state.
    /// 清除当前活动会话标识符以及全部期望重放状态。
    fn clear_desired_session_state(&self) -> Result<(), BoxError> {
        let mut state = self
            .inner
            .session_state
            .lock()
            .map_err(|_| invalid_input("controller client session state lock is poisoned"))?;
        state.client_session_id = None;
        state.session_needs_replay = false;
        state.attached_spaces.clear();
        state.sqlite_bindings.clear();
        state.lancedb_bindings.clear();
        Ok(())
    }

    /// Snapshot the desired replay state for one future recovery pass.
    /// 为未来一次恢复过程快照保存期望重放状态。
    fn snapshot_desired_state(&self) -> Result<DesiredReplayState, BoxError> {
        let state = self
            .inner
            .session_state
            .lock()
            .map_err(|_| invalid_input("controller client session state lock is poisoned"))?;
        Ok((
            state.attached_spaces.clone(),
            state.sqlite_bindings.clone(),
            state.lancedb_bindings.clone(),
        ))
    }

    /// Remember one attached space so it can be replayed after automatic recovery.
    /// 记录一个已附着空间，以便自动恢复后重放。
    fn remember_attached_space(&self, registration: SpaceRegistration) -> Result<(), BoxError> {
        self.inner
            .session_state
            .lock()
            .map_err(|_| invalid_input("controller client session state lock is poisoned"))?
            .attached_spaces
            .insert(registration.space_id.clone(), registration);
        Ok(())
    }

    /// Forget one detached space and any desired bindings beneath it.
    /// 遗忘一个已解除附着的空间以及其下的期望绑定。
    fn forget_attached_space(&self, space_id: &str) -> Result<(), BoxError> {
        let mut state = self
            .inner
            .session_state
            .lock()
            .map_err(|_| invalid_input("controller client session state lock is poisoned"))?;
        state.attached_spaces.remove(space_id);
        state
            .sqlite_bindings
            .retain(|key, _| key.space_id != space_id);
        state
            .lancedb_bindings
            .retain(|key, _| key.space_id != space_id);
        Ok(())
    }

    /// Remember one SQLite binding so it can be replayed after automatic recovery.
    /// 记录一个 SQLite 绑定，以便自动恢复后重放。
    fn remember_sqlite_binding(
        &self,
        request: ControllerSqliteEnableRequest,
    ) -> Result<(), BoxError> {
        self.inner
            .session_state
            .lock()
            .map_err(|_| invalid_input("controller client session state lock is poisoned"))?
            .sqlite_bindings
            .insert(
                BindingKey::new(&request.space_id, &request.binding_id),
                request,
            );
        Ok(())
    }

    /// Forget one SQLite binding after it has been disabled explicitly.
    /// 在显式禁用后遗忘一个 SQLite 绑定。
    fn forget_sqlite_binding(&self, space_id: &str, binding_id: &str) -> Result<(), BoxError> {
        self.inner
            .session_state
            .lock()
            .map_err(|_| invalid_input("controller client session state lock is poisoned"))?
            .sqlite_bindings
            .remove(&BindingKey::new(space_id, binding_id));
        Ok(())
    }

    /// Remember one LanceDB binding so it can be replayed after automatic recovery.
    /// 记录一个 LanceDB 绑定，以便自动恢复后重放。
    fn remember_lancedb_binding(
        &self,
        request: ControllerLanceDbEnableRequest,
    ) -> Result<(), BoxError> {
        self.inner
            .session_state
            .lock()
            .map_err(|_| invalid_input("controller client session state lock is poisoned"))?
            .lancedb_bindings
            .insert(
                BindingKey::new(&request.space_id, &request.binding_id),
                request,
            );
        Ok(())
    }

    /// Forget one LanceDB binding after it has been disabled explicitly.
    /// 在显式禁用后遗忘一个 LanceDB 绑定。
    fn forget_lancedb_binding(&self, space_id: &str, binding_id: &str) -> Result<(), BoxError> {
        self.inner
            .session_state
            .lock()
            .map_err(|_| invalid_input("controller client session state lock is poisoned"))?
            .lancedb_bindings
            .remove(&BindingKey::new(space_id, binding_id));
        Ok(())
    }

    /// Ensure that the background lease renewer exists exactly once.
    /// 确保后台租约续约任务只存在一份。
    fn ensure_renew_task(&self) -> Result<(), BoxError> {
        let mut renew_state =
            self.inner.renew_state.lock().map_err(|_| {
                invalid_input("controller client renew task state lock is poisoned")
            })?;
        if renew_state.join_handle.is_some() {
            return Ok(());
        }

        let (shutdown_sender, shutdown_receiver) = tokio::sync::oneshot::channel();
        let client = self.clone();
        let interval_secs = self.inner.config.lease_renew_interval_secs.max(1);
        let join_handle = tokio::spawn(async move {
            client
                .run_lease_renewer(shutdown_receiver, interval_secs)
                .await;
        });

        renew_state.shutdown_sender = Some(shutdown_sender);
        renew_state.join_handle = Some(join_handle);
        Ok(())
    }

    /// Stop the background renewer task when it exists.
    /// 当后台续约任务存在时将其停止。
    fn stop_renew_task(&self) {
        if let Ok(mut renew_state) = self.inner.renew_state.lock() {
            if let Some(sender) = renew_state.shutdown_sender.take() {
                let _ = sender.send(());
            }
            if let Some(handle) = renew_state.join_handle.take() {
                handle.abort();
            }
        }
    }

    /// Spawn one controller process with the configured bind address and lifecycle policy.
    /// 使用当前配置的绑定地址与生命周期策略唤起一个控制器进程。
    fn spawn_controller(&self) -> Result<(), BoxError> {
        let mut command = Command::new(self.inner.config.spawn_executable());
        command
            .arg("--bind")
            .arg(self.inner.config.bind_addr()?)
            .arg("--mode")
            .arg(match self.inner.config.spawn_process_mode {
                ControllerProcessMode::Service => "service",
                ControllerProcessMode::Managed => "managed",
            })
            .arg("--minimum-uptime-secs")
            .arg(self.inner.config.minimum_uptime_secs.to_string())
            .arg("--idle-timeout-secs")
            .arg(self.inner.config.idle_timeout_secs.to_string())
            .arg("--default-lease-ttl-secs")
            .arg(self.inner.config.default_lease_ttl_secs.to_string())
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        command.spawn()?;
        Ok(())
    }

    /// Background renew loop that keeps the client lease alive without depending on transport connections.
    /// 在不依赖传输连接持续存在的前提下保持客户端租约存活的后台续约循环。
    async fn run_lease_renewer(
        &self,
        mut shutdown_receiver: tokio::sync::oneshot::Receiver<()>,
        interval_secs: u64,
    ) {
        let mut interval = tokio::time::interval(Duration::from_secs(interval_secs));

        loop {
            tokio::select! {
                _ = &mut shutdown_receiver => {
                    break;
                }
                _ = interval.tick() => {
                    let _ = self.renew_once().await;
                }
            }
        }
    }

    /// Perform one lease renewal attempt.
    /// 执行一次租约续约尝试。
    async fn renew_once(&self) -> Result<(), BoxError> {
        let _client = self.connect_client().await?;
        Ok(())
    }
}

impl Drop for ControllerClient {
    /// Abort the renew task during drop to avoid leaking background work.
    /// 在析构期间中止续约任务，避免后台任务泄漏。
    fn drop(&mut self) {
        if Arc::strong_count(&self.inner) == 1 {
            self.stop_renew_task();
        }
    }
}

impl BindingKey {
    /// Build one stable binding key from the space and binding identifiers.
    /// 使用空间标识与绑定标识构造一个稳定绑定键。
    fn new(space_id: &str, binding_id: &str) -> Self {
        Self {
            space_id: space_id.to_string(),
            binding_id: binding_id.to_string(),
        }
    }
}

/// Map one shared SQLite value into the protobuf representation.
/// 将一条共享 SQLite 值映射成 protobuf 表示。
fn map_sqlite_value(value: ControllerSqliteValue) -> SqliteValue {
    match value {
        ControllerSqliteValue::Int64(value) => SqliteValue {
            kind: Some(crate::rpc::sqlite_value::Kind::Int64Value(value)),
        },
        ControllerSqliteValue::Float64(value) => SqliteValue {
            kind: Some(crate::rpc::sqlite_value::Kind::Float64Value(value)),
        },
        ControllerSqliteValue::String(value) => SqliteValue {
            kind: Some(crate::rpc::sqlite_value::Kind::StringValue(value)),
        },
        ControllerSqliteValue::Bytes(value) => SqliteValue {
            kind: Some(crate::rpc::sqlite_value::Kind::BytesValue(value)),
        },
        ControllerSqliteValue::Bool(value) => SqliteValue {
            kind: Some(crate::rpc::sqlite_value::Kind::BoolValue(value)),
        },
        ControllerSqliteValue::Null => SqliteValue {
            kind: Some(crate::rpc::sqlite_value::Kind::NullValue(
                crate::rpc::NullValue {},
            )),
        },
    }
}

/// Parse one legacy JSON parameter array into typed SQLite values.
/// 将一段 legacy JSON 参数数组解析成类型化 SQLite 值。
fn parse_sqlite_params_json(params_json: &str) -> Result<Vec<ControllerSqliteValue>, BoxError> {
    if params_json.trim().is_empty() {
        return Ok(Vec::new());
    }

    let value = serde_json::from_str::<JsonValue>(params_json)
        .map_err(|error| invalid_input(format!("params_json must be valid JSON: {error}")))?;
    let items = value.as_array().ok_or_else(|| {
        invalid_input("params_json must be a JSON array of scalar values or bytes wrapper objects")
    })?;
    items.iter().cloned().map(json_to_sqlite_value).collect()
}

/// Convert one JSON scalar into the shared SQLite typed value.
/// 将一条 JSON 标量或 bytes 包装对象转换成共享 SQLite 类型化值。
fn json_to_sqlite_value(value: JsonValue) -> Result<ControllerSqliteValue, BoxError> {
    match value {
        JsonValue::Null => Ok(ControllerSqliteValue::Null),
        JsonValue::Bool(value) => Ok(ControllerSqliteValue::Bool(value)),
        JsonValue::Number(value) => {
            if let Some(value) = value.as_i64() {
                Ok(ControllerSqliteValue::Int64(value))
            } else if let Some(value) = value.as_u64() {
                Ok(ControllerSqliteValue::Int64(i64::try_from(value).map_err(
                    |_| invalid_input("params_json contains an unsigned integer larger than i64"),
                )?))
            } else if let Some(value) = value.as_f64() {
                Ok(ControllerSqliteValue::Float64(value))
            } else {
                Err(invalid_input(
                    "params_json contains an unsupported numeric value",
                ))
            }
        }
        JsonValue::String(value) => Ok(ControllerSqliteValue::String(value)),
        JsonValue::Object(value) => {
            let wrapper = serde_json::from_value::<JsonSqliteBytesValue>(JsonValue::Object(value))
                .map_err(|_| {
                    invalid_input(
                        "params_json object values must use {\"type\":\"bytes_base64\",\"base64\":\"...\"} or {\"__type\":\"bytes_base64\",\"base64\":\"...\"}",
                    )
                })?;
            let wrapper_type = if !wrapper.r#type.trim().is_empty() {
                wrapper.r#type.trim()
            } else {
                wrapper.__type.trim()
            };
            if wrapper_type != "bytes_base64" {
                return Err(invalid_input(
                    "params_json object values only support the bytes_base64 wrapper type",
                ));
            }
            Ok(ControllerSqliteValue::Bytes(decode_base64(
                &wrapper.base64,
            )?))
        }
        JsonValue::Array(_) => Err(invalid_input(
            "params_json only supports scalar JSON values or bytes wrapper objects",
        )),
    }
}

/// Decode base64 text without introducing extra dependencies.
/// 在不引入额外依赖的前提下解码 base64 文本。
fn decode_base64(input: &str) -> Result<Vec<u8>, BoxError> {
    if !input.len().is_multiple_of(4) {
        return Err(invalid_input("base64 input length must be a multiple of 4"));
    }

    let mut output = Vec::with_capacity(input.len() / 4 * 3);
    for chunk in input.as_bytes().chunks_exact(4) {
        let a = decode_base64_char(chunk[0])?;
        let b = decode_base64_char(chunk[1])?;
        let c_padding = chunk[2] == b'=';
        let d_padding = chunk[3] == b'=';
        let c = if c_padding {
            0
        } else {
            decode_base64_char(chunk[2])?
        };
        let d = if d_padding {
            0
        } else {
            decode_base64_char(chunk[3])?
        };

        output.push((a << 2) | (b >> 4));
        if !c_padding {
            output.push(((b & 0x0F) << 4) | (c >> 2));
        }
        if !d_padding {
            output.push(((c & 0x03) << 6) | d);
        }
    }

    Ok(output)
}

/// Decode one base64 character into its 6-bit value.
/// 将一个 base64 字符解码成 6 位值。
fn decode_base64_char(value: u8) -> Result<u8, BoxError> {
    match value {
        b'A'..=b'Z' => Ok(value - b'A'),
        b'a'..=b'z' => Ok(value - b'a' + 26),
        b'0'..=b'9' => Ok(value - b'0' + 52),
        b'+' => Ok(62),
        b'/' => Ok(63),
        _ => Err(invalid_input(format!(
            "invalid base64 character `{}`",
            value as char
        ))),
    }
}

/// Map one controller tokenizer mode into the protobuf tokenizer mode.
/// 将一条控制器分词模式映射成 protobuf 分词模式。
fn map_sqlite_tokenizer_mode(mode: ControllerSqliteTokenizerMode) -> ProtoSqliteTokenizerMode {
    match mode {
        ControllerSqliteTokenizerMode::None => ProtoSqliteTokenizerMode::None,
        ControllerSqliteTokenizerMode::Jieba => ProtoSqliteTokenizerMode::Jieba,
    }
}

/// Convert one JSON create-table column into the shared typed representation.
/// 将一条 JSON 建表列定义转换成共享类型表示。
fn map_create_table_json_column(
    column: CreateTableJsonColumn,
) -> Result<ControllerLanceDbColumnDef, BoxError> {
    Ok(ControllerLanceDbColumnDef {
        name: column.name,
        column_type: parse_lancedb_column_type(&column.column_type)?,
        vector_dim: column.vector_dim,
        nullable: column.nullable,
    })
}

/// Map one shared LanceDB column definition into the protobuf representation.
/// 将一条共享 LanceDB 列定义映射成 protobuf 表示。
fn map_lancedb_column_def(column: ControllerLanceDbColumnDef) -> ProtoLanceDbColumnDef {
    ProtoLanceDbColumnDef {
        name: column.name,
        column_type: map_lancedb_column_type(column.column_type) as i32,
        vector_dim: column.vector_dim,
        nullable: column.nullable,
    }
}

/// Parse one legacy column-type string used by JSON wrappers.
/// 解析 JSON 包装层使用的 legacy 列类型字符串。
fn parse_lancedb_column_type(value: &str) -> Result<ControllerLanceDbColumnType, BoxError> {
    match value.trim().to_ascii_lowercase().as_str() {
        "" | "unspecified" => Ok(ControllerLanceDbColumnType::Unspecified),
        "string" => Ok(ControllerLanceDbColumnType::String),
        "int64" => Ok(ControllerLanceDbColumnType::Int64),
        "float64" => Ok(ControllerLanceDbColumnType::Float64),
        "bool" | "boolean" => Ok(ControllerLanceDbColumnType::Bool),
        "vector_float32" | "vector-float32" => Ok(ControllerLanceDbColumnType::VectorFloat32),
        "float32" => Ok(ControllerLanceDbColumnType::Float32),
        "uint64" => Ok(ControllerLanceDbColumnType::Uint64),
        "int32" => Ok(ControllerLanceDbColumnType::Int32),
        "uint32" => Ok(ControllerLanceDbColumnType::Uint32),
        other => Err(invalid_input(format!("unsupported column_type: {other}"))),
    }
}

/// Map one shared LanceDB column type into the protobuf representation.
/// 将一条共享 LanceDB 列类型映射成 protobuf 表示。
fn map_lancedb_column_type(kind: ControllerLanceDbColumnType) -> ProtoLanceDbColumnType {
    match kind {
        ControllerLanceDbColumnType::Unspecified => {
            ProtoLanceDbColumnType::LancedbColumnTypeUnspecified
        }
        ControllerLanceDbColumnType::String => ProtoLanceDbColumnType::LancedbColumnTypeString,
        ControllerLanceDbColumnType::Int64 => ProtoLanceDbColumnType::LancedbColumnTypeInt64,
        ControllerLanceDbColumnType::Float64 => ProtoLanceDbColumnType::LancedbColumnTypeFloat64,
        ControllerLanceDbColumnType::Bool => ProtoLanceDbColumnType::LancedbColumnTypeBool,
        ControllerLanceDbColumnType::VectorFloat32 => {
            ProtoLanceDbColumnType::LancedbColumnTypeVectorFloat32
        }
        ControllerLanceDbColumnType::Float32 => ProtoLanceDbColumnType::LancedbColumnTypeFloat32,
        ControllerLanceDbColumnType::Uint64 => ProtoLanceDbColumnType::LancedbColumnTypeUint64,
        ControllerLanceDbColumnType::Int32 => ProtoLanceDbColumnType::LancedbColumnTypeInt32,
        ControllerLanceDbColumnType::Uint32 => ProtoLanceDbColumnType::LancedbColumnTypeUint32,
    }
}

/// Parse one legacy input-format string used by JSON wrappers.
/// 解析 JSON 包装层使用的 legacy 输入格式字符串。
fn parse_lancedb_input_format(value: &str) -> Result<ControllerLanceDbInputFormat, BoxError> {
    match value.trim().to_ascii_lowercase().as_str() {
        "" | "unspecified" => Ok(ControllerLanceDbInputFormat::Unspecified),
        "json" | "json_rows" | "json-rows" => Ok(ControllerLanceDbInputFormat::JsonRows),
        "arrow" | "arrow_ipc" | "arrow-ipc" => Ok(ControllerLanceDbInputFormat::ArrowIpc),
        other => Err(invalid_input(format!("unsupported input_format: {other}"))),
    }
}

/// Map one shared LanceDB input format into the protobuf representation.
/// 将一条共享 LanceDB 输入格式映射成 protobuf 表示。
fn map_lancedb_input_format(format: ControllerLanceDbInputFormat) -> ProtoLanceDbInputFormat {
    match format {
        ControllerLanceDbInputFormat::Unspecified => {
            ProtoLanceDbInputFormat::LancedbInputFormatUnspecified
        }
        ControllerLanceDbInputFormat::JsonRows => {
            ProtoLanceDbInputFormat::LancedbInputFormatJsonRows
        }
        ControllerLanceDbInputFormat::ArrowIpc => {
            ProtoLanceDbInputFormat::LancedbInputFormatArrowIpc
        }
    }
}

/// Parse one legacy output-format string used by JSON wrappers.
/// 解析 JSON 包装层使用的 legacy 输出格式字符串。
fn parse_lancedb_output_format(value: &str) -> Result<ControllerLanceDbOutputFormat, BoxError> {
    match value.trim().to_ascii_lowercase().as_str() {
        "" | "unspecified" | "arrow" | "arrow_ipc" | "arrow-ipc" => {
            Ok(ControllerLanceDbOutputFormat::ArrowIpc)
        }
        "json" | "json_rows" | "json-rows" => Ok(ControllerLanceDbOutputFormat::JsonRows),
        other => Err(invalid_input(format!("unsupported output_format: {other}"))),
    }
}

/// Map one shared LanceDB output format into the protobuf representation.
/// 将一条共享 LanceDB 输出格式映射成 protobuf 表示。
fn map_lancedb_output_format(format: ControllerLanceDbOutputFormat) -> ProtoLanceDbOutputFormat {
    match format {
        ControllerLanceDbOutputFormat::Unspecified => {
            ProtoLanceDbOutputFormat::LancedbOutputFormatUnspecified
        }
        ControllerLanceDbOutputFormat::ArrowIpc => {
            ProtoLanceDbOutputFormat::LancedbOutputFormatArrowIpc
        }
        ControllerLanceDbOutputFormat::JsonRows => {
            ProtoLanceDbOutputFormat::LancedbOutputFormatJsonRows
        }
    }
}

/// Map one protobuf client snapshot into the shared representation.
/// 将一条 protobuf 客户端快照映射成共享表示。
fn map_client_snapshot(
    snapshot: crate::rpc::ClientLeaseSnapshot,
) -> crate::types::ClientLeaseSnapshot {
    crate::types::ClientLeaseSnapshot {
        client_session_id: snapshot.client_session_id,
        client_name: snapshot.client_name,
        host_kind: snapshot.host_kind,
        process_id: snapshot.process_id,
        process_name: snapshot.process_name,
        last_seen_unix_ms: snapshot.last_seen_unix_ms,
        expires_at_unix_ms: snapshot.expires_at_unix_ms,
        attached_space_ids: snapshot.attached_space_ids,
    }
}

/// Map one protobuf process mode into the controller-core process mode.
/// 将一个 protobuf 进程模式映射成控制器核心进程模式。
fn map_process_mode(raw_mode: i32) -> Result<ControllerProcessMode, BoxError> {
    ControllerProcessMode::from_proto_value(raw_mode)
}

/// Map one controller-core space kind into the protobuf enum value.
/// 将一个控制器核心空间类型映射成 protobuf 枚举值。
fn map_space_kind(kind: SpaceKind) -> crate::rpc::SpaceKind {
    match kind {
        SpaceKind::Root => crate::rpc::SpaceKind::Root,
        SpaceKind::User => crate::rpc::SpaceKind::User,
        SpaceKind::Project => crate::rpc::SpaceKind::Project,
    }
}

/// Map one protobuf space snapshot into the shared snapshot type.
/// 将一条 protobuf 空间快照映射成共享快照类型。
fn map_space_snapshot(snapshot: crate::rpc::SpaceSnapshot) -> Result<SpaceSnapshot, BoxError> {
    Ok(SpaceSnapshot {
        space_id: snapshot.space_id,
        space_label: snapshot.space_label,
        space_kind: match crate::rpc::SpaceKind::try_from(snapshot.space_kind) {
            Ok(crate::rpc::SpaceKind::Root) => SpaceKind::Root,
            Ok(crate::rpc::SpaceKind::User) => SpaceKind::User,
            Ok(crate::rpc::SpaceKind::Project) => SpaceKind::Project,
            _ => {
                return Err(invalid_input(format!(
                    "unsupported protobuf space kind value `{}`",
                    snapshot.space_kind
                )));
            }
        },
        space_root: snapshot.space_root,
        attached_clients: snapshot.attached_clients as usize,
        sqlite: snapshot
            .sqlite
            .map(|backend| crate::types::SpaceBackendStatus {
                enabled: backend.enabled,
                mode: backend.mode,
                target: backend.target,
            }),
        lancedb: snapshot
            .lancedb
            .map(|backend| crate::types::SpaceBackendStatus {
                enabled: backend.enabled,
                mode: backend.mode,
                target: backend.target,
            }),
    })
}

/// Return the default nullable flag for JSON create-table columns.
/// 返回 JSON 建表列的默认可空标记。
fn default_nullable() -> bool {
    true
}

/// Return the default search limit used by JSON wrappers.
/// 返回 JSON 包装层使用的默认检索限制。
fn default_search_limit() -> u32 {
    10
}

/// Build one boxed invalid-input error.
/// 构造一个盒装无效输入错误。
fn invalid_input(message: impl Into<String>) -> BoxError {
    VldbControllerError::invalid_input(message)
}

/// Detect the controller response used when one client session no longer exists.
/// 检测控制器用于表达客户端会话已不存在的响应。
fn is_missing_client_session_status(status: &tonic::Status) -> bool {
    status.code() == tonic::Code::InvalidArgument && status.message().contains("is not registered")
}

/// Build one stable local startup lock path for the configured endpoint.
/// 为当前配置端点构造一个稳定的本地启动锁路径。
fn startup_lock_path(config: &ControllerClientConfig) -> PathBuf {
    let mut hasher = DefaultHasher::new();
    config
        .endpoint_url()
        .unwrap_or_else(|_| config.endpoint.trim().to_string())
        .hash(&mut hasher);
    let hash = hasher.finish();
    std::env::temp_dir()
        .join("vldb-controller")
        .join(format!("startup-{hash:016x}.lock"))
}

/// Normalize one host and port into a concrete socket address.
/// 将一个主机与端口标准化成具体套接字地址。
fn normalize_socket_addr(host: &str, port: &str, raw_value: &str) -> Result<SocketAddr, BoxError> {
    format!("{host}:{port}")
        .parse::<SocketAddr>()
        .map_err(|error| {
            invalid_input(format!(
                "invalid controller endpoint `{raw_value}`: {error}"
            ))
        })
}

/// Normalize one HTTP endpoint while preserving hostnames and stripping paths.
/// 标准化一个 HTTP 端点，同时保留主机名并去除路径部分。
fn normalize_http_endpoint(value: &str) -> Result<String, BoxError> {
    let Some((scheme, rest)) = value.split_once("://") else {
        return Err(invalid_input(format!(
            "controller endpoint `{value}` must include an http or https scheme"
        )));
    };
    if scheme != "http" && scheme != "https" {
        return Err(invalid_input(format!(
            "controller endpoint `{value}` must use http or https"
        )));
    }
    let authority = rest
        .split(['/', '?', '#'])
        .next()
        .unwrap_or_default()
        .trim();
    if authority.is_empty() {
        return Err(invalid_input(format!(
            "controller endpoint `{value}` must include one host:port authority"
        )));
    }
    if authority.contains(' ') {
        return Err(invalid_input(format!(
            "controller endpoint `{value}` must not contain spaces"
        )));
    }
    Ok(format!("{scheme}://{authority}"))
}

/// Normalize one controller endpoint into a concrete bind address when auto-spawn needs one.
/// 当自动唤起需要绑定地址时，将控制器端点标准化成具体套接字地址。
fn normalize_bind_endpoint(value: &str) -> Result<String, BoxError> {
    let authority = if value.starts_with("http://") || value.starts_with("https://") {
        strip_http_scheme(&normalize_http_endpoint(value)?).to_string()
    } else {
        value.trim().to_string()
    };

    if let Ok(socket_addr) = authority.parse::<SocketAddr>() {
        if socket_addr.port() == 0 {
            return Err(invalid_input(format!(
                "controller endpoint `{value}` must not use port 0 for auto-spawn"
            )));
        }
        return Ok(socket_addr.to_string());
    }

    if let Some(port) = authority.strip_prefix("localhost:") {
        return Ok(normalize_socket_addr("127.0.0.1", port, value)?.to_string());
    }

    Err(invalid_input(format!(
        "controller endpoint `{value}` cannot be converted into a concrete bind address for auto-spawn"
    )))
}

/// Remove one HTTP scheme prefix from a normalized endpoint string.
/// 从标准化端点字符串中移除一个 HTTP scheme 前缀。
fn strip_http_scheme(value: &str) -> &str {
    value
        .trim_start_matches("http://")
        .trim_start_matches("https://")
}

/// Check whether one startup lock file is stale enough to be reclaimed.
/// 检查一个启动锁文件是否已经陈旧到可以被回收。
fn startup_lock_is_stale(path: &PathBuf, stale_after: Duration) -> bool {
    let Ok(metadata) = std::fs::metadata(path) else {
        return false;
    };
    let Ok(modified_at) = metadata.modified() else {
        return false;
    };
    match SystemTime::now().duration_since(modified_at) {
        Ok(elapsed) => elapsed >= stale_after,
        Err(_) => false,
    }
}

#[cfg(test)]
mod tests {
    use super::{
        ControllerClientConfig, parse_lancedb_output_format, parse_sqlite_params_json,
        startup_lock_path,
    };
    use crate::types::{ControllerLanceDbOutputFormat, ControllerSqliteValue};

    #[test]
    fn client_config_normalizes_endpoint_and_bind_addr() {
        let config = ControllerClientConfig {
            endpoint: "127.0.0.1:19811".to_string(),
            ..ControllerClientConfig::default()
        };

        assert_eq!(
            config.endpoint_url().expect("endpoint should normalize"),
            "http://127.0.0.1:19811"
        );
        assert_eq!(
            config.bind_addr().expect("bind addr should normalize"),
            "127.0.0.1:19811"
        );
    }

    #[test]
    fn client_config_keeps_existing_http_scheme() {
        let config = ControllerClientConfig {
            endpoint: "http://127.0.0.1:19801".to_string(),
            ..ControllerClientConfig::default()
        };

        assert_eq!(
            config.endpoint_url().expect("endpoint should stay valid"),
            "http://127.0.0.1:19801"
        );
        assert_eq!(
            config.bind_addr().expect("bind addr should stay valid"),
            "127.0.0.1:19801"
        );
    }

    #[test]
    fn client_config_normalizes_port_only_endpoint() {
        let config = ControllerClientConfig {
            endpoint: "19811".to_string(),
            ..ControllerClientConfig::default()
        };

        assert_eq!(
            config
                .endpoint_url()
                .expect("port-only endpoint should normalize"),
            "http://127.0.0.1:19811"
        );
        assert_eq!(
            config.bind_addr().expect("bind addr should normalize"),
            "127.0.0.1:19811"
        );
    }

    #[test]
    fn client_config_normalizes_colon_port_endpoint() {
        let config = ControllerClientConfig {
            endpoint: ":19811".to_string(),
            ..ControllerClientConfig::default()
        };

        assert_eq!(
            config
                .endpoint_url()
                .expect("colon-port endpoint should normalize"),
            "http://127.0.0.1:19811"
        );
        assert_eq!(
            config.bind_addr().expect("bind addr should normalize"),
            "0.0.0.0:19811"
        );
    }

    #[test]
    fn client_config_keeps_hostname_endpoints_connectable() {
        let config = ControllerClientConfig {
            endpoint: "localhost:19811".to_string(),
            ..ControllerClientConfig::default()
        };

        assert_eq!(
            config
                .endpoint_url()
                .expect("hostname endpoint should stay connectable"),
            "http://localhost:19811"
        );
        assert_eq!(
            config
                .bind_addr()
                .expect("bind addr should resolve localhost for auto-spawn"),
            "127.0.0.1:19811"
        );
    }

    #[test]
    fn client_config_rejects_non_local_hostname_for_auto_spawn_bind_addr() {
        let config = ControllerClientConfig {
            endpoint: "controller.internal:19811".to_string(),
            ..ControllerClientConfig::default()
        };

        assert_eq!(
            config
                .endpoint_url()
                .expect("hostname endpoint should stay connectable"),
            "http://controller.internal:19811"
        );
        let error = config
            .bind_addr()
            .expect_err("non-local hostname should not become auto-spawn bind addr");
        assert!(
            error
                .to_string()
                .contains("cannot be converted into a concrete bind address"),
            "error should explain the auto-spawn bind addr restriction, got: {error}"
        );
    }

    #[test]
    fn startup_lock_path_is_stable_for_same_endpoint() {
        let config = ControllerClientConfig {
            endpoint: "127.0.0.1:19801".to_string(),
            ..ControllerClientConfig::default()
        };
        assert_eq!(startup_lock_path(&config), startup_lock_path(&config));
    }

    #[test]
    fn startup_lock_path_differs_for_different_endpoints() {
        let shared = ControllerClientConfig {
            endpoint: "127.0.0.1:19801".to_string(),
            ..ControllerClientConfig::default()
        };
        let isolated = ControllerClientConfig {
            endpoint: "127.0.0.1:19811".to_string(),
            ..ControllerClientConfig::default()
        };
        assert_ne!(startup_lock_path(&shared), startup_lock_path(&isolated));
    }

    #[test]
    fn sqlite_json_wrapper_params_are_parsed_into_typed_values() {
        let params =
            parse_sqlite_params_json("[1,2.5,true,\"hello\",null]").expect("params should parse");
        assert_eq!(
            params,
            vec![
                ControllerSqliteValue::Int64(1),
                ControllerSqliteValue::Float64(2.5),
                ControllerSqliteValue::Bool(true),
                ControllerSqliteValue::String("hello".to_string()),
                ControllerSqliteValue::Null,
            ]
        );
    }

    #[test]
    fn sqlite_json_wrapper_supports_bytes_base64_objects() {
        let params = parse_sqlite_params_json(
            r#"[{"type":"bytes_base64","base64":"aGVsbG8="},{"__type":"bytes_base64","base64":"AQID"}]"#,
        )
        .expect("bytes params should parse");
        assert_eq!(
            params,
            vec![
                ControllerSqliteValue::Bytes(b"hello".to_vec()),
                ControllerSqliteValue::Bytes(vec![1, 2, 3]),
            ]
        );
    }

    #[test]
    fn lancedb_json_wrapper_output_format_uses_arrow_default() {
        assert_eq!(
            parse_lancedb_output_format("").expect("blank output format should parse"),
            ControllerLanceDbOutputFormat::ArrowIpc
        );
    }
}

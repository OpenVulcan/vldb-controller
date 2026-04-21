use serde::{Deserialize, Serialize};

/// Logical process mode used by the controller lifecycle policy.
/// 控制器生命周期策略使用的逻辑进程模式。
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ControllerProcessMode {
    /// Service mode keeps the controller alive until the process is stopped externally.
    /// 服务模式会让控制器持续存活，直到进程被外部停止。
    Service,
    /// Managed mode allows idle self-shutdown after the configured uptime and idle windows.
    /// 托管模式允许控制器在满足最小时长和空闲窗口后自行退出。
    Managed,
}

/// Logical space kind used by the controller.
/// 控制层使用的逻辑空间类型。
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SpaceKind {
    /// Global root-level shared runtime space.
    /// 全局根级共享运行时空间。
    Root,
    /// User-level shared runtime space.
    /// 用户级共享运行时空间。
    User,
    /// Project-level runtime space owned by one workspace.
    /// 由单个工作区拥有的项目级运行时空间。
    Project,
}

/// Process-level runtime policy used by the service implementation.
/// 服务实现使用的进程级运行时策略。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ControllerRuntimeConfig {
    /// Selected process mode.
    /// 当前选定的进程模式。
    pub process_mode: ControllerProcessMode,
    /// Minimum uptime before a managed controller is allowed to stop itself.
    /// 托管模式控制器允许自停前必须满足的最小存活时长。
    pub minimum_uptime_secs: u64,
    /// Idle timeout after the last observed request before managed self-shutdown.
    /// 托管模式下自最后一次请求开始计算的空闲超时时长。
    pub idle_timeout_secs: u64,
    /// Default lease TTL applied when a client does not provide one explicitly.
    /// 客户端未显式提供时使用的默认租约 TTL。
    pub default_lease_ttl_secs: u64,
}

impl Default for ControllerRuntimeConfig {
    fn default() -> Self {
        Self {
            process_mode: ControllerProcessMode::Managed,
            minimum_uptime_secs: 300,
            idle_timeout_secs: 900,
            default_lease_ttl_secs: 120,
        }
    }
}

/// Service launch configuration resolved from process arguments.
/// 从进程启动参数解析出的服务启动配置。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ControllerServerConfig {
    /// Local bind address used by the gRPC controller endpoint.
    /// gRPC 控制器端点使用的本地绑定地址。
    pub bind_addr: String,
    /// Runtime lifecycle policy applied to the controller core.
    /// 应用于控制器核心的运行时生命周期策略。
    pub runtime: ControllerRuntimeConfig,
}

impl Default for ControllerServerConfig {
    fn default() -> Self {
        Self {
            bind_addr: "127.0.0.1:19801".to_string(),
            runtime: ControllerRuntimeConfig::default(),
        }
    }
}

/// Stable host-supplied registration for one runtime space.
/// 宿主提供的单个运行时空间稳定注册信息。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SpaceRegistration {
    /// Stable unique controller key such as `root`, `user`, or one project-specific identifier.
    /// 稳定唯一的控制层键，例如 `root`、`user` 或某个项目专用标识符。
    pub space_id: String,
    /// Stable human-readable space label supplied by the host.
    /// 由宿主提供的稳定可读空间标签。
    pub space_label: String,
    /// Logical space kind used for diagnostics and policy routing.
    /// 用于诊断和策略路由的逻辑空间类型。
    pub space_kind: SpaceKind,
    /// Physical runtime-space root path supplied by the host.
    /// 由宿主提供的物理运行时空间根路径。
    pub space_root: String,
}

/// Stable client registration used to express that one host process is using the controller.
/// 用于表达宿主进程正在使用控制器的稳定客户端注册信息。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ClientRegistration {
    /// Stable host-generated client identifier.
    /// 由宿主生成的稳定客户端标识符。
    pub client_id: String,
    /// Host kind such as `mcp`, `ide`, or `opencode`.
    /// 宿主类型，例如 `mcp`、`ide` 或 `opencode`。
    pub host_kind: String,
    /// Operating-system process identifier when available.
    /// 操作系统进程标识符（如果可用）。
    pub process_id: u32,
    /// Human-readable process name supplied by the host.
    /// 由宿主提供的人类可读进程名。
    pub process_name: String,
    /// Optional explicit lease TTL; falls back to the runtime default when absent.
    /// 可选的显式租约 TTL；缺失时回落到运行时默认值。
    pub lease_ttl_secs: Option<u64>,
}

/// SQLite enable request resolved by the host before entering the controller.
/// 宿主在进入控制层前解析完成的 SQLite 启用请求。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ControllerSqliteEnableRequest {
    /// Target runtime space identifier.
    /// 目标运行时空间标识符。
    pub space_id: String,
    /// Concrete database path chosen by the host.
    /// 由宿主决定的实际数据库路径。
    pub db_path: String,
    /// Suggested connection-pool size kept as runtime metadata.
    /// 作为运行时元信息保留的建议连接池大小。
    pub connection_pool_size: usize,
    /// busy_timeout in milliseconds.
    /// 毫秒级 busy_timeout。
    pub busy_timeout_ms: u64,
    /// Requested journal mode.
    /// 期望的 journal 模式。
    pub journal_mode: String,
    /// Requested synchronous mode.
    /// 期望的 synchronous 模式。
    pub synchronous: String,
    /// Whether foreign keys should be enabled.
    /// 是否启用外键。
    pub foreign_keys: bool,
    /// Temporary storage mode.
    /// 临时存储模式。
    pub temp_store: String,
    /// WAL auto-checkpoint page count.
    /// WAL 自动 checkpoint 页数。
    pub wal_autocheckpoint_pages: u32,
    /// SQLite cache size in KiB.
    /// SQLite 缓存大小（KiB）。
    pub cache_size_kib: i64,
    /// mmap size in bytes.
    /// mmap 大小（字节）。
    pub mmap_size_bytes: u64,
    /// Whether the database file lock should be enforced.
    /// 是否强制启用数据库文件锁。
    pub enforce_db_file_lock: bool,
    /// Whether the database should be opened as read-only.
    /// 是否以只读方式打开数据库。
    pub read_only: bool,
    /// Whether SQLite URI filenames are allowed.
    /// 是否允许 SQLite URI 文件名。
    pub allow_uri_filenames: bool,
    /// Whether trusted_schema should be enabled.
    /// 是否启用 trusted_schema。
    pub trusted_schema: bool,
    /// Whether SQLite defensive mode should be enabled.
    /// 是否启用 SQLite defensive 模式。
    pub defensive: bool,
}

impl Default for ControllerSqliteEnableRequest {
    fn default() -> Self {
        Self {
            space_id: String::new(),
            db_path: String::new(),
            connection_pool_size: 8,
            busy_timeout_ms: 5_000,
            journal_mode: "WAL".to_string(),
            synchronous: "NORMAL".to_string(),
            foreign_keys: true,
            temp_store: "MEMORY".to_string(),
            wal_autocheckpoint_pages: 1_000,
            cache_size_kib: 65_536,
            mmap_size_bytes: 268_435_456,
            enforce_db_file_lock: true,
            read_only: false,
            allow_uri_filenames: false,
            trusted_schema: false,
            defensive: true,
        }
    }
}

/// LanceDB enable request resolved by the host before entering the controller.
/// 宿主在进入控制层前解析完成的 LanceDB 启用请求。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ControllerLanceDbEnableRequest {
    /// Target runtime space identifier.
    /// 目标运行时空间标识符。
    pub space_id: String,
    /// Default database path resolved by the host.
    /// 由宿主解析出的默认数据库路径。
    pub default_db_path: String,
    /// Optional database root used for named child databases.
    /// 用于命名子库的可选数据库根路径。
    pub db_root: Option<String>,
    /// Optional read-consistency interval in milliseconds.
    /// 可选的毫秒级读一致性间隔。
    pub read_consistency_interval_ms: Option<u64>,
    /// Maximum upsert payload size in bytes.
    /// 最大 upsert 负载大小（字节）。
    pub max_upsert_payload: usize,
    /// Maximum search result limit.
    /// 最大搜索结果限制。
    pub max_search_limit: usize,
    /// Maximum concurrent request count.
    /// 最大并发请求数。
    pub max_concurrent_requests: usize,
}

impl Default for ControllerLanceDbEnableRequest {
    fn default() -> Self {
        Self {
            space_id: String::new(),
            default_db_path: String::new(),
            db_root: None,
            read_consistency_interval_ms: None,
            max_upsert_payload: 50 * 1024 * 1024,
            max_search_limit: 10_000,
            max_concurrent_requests: 500,
        }
    }
}

/// One backend status snapshot exposed for diagnostics.
/// 用于诊断的一条后端状态快照。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SpaceBackendStatus {
    /// Whether the backend is currently enabled.
    /// 当前后端是否已启用。
    pub enabled: bool,
    /// Stable backend mode name for diagnostics.
    /// 供诊断使用的稳定后端模式名称。
    pub mode: String,
    /// Backend-owned path or root chosen by the host.
    /// 由宿主决定的后端路径或根目录。
    pub target: String,
}

/// One full space snapshot exposed by the controller.
/// 控制层暴露的一条完整空间快照。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SpaceSnapshot {
    /// Stable runtime space identifier.
    /// 稳定运行时空间标识符。
    pub space_id: String,
    /// Stable human-readable space label.
    /// 稳定的人类可读空间标签。
    pub space_label: String,
    /// Logical space kind.
    /// 逻辑空间类型。
    pub space_kind: SpaceKind,
    /// Physical space root path.
    /// 物理空间根路径。
    pub space_root: String,
    /// Number of attached clients that currently reference this space.
    /// 当前引用该空间的已附着客户端数量。
    pub attached_clients: usize,
    /// Whether SQLite is currently enabled.
    /// SQLite 当前是否已启用。
    pub sqlite: Option<SpaceBackendStatus>,
    /// Whether LanceDB is currently enabled.
    /// LanceDB 当前是否已启用。
    pub lancedb: Option<SpaceBackendStatus>,
}

/// One client lease snapshot exposed for diagnostics.
/// 用于诊断的一条客户端租约快照。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ClientLeaseSnapshot {
    /// Stable host-generated client identifier.
    /// 由宿主生成的稳定客户端标识符。
    pub client_id: String,
    /// Host kind supplied during registration.
    /// 注册时提供的宿主类型。
    pub host_kind: String,
    /// Operating-system process identifier.
    /// 操作系统进程标识符。
    pub process_id: u32,
    /// Host-supplied human-readable process name.
    /// 宿主提供的人类可读进程名。
    pub process_name: String,
    /// Last observed activity timestamp in Unix milliseconds.
    /// 最后一次观察到活动的 Unix 毫秒时间戳。
    pub last_seen_unix_ms: u64,
    /// Lease expiry timestamp in Unix milliseconds.
    /// 租约到期的 Unix 毫秒时间戳。
    pub expires_at_unix_ms: u64,
    /// Space identifiers currently attached by the client.
    /// 当前由该客户端附着的空间标识符集合。
    pub attached_space_ids: Vec<String>,
}

/// High-level controller status snapshot exposed to hosts and diagnostics.
/// 向宿主与诊断面暴露的控制器高层状态快照。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ControllerStatusSnapshot {
    /// Selected process mode.
    /// 当前选定的进程模式。
    pub process_mode: ControllerProcessMode,
    /// Bound listening address used by the current controller instance.
    /// 当前控制器实例使用的监听地址。
    pub bind_addr: String,
    /// Process start timestamp in Unix milliseconds.
    /// 进程启动的 Unix 毫秒时间戳。
    pub started_at_unix_ms: u64,
    /// Last request activity timestamp in Unix milliseconds.
    /// 最后一次请求活动的 Unix 毫秒时间戳。
    pub last_request_at_unix_ms: u64,
    /// Minimum uptime before managed self-shutdown is allowed.
    /// 托管自停允许发生前必须满足的最小时长。
    pub minimum_uptime_secs: u64,
    /// Idle timeout used by managed self-shutdown.
    /// 托管自停使用的空闲超时。
    pub idle_timeout_secs: u64,
    /// Default lease TTL used when a client does not provide one.
    /// 客户端未提供显式 TTL 时使用的默认租约 TTL。
    pub default_lease_ttl_secs: u64,
    /// Number of active client leases after expiry pruning.
    /// 过期清理后的活跃客户端租约数量。
    pub active_clients: usize,
    /// Number of currently attached spaces.
    /// 当前已附着的空间数量。
    pub attached_spaces: usize,
    /// Number of in-flight requests observed by the runtime.
    /// 运行时当前观察到的执行中请求数量。
    pub inflight_requests: usize,
    /// Whether the runtime currently qualifies for managed shutdown.
    /// 运行时当前是否满足托管关闭条件。
    pub shutdown_candidate: bool,
}

/// One SQLite execution result surfaced by the controller data plane.
/// 控制器数据面暴露的一条 SQLite 执行结果。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ControllerSqliteExecuteResult {
    /// Whether the SQL execution succeeded.
    /// SQL 执行是否成功。
    pub success: bool,
    /// Human-readable execution message.
    /// 人类可读的执行结果消息。
    pub message: String,
    /// Number of changed rows reported by SQLite.
    /// SQLite 报告的受影响行数。
    pub rows_changed: i64,
    /// Last inserted row id reported by SQLite.
    /// SQLite 报告的最近插入行 ID。
    pub last_insert_rowid: i64,
}

/// One SQLite JSON query result surfaced by the controller data plane.
/// 控制器数据面暴露的一条 SQLite JSON 查询结果。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ControllerSqliteQueryResult {
    /// JSON row-set payload returned by SQLite.
    /// SQLite 返回的 JSON 行集载荷。
    pub json_data: String,
    /// Number of rows returned by the query.
    /// 查询返回的行数。
    pub row_count: u64,
}

/// One SQLite batch-execution result surfaced by the controller data plane.
/// 控制器数据面暴露的一条 SQLite 批量执行结果。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ControllerSqliteExecuteBatchResult {
    /// Whether the SQL batch execution succeeded.
    /// SQL 批量执行是否成功。
    pub success: bool,
    /// Human-readable batch execution message.
    /// 人类可读的批量执行结果消息。
    pub message: String,
    /// Number of changed rows reported by SQLite.
    /// SQLite 报告的受影响行数。
    pub rows_changed: i64,
    /// Last inserted row id reported by SQLite.
    /// SQLite 报告的最近插入行 ID。
    pub last_insert_rowid: i64,
    /// Number of executed parameter groups.
    /// 实际执行的参数组数量。
    pub statements_executed: i64,
}

/// Controller-side tokenizer mode shared with hosts and the service crate.
/// 向宿主与服务端 crate 共享的控制器侧分词模式。
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum ControllerSqliteTokenizerMode {
    /// Plain mode without Jieba-specific segmentation.
    /// 不带 Jieba 专项切词的普通模式。
    #[default]
    None,
    /// Jieba mode with custom-dictionary aware segmentation.
    /// 带自定义词典感知的 Jieba 模式。
    Jieba,
}

/// LanceDB column type shared by the controller client typed API.
/// 控制器客户端类型化 API 共享的 LanceDB 列类型。
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ControllerLanceDbColumnType {
    /// Unspecified column type placeholder.
    /// 未指定列类型占位符。
    Unspecified,
    /// UTF-8 string column.
    /// UTF-8 字符串列。
    String,
    /// Signed 64-bit integer column.
    /// 有符号 64 位整数列。
    Int64,
    /// 64-bit floating-point column.
    /// 64 位浮点列。
    Float64,
    /// Boolean column.
    /// 布尔列。
    Bool,
    /// Float32 vector column.
    /// Float32 向量列。
    VectorFloat32,
    /// 32-bit floating-point scalar column.
    /// 32 位浮点标量列。
    Float32,
    /// Unsigned 64-bit integer column.
    /// 无符号 64 位整数列。
    Uint64,
    /// Signed 32-bit integer column.
    /// 有符号 32 位整数列。
    Int32,
    /// Unsigned 32-bit integer column.
    /// 无符号 32 位整数列。
    Uint32,
}

/// LanceDB column definition shared by the controller client typed API.
/// 控制器客户端类型化 API 共享的 LanceDB 列定义。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ControllerLanceDbColumnDef {
    /// Stable logical column name.
    /// 稳定逻辑列名。
    pub name: String,
    /// Column type descriptor.
    /// 列类型描述。
    pub column_type: ControllerLanceDbColumnType,
    /// Vector dimension when `column_type` is vector-based.
    /// 当 `column_type` 为向量列时使用的向量维度。
    pub vector_dim: u32,
    /// Whether the column allows NULL values.
    /// 该列是否允许 NULL 值。
    pub nullable: bool,
}

/// LanceDB input format shared by the controller client typed API.
/// 控制器客户端类型化 API 共享的 LanceDB 输入格式。
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ControllerLanceDbInputFormat {
    /// Unspecified input format placeholder.
    /// 未指定输入格式占位符。
    Unspecified,
    /// JSON row-array payload.
    /// JSON 行数组载荷。
    JsonRows,
    /// Arrow IPC payload.
    /// Arrow IPC 载荷。
    ArrowIpc,
}

/// LanceDB output format shared by the controller client typed API.
/// 控制器客户端类型化 API 共享的 LanceDB 输出格式。
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ControllerLanceDbOutputFormat {
    /// Unspecified output format placeholder.
    /// 未指定输出格式占位符。
    Unspecified,
    /// Arrow IPC output payload.
    /// Arrow IPC 输出载荷。
    ArrowIpc,
    /// JSON rows output payload.
    /// JSON 行输出载荷。
    JsonRows,
}

/// One SQLite streaming-query result surfaced by the controller data plane.
/// 控制器数据面暴露的一条 SQLite 流式查询结果。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ControllerSqliteQueryStreamResult {
    /// Encoded Arrow IPC chunks emitted by the query stream path.
    /// 查询流路径产出的 Arrow IPC 编码分块。
    pub chunks: Vec<Vec<u8>>,
    /// Number of rows returned by the query.
    /// 查询返回的行数。
    pub row_count: u64,
    /// Number of emitted chunks.
    /// 实际产生的分块数量。
    pub chunk_count: u64,
    /// Total byte size across all chunks.
    /// 全部分块的总字节数。
    pub total_bytes: u64,
}

/// One SQLite scalar value used by the controller client typed API.
/// 控制器客户端类型化 API 使用的一条 SQLite 标量值。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum ControllerSqliteValue {
    /// Signed 64-bit integer value.
    /// 有符号 64 位整数值。
    Int64(i64),
    /// IEEE-754 64-bit floating-point value.
    /// IEEE-754 64 位浮点值。
    Float64(f64),
    /// UTF-8 text value.
    /// UTF-8 文本值。
    String(String),
    /// Raw byte-array value.
    /// 原始字节数组值。
    Bytes(Vec<u8>),
    /// Boolean value mapped onto SQLite integer semantics.
    /// 映射到 SQLite 整数语义的布尔值。
    Bool(bool),
    /// Explicit SQLite NULL value.
    /// 显式 SQLite NULL 值。
    Null,
}

/// One SQLite tokenization result surfaced by the controller data plane.
/// 控制器数据面暴露的一条 SQLite 分词结果。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ControllerSqliteTokenizeResult {
    /// Effective tokenizer mode used by the engine.
    /// 引擎实际使用的分词模式。
    pub tokenizer_mode: String,
    /// Lightly normalized input text.
    /// 轻量规范化后的输入文本。
    pub normalized_text: String,
    /// Token list after segmentation.
    /// 切分后的词元列表。
    pub tokens: Vec<String>,
    /// Safe FTS MATCH expression derived from the tokens.
    /// 根据词元生成的安全 FTS MATCH 表达式。
    pub fts_query: String,
}

/// One SQLite custom dictionary entry exposed to hosts.
/// 暴露给宿主的一条 SQLite 自定义词典条目。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ControllerSqliteCustomWordEntry {
    /// Custom word text.
    /// 自定义词文本。
    pub word: String,
    /// Custom word weight.
    /// 自定义词权重。
    pub weight: usize,
}

/// One SQLite dictionary mutation result surfaced by the controller data plane.
/// 控制器数据面暴露的一条 SQLite 词典变更结果。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ControllerSqliteDictionaryMutationResult {
    /// Whether the dictionary mutation succeeded.
    /// 词典变更是否成功。
    pub success: bool,
    /// Human-readable mutation message.
    /// 人类可读的变更结果消息。
    pub message: String,
    /// Number of affected rows.
    /// 受影响行数。
    pub affected_rows: u64,
}

/// One SQLite custom-dictionary listing result surfaced by the controller data plane.
/// 控制器数据面暴露的一条 SQLite 自定义词典列表结果。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ControllerSqliteListCustomWordsResult {
    /// Whether the listing succeeded.
    /// 列表读取是否成功。
    pub success: bool,
    /// Human-readable listing message.
    /// 人类可读的列表结果消息。
    pub message: String,
    /// Current enabled custom words.
    /// 当前启用的自定义词条目。
    pub words: Vec<ControllerSqliteCustomWordEntry>,
}

/// One SQLite FTS ensure-index result surfaced by the controller data plane.
/// 控制器数据面暴露的一条 SQLite FTS 索引确认结果。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ControllerSqliteEnsureFtsIndexResult {
    /// Whether the ensure-index operation succeeded.
    /// 索引确认操作是否成功。
    pub success: bool,
    /// Human-readable ensure-index message.
    /// 人类可读的索引确认消息。
    pub message: String,
    /// Effective sanitized index name.
    /// 实际生效的规范化索引名。
    pub index_name: String,
    /// Effective tokenizer mode name.
    /// 实际生效的分词模式名称。
    pub tokenizer_mode: String,
}

/// One SQLite FTS rebuild result surfaced by the controller data plane.
/// 控制器数据面暴露的一条 SQLite FTS 重建结果。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ControllerSqliteRebuildFtsIndexResult {
    /// Whether the rebuild operation succeeded.
    /// 索引重建操作是否成功。
    pub success: bool,
    /// Human-readable rebuild message.
    /// 人类可读的索引重建消息。
    pub message: String,
    /// Effective sanitized index name.
    /// 实际生效的规范化索引名。
    pub index_name: String,
    /// Effective tokenizer mode name.
    /// 实际生效的分词模式名称。
    pub tokenizer_mode: String,
    /// Number of rows reindexed during rebuild.
    /// 重建过程中重新索引的行数。
    pub reindexed_rows: u64,
}

/// One SQLite FTS document mutation result surfaced by the controller data plane.
/// 控制器数据面暴露的一条 SQLite FTS 文档变更结果。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ControllerSqliteFtsMutationResult {
    /// Whether the FTS document mutation succeeded.
    /// FTS 文档变更是否成功。
    pub success: bool,
    /// Human-readable mutation message.
    /// 人类可读的变更结果消息。
    pub message: String,
    /// Number of affected rows.
    /// 受影响行数。
    pub affected_rows: u64,
    /// Effective index name.
    /// 实际生效的索引名。
    pub index_name: String,
}

/// One SQLite FTS search hit surfaced by the controller data plane.
/// 控制器数据面暴露的一条 SQLite FTS 检索命中记录。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ControllerSqliteSearchFtsHit {
    /// Business identifier.
    /// 业务标识符。
    pub id: String,
    /// File path or logical path.
    /// 文件路径或逻辑路径。
    pub file_path: String,
    /// Raw title field.
    /// 原始标题字段。
    pub title: String,
    /// Highlighted title field.
    /// 带高亮的标题字段。
    pub title_highlight: String,
    /// Highlighted content snippet.
    /// 带高亮的正文片段。
    pub content_snippet: String,
    /// Normalized score where higher is better.
    /// 归一化分数，约定越大越好。
    pub score: f64,
    /// Rank inside the current result set.
    /// 当前结果集中的排序名次。
    pub rank: u64,
    /// Raw SQLite bm25 score.
    /// SQLite bm25 原始分数。
    pub raw_score: f64,
}

/// One SQLite FTS search result surfaced by the controller data plane.
/// 控制器数据面暴露的一条 SQLite FTS 检索结果。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ControllerSqliteSearchFtsResult {
    /// Whether the FTS search succeeded.
    /// FTS 检索是否成功。
    pub success: bool,
    /// Human-readable search message.
    /// 人类可读的检索结果消息。
    pub message: String,
    /// Effective index name.
    /// 实际生效的索引名。
    pub index_name: String,
    /// Effective tokenizer mode name.
    /// 实际生效的分词模式名称。
    pub tokenizer_mode: String,
    /// Normalized query text.
    /// 规范化后的查询文本。
    pub normalized_query: String,
    /// Final MATCH expression.
    /// 最终的 MATCH 表达式。
    pub fts_query: String,
    /// Search source label.
    /// 检索来源标签。
    pub source: String,
    /// Query mode label.
    /// 查询模式标签。
    pub query_mode: String,
    /// Total hit count.
    /// 总命中数。
    pub total: u64,
    /// Search hits.
    /// 检索命中列表。
    pub hits: Vec<ControllerSqliteSearchFtsHit>,
}

/// One LanceDB create-table result surfaced by the controller data plane.
/// 控制器数据面暴露的一条 LanceDB 建表结果。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ControllerLanceDbCreateTableResult {
    /// Human-readable create-table result message.
    /// 人类可读的建表结果消息。
    pub message: String,
}

/// One LanceDB upsert result surfaced by the controller data plane.
/// 控制器数据面暴露的一条 LanceDB 写入结果。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ControllerLanceDbUpsertResult {
    /// Human-readable upsert result message.
    /// 人类可读的写入结果消息。
    pub message: String,
    /// Resulting table version after the write.
    /// 写入完成后的表版本号。
    pub version: u64,
    /// Number of input rows decoded from the payload.
    /// 从载荷解码出的输入行数。
    pub input_rows: u64,
    /// Number of inserted rows.
    /// 插入的行数。
    pub inserted_rows: u64,
    /// Number of updated rows.
    /// 更新的行数。
    pub updated_rows: u64,
    /// Number of deleted rows reported by the merge path.
    /// merge 路径报告的删除行数。
    pub deleted_rows: u64,
}

/// One LanceDB search result surfaced by the controller data plane.
/// 控制器数据面暴露的一条 LanceDB 检索结果。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ControllerLanceDbSearchResult {
    /// Human-readable search result message.
    /// 人类可读的检索结果消息。
    pub message: String,
    /// Effective output format name.
    /// 生效的输出格式名称。
    pub format: String,
    /// Number of rows returned by the search.
    /// 检索返回的行数。
    pub rows: u64,
    /// Encoded result payload bytes.
    /// 编码后的结果载荷字节。
    pub data: Vec<u8>,
}

/// One LanceDB delete result surfaced by the controller data plane.
/// 控制器数据面暴露的一条 LanceDB 删除结果。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ControllerLanceDbDeleteResult {
    /// Human-readable delete result message.
    /// 人类可读的删除结果消息。
    pub message: String,
    /// Resulting table version after the delete.
    /// 删除完成后的表版本号。
    pub version: u64,
    /// Number of deleted rows.
    /// 删除的行数。
    pub deleted_rows: u64,
}

/// One LanceDB drop-table result surfaced by the controller data plane.
/// 控制器数据面暴露的一条 LanceDB 删表结果。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ControllerLanceDbDropTableResult {
    /// Human-readable drop-table result message.
    /// 人类可读的删表结果消息。
    pub message: String,
}

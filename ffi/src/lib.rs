use std::ffi::{CStr, CString, c_char, c_float, c_int, c_uchar, c_ulonglong};
use std::ptr;

use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use tokio::runtime::Runtime;
use vldb_controller_client::{
    ClientRegistration, ControllerClient, ControllerClientConfig, ControllerLanceDbColumnDef,
    ControllerLanceDbColumnType, ControllerLanceDbCreateTableResult, ControllerLanceDbDeleteResult,
    ControllerLanceDbDropTableResult, ControllerLanceDbEnableRequest, ControllerLanceDbInputFormat,
    ControllerLanceDbOutputFormat, ControllerLanceDbSearchResult, ControllerLanceDbUpsertResult,
    ControllerProcessMode, ControllerSqliteCustomWordEntry,
    ControllerSqliteDictionaryMutationResult, ControllerSqliteEnableRequest,
    ControllerSqliteEnsureFtsIndexResult, ControllerSqliteExecuteBatchResult,
    ControllerSqliteExecuteResult, ControllerSqliteFtsMutationResult,
    ControllerSqliteListCustomWordsResult, ControllerSqliteQueryResult,
    ControllerSqliteRebuildFtsIndexResult, ControllerSqliteSearchFtsHit,
    ControllerSqliteSearchFtsResult, ControllerSqliteTokenizeResult, ControllerSqliteTokenizerMode,
    ControllerSqliteValue, ControllerStatusSnapshot, SpaceBackendStatus, SpaceKind,
    SpaceRegistration, SpaceSnapshot,
};

/// Stable success status returned by all native FFI functions.
/// 所有原生 FFI 函数返回的稳定成功状态码。
const FFI_STATUS_OK: c_int = 0;

/// Stable failure status returned by all native FFI functions.
/// 所有原生 FFI 函数返回的稳定失败状态码。
const FFI_STATUS_ERR: c_int = 1;

/// Opaque FFI handle that owns one Tokio runtime and one controller client proxy.
/// 拥有一个 Tokio 运行时和一个控制器客户端代理的不透明 FFI 句柄。
pub struct FfiControllerClientHandle {
    runtime: Runtime,
    client: ControllerClient,
}

/// Native client configuration used by hosts that prefer structured ABI calls.
/// 供偏好结构化 ABI 调用的宿主使用的原生客户端配置。
#[repr(C)]
pub struct FfiControllerClientConfig {
    pub endpoint: *const c_char,
    pub auto_spawn: c_uchar,
    pub spawn_executable: *const c_char,
    pub spawn_process_mode: c_int,
    pub minimum_uptime_secs: c_ulonglong,
    pub idle_timeout_secs: c_ulonglong,
    pub default_lease_ttl_secs: c_ulonglong,
    pub connect_timeout_secs: c_ulonglong,
    pub startup_timeout_secs: c_ulonglong,
    pub startup_retry_interval_ms: c_ulonglong,
    pub lease_renew_interval_secs: c_ulonglong,
}

/// Native client registration used by hosts that prefer structured ABI calls.
/// 供偏好结构化 ABI 调用的宿主使用的原生客户端注册结构。
#[repr(C)]
pub struct FfiClientRegistration {
    pub client_id: *const c_char,
    pub host_kind: *const c_char,
    pub process_id: u32,
    pub process_name: *const c_char,
    pub lease_ttl_secs: c_ulonglong,
}

/// Native runtime space registration used by structured ABI calls.
/// 供结构化 ABI 调用使用的原生运行时空间注册结构。
#[repr(C)]
pub struct FfiSpaceRegistration {
    pub space_id: *const c_char,
    pub space_label: *const c_char,
    pub space_kind: c_int,
    pub space_root: *const c_char,
}

/// Native SQLite enable request used by structured ABI calls.
/// 供结构化 ABI 调用使用的原生 SQLite 启用请求。
#[repr(C)]
pub struct FfiControllerSqliteEnableRequest {
    pub space_id: *const c_char,
    pub db_path: *const c_char,
    pub connection_pool_size: c_ulonglong,
    pub busy_timeout_ms: c_ulonglong,
    pub journal_mode: *const c_char,
    pub synchronous: *const c_char,
    pub foreign_keys: c_uchar,
    pub temp_store: *const c_char,
    pub wal_autocheckpoint_pages: u32,
    pub cache_size_kib: i64,
    pub mmap_size_bytes: c_ulonglong,
    pub enforce_db_file_lock: c_uchar,
    pub read_only: c_uchar,
    pub allow_uri_filenames: c_uchar,
    pub trusted_schema: c_uchar,
    pub defensive: c_uchar,
}

/// Native LanceDB enable request used by structured ABI calls.
/// 供结构化 ABI 调用使用的原生 LanceDB 启用请求。
#[repr(C)]
pub struct FfiControllerLanceDbEnableRequest {
    pub space_id: *const c_char,
    pub default_db_path: *const c_char,
    pub db_root: *const c_char,
    pub read_consistency_interval_ms: c_ulonglong,
    pub max_upsert_payload: c_ulonglong,
    pub max_search_limit: c_ulonglong,
    pub max_concurrent_requests: c_ulonglong,
}

/// Native backend status snapshot returned by structured ABI calls.
/// 由结构化 ABI 调用返回的原生后端状态快照。
#[repr(C)]
pub struct FfiSpaceBackendStatus {
    pub enabled: c_uchar,
    pub mode: *mut c_char,
    pub target: *mut c_char,
}

/// Native space snapshot returned by structured ABI calls.
/// 由结构化 ABI 调用返回的原生空间快照。
#[repr(C)]
pub struct FfiSpaceSnapshot {
    pub space_id: *mut c_char,
    pub space_label: *mut c_char,
    pub space_kind: c_int,
    pub space_root: *mut c_char,
    pub attached_clients: c_ulonglong,
    pub sqlite: *mut FfiSpaceBackendStatus,
    pub lancedb: *mut FfiSpaceBackendStatus,
}

/// Native array wrapper for space snapshots.
/// 用于包装空间快照数组的原生结构。
#[repr(C)]
pub struct FfiSpaceSnapshotArray {
    pub items: *mut FfiSpaceSnapshot,
    pub len: usize,
}

/// Native controller status snapshot returned by structured ABI calls.
/// 由结构化 ABI 调用返回的原生控制器状态快照。
#[repr(C)]
pub struct FfiControllerStatusSnapshot {
    pub process_mode: c_int,
    pub bind_addr: *mut c_char,
    pub started_at_unix_ms: c_ulonglong,
    pub last_request_at_unix_ms: c_ulonglong,
    pub minimum_uptime_secs: c_ulonglong,
    pub idle_timeout_secs: c_ulonglong,
    pub default_lease_ttl_secs: c_ulonglong,
    pub active_clients: c_ulonglong,
    pub attached_spaces: c_ulonglong,
    pub inflight_requests: c_ulonglong,
    pub shutdown_candidate: c_uchar,
}

/// Native SQLite execution result returned by structured ABI calls.
/// 由结构化 ABI 调用返回的原生 SQLite 执行结果。
#[repr(C)]
pub struct FfiControllerSqliteExecuteResult {
    pub success: c_uchar,
    pub message: *mut c_char,
    pub rows_changed: i64,
    pub last_insert_rowid: i64,
}

/// Native SQLite JSON query result returned by structured ABI calls.
/// 由结构化 ABI 调用返回的原生 SQLite JSON 查询结果。
#[repr(C)]
pub struct FfiControllerSqliteQueryResult {
    pub json_data: *mut c_char,
    pub row_count: c_ulonglong,
}

/// Native byte-buffer item returned by structured ABI calls.
/// 由结构化 ABI 调用返回的原生字节缓冲项。
#[repr(C)]
pub struct FfiByteBuffer {
    pub data: *mut u8,
    pub len: usize,
}

/// Native byte-buffer array returned by structured ABI calls.
/// 由结构化 ABI 调用返回的原生字节缓冲数组。
#[repr(C)]
pub struct FfiByteBufferArray {
    pub items: *mut FfiByteBuffer,
    pub len: usize,
}

/// Native string array used by structured ABI calls.
/// 供结构化 ABI 调用使用的原生字符串数组。
#[repr(C)]
pub struct FfiStringArray {
    pub items: *const *const c_char,
    pub len: usize,
}

/// Native SQLite value used by structured ABI calls.
/// 供结构化 ABI 调用使用的原生 SQLite 值。
#[repr(C)]
pub struct FfiSqliteValue {
    pub kind: c_int,
    pub int64_value: i64,
    pub float64_value: f64,
    pub string_value: *const c_char,
    pub bytes_value: *const u8,
    pub bytes_len: usize,
    pub bool_value: c_uchar,
}

/// Native SQLite batch item used by structured ABI calls.
/// 供结构化 ABI 调用使用的原生 SQLite 批量参数项。
#[repr(C)]
pub struct FfiSqliteBatchItem {
    pub params: *const FfiSqliteValue,
    pub params_len: usize,
}

/// Native SQLite batch execution result returned by structured ABI calls.
/// 由结构化 ABI 调用返回的原生 SQLite 批量执行结果。
#[repr(C)]
pub struct FfiControllerSqliteExecuteBatchResult {
    pub success: c_uchar,
    pub message: *mut c_char,
    pub rows_changed: i64,
    pub last_insert_rowid: i64,
    pub statements_executed: i64,
}

/// Native SQLite streaming query result returned by structured ABI calls.
/// 由结构化 ABI 调用返回的原生 SQLite 流式查询结果。
#[repr(C)]
pub struct FfiControllerSqliteQueryStreamResult {
    pub chunks: *mut FfiByteBufferArray,
    pub row_count: c_ulonglong,
    pub chunk_count: c_ulonglong,
    pub total_bytes: c_ulonglong,
}

/// Native SQLite tokenize result returned by structured ABI calls.
/// 由结构化 ABI 调用返回的原生 SQLite 分词结果。
#[repr(C)]
pub struct FfiControllerSqliteTokenizeResult {
    pub tokenizer_mode: *mut c_char,
    pub normalized_text: *mut c_char,
    pub tokens_json: *mut c_char,
    pub fts_query: *mut c_char,
}

/// Native SQLite custom-word entry returned by structured ABI calls.
/// 由结构化 ABI 调用返回的原生 SQLite 自定义词条目。
#[repr(C)]
pub struct FfiControllerSqliteCustomWordEntry {
    pub word: *mut c_char,
    pub weight: c_ulonglong,
}

/// Native SQLite custom-word array returned by structured ABI calls.
/// 由结构化 ABI 调用返回的原生 SQLite 自定义词数组。
#[repr(C)]
pub struct FfiControllerSqliteCustomWordArray {
    pub items: *mut FfiControllerSqliteCustomWordEntry,
    pub len: usize,
}

/// Native SQLite dictionary mutation result returned by structured ABI calls.
/// 由结构化 ABI 调用返回的原生 SQLite 词典变更结果。
#[repr(C)]
pub struct FfiControllerSqliteDictionaryMutationResult {
    pub success: c_uchar,
    pub message: *mut c_char,
    pub affected_rows: c_ulonglong,
}

/// Native SQLite custom-word listing result returned by structured ABI calls.
/// 由结构化 ABI 调用返回的原生 SQLite 自定义词列表结果。
#[repr(C)]
pub struct FfiControllerSqliteListCustomWordsResult {
    pub success: c_uchar,
    pub message: *mut c_char,
    pub words: *mut FfiControllerSqliteCustomWordArray,
}

/// Native SQLite ensure-FTS result returned by structured ABI calls.
/// 由结构化 ABI 调用返回的原生 SQLite FTS 确认结果。
#[repr(C)]
pub struct FfiControllerSqliteEnsureFtsIndexResult {
    pub success: c_uchar,
    pub message: *mut c_char,
    pub index_name: *mut c_char,
    pub tokenizer_mode: *mut c_char,
}

/// Native SQLite rebuild-FTS result returned by structured ABI calls.
/// 由结构化 ABI 调用返回的原生 SQLite FTS 重建结果。
#[repr(C)]
pub struct FfiControllerSqliteRebuildFtsIndexResult {
    pub success: c_uchar,
    pub message: *mut c_char,
    pub index_name: *mut c_char,
    pub tokenizer_mode: *mut c_char,
    pub reindexed_rows: c_ulonglong,
}

/// Native SQLite FTS document mutation result returned by structured ABI calls.
/// 由结构化 ABI 调用返回的原生 SQLite FTS 文档变更结果。
#[repr(C)]
pub struct FfiControllerSqliteFtsMutationResult {
    pub success: c_uchar,
    pub message: *mut c_char,
    pub affected_rows: c_ulonglong,
    pub index_name: *mut c_char,
}

/// Native SQLite FTS search hit returned by structured ABI calls.
/// 由结构化 ABI 调用返回的原生 SQLite FTS 检索命中。
#[repr(C)]
pub struct FfiControllerSqliteSearchFtsHit {
    pub id: *mut c_char,
    pub file_path: *mut c_char,
    pub title: *mut c_char,
    pub title_highlight: *mut c_char,
    pub content_snippet: *mut c_char,
    pub score: f64,
    pub rank: c_ulonglong,
    pub raw_score: f64,
}

/// Native SQLite FTS hit array returned by structured ABI calls.
/// 由结构化 ABI 调用返回的原生 SQLite FTS 命中数组。
#[repr(C)]
pub struct FfiControllerSqliteSearchFtsHitArray {
    pub items: *mut FfiControllerSqliteSearchFtsHit,
    pub len: usize,
}

/// Native SQLite FTS search result returned by structured ABI calls.
/// 由结构化 ABI 调用返回的原生 SQLite FTS 检索结果。
#[repr(C)]
pub struct FfiControllerSqliteSearchFtsResult {
    pub success: c_uchar,
    pub message: *mut c_char,
    pub index_name: *mut c_char,
    pub tokenizer_mode: *mut c_char,
    pub normalized_query: *mut c_char,
    pub fts_query: *mut c_char,
    pub source: *mut c_char,
    pub query_mode: *mut c_char,
    pub total: c_ulonglong,
    pub hits: *mut FfiControllerSqliteSearchFtsHitArray,
}

/// Native LanceDB create-table result returned by structured ABI calls.
/// 由结构化 ABI 调用返回的原生 LanceDB 建表结果。
#[repr(C)]
pub struct FfiControllerLanceDbCreateTableResult {
    pub message: *mut c_char,
}

/// Native LanceDB upsert result returned by structured ABI calls.
/// 由结构化 ABI 调用返回的原生 LanceDB 写入结果。
#[repr(C)]
pub struct FfiControllerLanceDbUpsertResult {
    pub message: *mut c_char,
    pub version: c_ulonglong,
    pub input_rows: c_ulonglong,
    pub inserted_rows: c_ulonglong,
    pub updated_rows: c_ulonglong,
    pub deleted_rows: c_ulonglong,
}

/// Native LanceDB search result returned by structured ABI calls.
/// 由结构化 ABI 调用返回的原生 LanceDB 检索结果。
#[repr(C)]
pub struct FfiControllerLanceDbSearchResult {
    pub message: *mut c_char,
    pub format: *mut c_char,
    pub rows: c_ulonglong,
    pub data: *mut u8,
    pub data_len: usize,
}

/// Native LanceDB delete result returned by structured ABI calls.
/// 由结构化 ABI 调用返回的原生 LanceDB 删除结果。
#[repr(C)]
pub struct FfiControllerLanceDbDeleteResult {
    pub message: *mut c_char,
    pub version: c_ulonglong,
    pub deleted_rows: c_ulonglong,
}

/// Native LanceDB drop-table result returned by structured ABI calls.
/// 由结构化 ABI 调用返回的原生 LanceDB 删表结果。
#[repr(C)]
pub struct FfiControllerLanceDbDropTableResult {
    pub message: *mut c_char,
}

/// Native LanceDB column definition used by structured ABI calls.
/// 供结构化 ABI 调用使用的原生 LanceDB 列定义。
#[repr(C)]
pub struct FfiControllerLanceDbColumnDef {
    pub name: *const c_char,
    pub column_type: c_int,
    pub vector_dim: u32,
    pub nullable: c_uchar,
}

/// JSON request used by `client_create_json`.
/// `client_create_json` 使用的 JSON 请求结构。
#[derive(Deserialize)]
struct CreateClientJsonRequest {
    config: ControllerClientConfig,
    registration: ClientRegistration,
}

/// JSON request used by `connect_json`.
/// `connect_json` 使用的 JSON 请求结构。
#[derive(Default, Deserialize)]
struct EmptyJsonRequest {}

/// JSON request used by `attach_space_json`.
/// `attach_space_json` 使用的 JSON 请求结构。
#[derive(Deserialize)]
struct AttachSpaceJsonRequest {
    registration: SpaceRegistration,
}

/// JSON request used by `detach_space_json`.
/// `detach_space_json` 使用的 JSON 请求结构。
#[derive(Deserialize)]
struct DetachSpaceJsonRequest {
    space_id: String,
}

/// JSON request used by `enable_sqlite_json`.
/// `enable_sqlite_json` 使用的 JSON 请求结构。
#[derive(Deserialize)]
struct EnableSqliteJsonRequest {
    request: ControllerSqliteEnableRequest,
}

/// JSON request used by `disable_backend_json`.
/// `disable_backend_json` 使用的 JSON 请求结构。
#[derive(Deserialize)]
struct DisableBackendJsonRequest {
    space_id: String,
}

/// JSON request used by `execute_sqlite_script_json`.
/// `execute_sqlite_script_json` 使用的 JSON 请求结构。
#[derive(Deserialize)]
struct ExecuteSqliteScriptJsonRequest {
    space_id: String,
    sql: String,
    #[serde(default)]
    params: Vec<JsonValue>,
}

/// JSON request used by `query_sqlite_json_json`.
/// `query_sqlite_json_json` 使用的 JSON 请求结构。
#[derive(Deserialize)]
struct QuerySqliteJsonJsonRequest {
    space_id: String,
    sql: String,
    #[serde(default)]
    params: Vec<JsonValue>,
}

/// JSON request used by `execute_sqlite_batch_json`.
/// `execute_sqlite_batch_json` 使用的 JSON 请求结构。
#[derive(Deserialize)]
struct ExecuteSqliteBatchJsonRequest {
    space_id: String,
    sql: String,
    #[serde(default)]
    batch_params: Vec<Vec<JsonValue>>,
}

/// JSON request used by `query_sqlite_stream_json`.
/// `query_sqlite_stream_json` 使用的 JSON 请求结构。
#[derive(Deserialize)]
struct QuerySqliteStreamJsonRequest {
    space_id: String,
    sql: String,
    #[serde(default)]
    params: Vec<JsonValue>,
    target_chunk_size: Option<u64>,
}

/// JSON request used by `tokenize_sqlite_text_json`.
/// `tokenize_sqlite_text_json` 使用的 JSON 请求结构。
#[derive(Deserialize)]
struct TokenizeSqliteTextJsonRequest {
    space_id: String,
    tokenizer_mode: String,
    text: String,
    search_mode: bool,
}

/// JSON request used by `list_sqlite_custom_words_json`.
/// `list_sqlite_custom_words_json` 使用的 JSON 请求结构。
#[derive(Deserialize)]
struct ListSqliteCustomWordsJsonRequest {
    space_id: String,
}

/// JSON request used by `upsert_sqlite_custom_word_json`.
/// `upsert_sqlite_custom_word_json` 使用的 JSON 请求结构。
#[derive(Deserialize)]
struct UpsertSqliteCustomWordJsonRequest {
    space_id: String,
    word: String,
    weight: u32,
}

/// JSON request used by `remove_sqlite_custom_word_json`.
/// `remove_sqlite_custom_word_json` 使用的 JSON 请求结构。
#[derive(Deserialize)]
struct RemoveSqliteCustomWordJsonRequest {
    space_id: String,
    word: String,
}

/// JSON request used by `ensure_sqlite_fts_index_json`.
/// `ensure_sqlite_fts_index_json` 使用的 JSON 请求结构。
#[derive(Deserialize)]
struct EnsureSqliteFtsIndexJsonRequest {
    space_id: String,
    index_name: String,
    tokenizer_mode: String,
}

/// JSON request used by `rebuild_sqlite_fts_index_json`.
/// `rebuild_sqlite_fts_index_json` 使用的 JSON 请求结构。
#[derive(Deserialize)]
struct RebuildSqliteFtsIndexJsonRequest {
    space_id: String,
    index_name: String,
    tokenizer_mode: String,
}

/// JSON request used by `upsert_sqlite_fts_document_json`.
/// `upsert_sqlite_fts_document_json` 使用的 JSON 请求结构。
#[derive(Deserialize)]
struct UpsertSqliteFtsDocumentJsonRequest {
    space_id: String,
    index_name: String,
    tokenizer_mode: String,
    id: String,
    file_path: String,
    title: String,
    content: String,
}

/// JSON request used by `delete_sqlite_fts_document_json`.
/// `delete_sqlite_fts_document_json` 使用的 JSON 请求结构。
#[derive(Deserialize)]
struct DeleteSqliteFtsDocumentJsonRequest {
    space_id: String,
    index_name: String,
    id: String,
}

/// JSON request used by `search_sqlite_fts_json`.
/// `search_sqlite_fts_json` 使用的 JSON 请求结构。
#[derive(Deserialize)]
struct SearchSqliteFtsJsonRequest {
    space_id: String,
    index_name: String,
    tokenizer_mode: String,
    query: String,
    limit: u32,
    offset: u32,
}

/// JSON request used by `enable_lancedb_json`.
/// `enable_lancedb_json` 使用的 JSON 请求结构。
#[derive(Deserialize)]
struct EnableLanceDbJsonRequest {
    request: ControllerLanceDbEnableRequest,
}

/// JSON request used by `create_lancedb_table_json`.
/// `create_lancedb_table_json` 使用的 JSON 请求结构。
#[derive(Deserialize)]
struct CreateLanceDbTableJsonRequest {
    space_id: String,
    table_name: String,
    columns: Vec<CreateLanceDbTableJsonColumn>,
    #[serde(default)]
    overwrite_if_exists: bool,
}

/// JSON create-table column used by FFI JSON mode.
/// FFI JSON 模式使用的建表列定义。
#[derive(Deserialize, Serialize)]
struct CreateLanceDbTableJsonColumn {
    name: String,
    column_type: String,
    #[serde(default)]
    vector_dim: u32,
    #[serde(default = "default_nullable")]
    nullable: bool,
}

/// JSON request used by `upsert_lancedb_json`.
/// `upsert_lancedb_json` 使用的 JSON 请求结构。
#[derive(Deserialize)]
struct UpsertLanceDbJsonRequest {
    space_id: String,
    table_name: String,
    input_format: String,
    #[serde(default)]
    key_columns: Vec<String>,
    data_base64: String,
}

/// JSON request used by `search_lancedb_json`.
/// `search_lancedb_json` 使用的 JSON 请求结构。
#[derive(Deserialize)]
struct SearchLanceDbJsonRequest {
    space_id: String,
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

/// JSON request used by `delete_lancedb_json`.
/// `delete_lancedb_json` 使用的 JSON 请求结构。
#[derive(Deserialize)]
struct DeleteLanceDbJsonRequest {
    space_id: String,
    table_name: String,
    condition: String,
}

/// JSON bytes wrapper used by SQLite JSON-mode parameters.
/// SQLite JSON 模式参数使用的 JSON 字节包装结构。
#[derive(Deserialize)]
struct JsonSqliteBytesValue {
    #[serde(default)]
    r#type: String,
    #[serde(default)]
    __type: String,
    base64: String,
}

/// Compatibility JSON create-table payload used internally by the Rust SDK wrapper.
/// 供 Rust SDK 兼容包装层内部使用的 JSON 建表负载。
#[derive(Serialize)]
struct CreateLanceDbTableJsonCompatRequest {
    table_name: String,
    columns: Vec<CreateLanceDbTableJsonColumn>,
    overwrite_if_exists: bool,
}

impl From<CreateLanceDbTableJsonRequest> for CreateLanceDbTableJsonCompatRequest {
    fn from(value: CreateLanceDbTableJsonRequest) -> Self {
        Self {
            table_name: value.table_name,
            columns: value.columns,
            overwrite_if_exists: value.overwrite_if_exists,
        }
    }
}

/// Compatibility JSON upsert payload used internally by the Rust SDK wrapper.
/// 供 Rust SDK 兼容包装层内部使用的 JSON 写入负载。
#[derive(Serialize)]
struct UpsertLanceDbJsonCompatRequest {
    table_name: String,
    input_format: String,
    key_columns: Vec<String>,
}

impl From<&UpsertLanceDbJsonRequest> for UpsertLanceDbJsonCompatRequest {
    fn from(value: &UpsertLanceDbJsonRequest) -> Self {
        Self {
            table_name: value.table_name.clone(),
            input_format: value.input_format.clone(),
            key_columns: value.key_columns.clone(),
        }
    }
}

/// Compatibility JSON search payload used internally by the Rust SDK wrapper.
/// 供 Rust SDK 兼容包装层内部使用的 JSON 检索负载。
#[derive(Serialize)]
struct SearchLanceDbJsonCompatRequest {
    table_name: String,
    vector: Vec<f32>,
    limit: u32,
    filter: String,
    vector_column: String,
    output_format: String,
}

impl From<&SearchLanceDbJsonRequest> for SearchLanceDbJsonCompatRequest {
    fn from(value: &SearchLanceDbJsonRequest) -> Self {
        Self {
            table_name: value.table_name.clone(),
            vector: value.vector.clone(),
            limit: value.limit,
            filter: value.filter.clone(),
            vector_column: value.vector_column.clone(),
            output_format: value.output_format.clone(),
        }
    }
}

/// Compatibility JSON delete payload used internally by the Rust SDK wrapper.
/// 供 Rust SDK 兼容包装层内部使用的 JSON 删除负载。
#[derive(Serialize)]
struct DeleteLanceDbJsonCompatRequest {
    table_name: String,
    condition: String,
}

impl From<&DeleteLanceDbJsonRequest> for DeleteLanceDbJsonCompatRequest {
    fn from(value: &DeleteLanceDbJsonRequest) -> Self {
        Self {
            table_name: value.table_name.clone(),
            condition: value.condition.clone(),
        }
    }
}

/// JSON request used by `drop_lancedb_table_json`.
/// `drop_lancedb_table_json` 使用的 JSON 请求结构。
#[derive(Deserialize)]
struct DropLanceDbTableJsonRequest {
    space_id: String,
    table_name: String,
}

/// JSON response used by empty-result operations.
/// 用于空结果操作的 JSON 响应结构。
#[derive(Serialize)]
struct SuccessJsonResponse {
    ok: bool,
}

/// JSON-friendly SQLite query-stream response that encodes chunks as base64 text.
/// 将分块编码成 base64 文本的 JSON 友好 SQLite 流式查询响应。
#[derive(Serialize)]
struct QuerySqliteStreamJsonResponse {
    chunks_base64: Vec<String>,
    row_count: u64,
    chunk_count: u64,
    total_bytes: u64,
}

/// Internal SQLite stream compatibility result used by the current FFI layer.
/// 当前 FFI 层使用的内部 SQLite 流式兼容结果。
struct ControllerSqliteQueryStreamCompatResult {
    chunks: Vec<Vec<u8>>,
    row_count: u64,
    chunk_count: u64,
    total_bytes: u64,
}

/// Return the FFI package version.
/// 返回 FFI 包版本号。
#[unsafe(no_mangle)]
pub extern "C" fn vldb_controller_ffi_version() -> *mut c_char {
    string_into_raw(env!("CARGO_PKG_VERSION").to_string())
}

/// Free one string allocated by the FFI layer.
/// 释放一条由 FFI 层分配的字符串。
#[unsafe(no_mangle)]
pub extern "C" fn vldb_controller_ffi_string_free(value: *mut c_char) {
    if !value.is_null() {
        unsafe {
            drop(CString::from_raw(value));
        }
    }
}

/// Free one byte buffer allocated by the FFI layer.
/// 释放一段由 FFI 层分配的字节缓冲区。
#[unsafe(no_mangle)]
pub extern "C" fn vldb_controller_ffi_bytes_free(data: *mut u8, len: usize) {
    if !data.is_null() {
        unsafe {
            drop(Vec::from_raw_parts(data, len, len));
        }
    }
}

/// Create one controller client handle from native input structures.
/// 从原生输入结构创建一个控制器客户端句柄。
#[unsafe(no_mangle)]
pub extern "C" fn vldb_controller_ffi_client_create(
    config: *const FfiControllerClientConfig,
    registration: *const FfiClientRegistration,
    client_out: *mut *mut FfiControllerClientHandle,
    error_out: *mut *mut c_char,
) -> c_int {
    clear_out_ptr(client_out);
    match native_client_create(config, registration) {
        Ok(handle) => {
            write_out_ptr(client_out, Box::into_raw(Box::new(handle)));
            ffi_ok_status(error_out)
        }
        Err(error) => ffi_error_status(error_out, error),
    }
}

/// Create one controller client handle from JSON input payloads.
/// 从 JSON 输入载荷创建一个控制器客户端句柄。
#[unsafe(no_mangle)]
pub extern "C" fn vldb_controller_ffi_client_create_json(
    request_json: *const c_char,
    client_out: *mut *mut FfiControllerClientHandle,
    response_out: *mut *mut c_char,
    error_out: *mut *mut c_char,
) -> c_int {
    clear_out_ptr(client_out);
    clear_out_ptr(response_out);
    let result = (|| -> Result<(*mut FfiControllerClientHandle, String), String> {
        let request: CreateClientJsonRequest = parse_json_input(request_json)?;
        let handle = build_client_handle(request.config, request.registration)?;
        let response = serde_json::to_string(&SuccessJsonResponse { ok: true })
            .map_err(|error| format!("failed to serialize create_client_json response: {error}"))?;
        Ok((Box::into_raw(Box::new(handle)), response))
    })();

    match result {
        Ok((handle, response)) => {
            write_out_ptr(client_out, handle);
            write_out_ptr(response_out, string_into_raw(response));
            ffi_ok_status(error_out)
        }
        Err(error) => ffi_error_status(error_out, error),
    }
}

/// Free one controller client handle created by the FFI layer.
/// 释放一个由 FFI 层创建的控制器客户端句柄。
#[unsafe(no_mangle)]
pub extern "C" fn vldb_controller_ffi_client_free(client: *mut FfiControllerClientHandle) {
    if !client.is_null() {
        let handle = unsafe { Box::from_raw(client) };
        let _ = handle.runtime.block_on(handle.client.shutdown());
        drop(handle);
    }
}

/// Connect one controller client and start lease maintenance.
/// 连接一个控制器客户端并启动租约维护。
#[unsafe(no_mangle)]
pub extern "C" fn vldb_controller_ffi_client_connect(
    client: *mut FfiControllerClientHandle,
    error_out: *mut *mut c_char,
) -> c_int {
    let result = with_client_handle(client, |handle| {
        handle
            .runtime
            .block_on(handle.client.connect())
            .map_err(error_to_string)
    });
    match result {
        Ok(()) => ffi_ok_status(error_out),
        Err(error) => ffi_error_status(error_out, error),
    }
}

/// Connect one controller client using JSON mode.
/// 使用 JSON 模式连接一个控制器客户端。
#[unsafe(no_mangle)]
pub extern "C" fn vldb_controller_ffi_client_connect_json(
    client: *mut FfiControllerClientHandle,
    request_json: *const c_char,
    response_out: *mut *mut c_char,
    error_out: *mut *mut c_char,
) -> c_int {
    clear_out_ptr(response_out);
    let result = (|| -> Result<String, String> {
        let _: EmptyJsonRequest = parse_json_input(request_json)?;
        with_client_handle(client, |handle| {
            handle
                .runtime
                .block_on(handle.client.connect())
                .map_err(error_to_string)?;
            serde_json::to_string(&SuccessJsonResponse { ok: true })
                .map_err(|error| format!("failed to serialize connect_json response: {error}"))
        })
    })();
    match result {
        Ok(response) => {
            write_out_ptr(response_out, string_into_raw(response));
            ffi_ok_status(error_out)
        }
        Err(error) => ffi_error_status(error_out, error),
    }
}

/// Shutdown one controller client and unregister its lease explicitly.
/// 关闭一个控制器客户端并显式注销其租约。
#[unsafe(no_mangle)]
pub extern "C" fn vldb_controller_ffi_client_shutdown(
    client: *mut FfiControllerClientHandle,
    error_out: *mut *mut c_char,
) -> c_int {
    let result = with_client_handle(client, |handle| {
        handle
            .runtime
            .block_on(handle.client.shutdown())
            .map_err(error_to_string)
    });
    match result {
        Ok(()) => ffi_ok_status(error_out),
        Err(error) => ffi_error_status(error_out, error),
    }
}

/// Shutdown one controller client using JSON mode.
/// 使用 JSON 模式关闭一个控制器客户端。
#[unsafe(no_mangle)]
pub extern "C" fn vldb_controller_ffi_client_shutdown_json(
    client: *mut FfiControllerClientHandle,
    request_json: *const c_char,
    response_out: *mut *mut c_char,
    error_out: *mut *mut c_char,
) -> c_int {
    clear_out_ptr(response_out);
    let result = (|| -> Result<String, String> {
        let _: EmptyJsonRequest = parse_json_input(request_json)?;
        with_client_handle(client, |handle| {
            handle
                .runtime
                .block_on(handle.client.shutdown())
                .map_err(error_to_string)?;
            serde_json::to_string(&SuccessJsonResponse { ok: true })
                .map_err(|error| format!("failed to serialize shutdown_json response: {error}"))
        })
    })();
    match result {
        Ok(response) => {
            write_out_ptr(response_out, string_into_raw(response));
            ffi_ok_status(error_out)
        }
        Err(error) => ffi_error_status(error_out, error),
    }
}

/// Return one native controller status snapshot.
/// 返回一份原生控制器状态快照。
#[unsafe(no_mangle)]
pub extern "C" fn vldb_controller_ffi_client_get_status(
    client: *mut FfiControllerClientHandle,
    status_out: *mut *mut FfiControllerStatusSnapshot,
    error_out: *mut *mut c_char,
) -> c_int {
    clear_out_ptr(status_out);
    let result = with_client_handle(client, |handle| {
        handle
            .runtime
            .block_on(handle.client.get_status())
            .map_err(error_to_string)
    });
    match result {
        Ok(snapshot) => {
            write_out_ptr(
                status_out,
                Box::into_raw(Box::new(map_status_snapshot(snapshot))),
            );
            ffi_ok_status(error_out)
        }
        Err(error) => ffi_error_status(error_out, error),
    }
}

/// Return one JSON controller status snapshot.
/// 返回一份 JSON 控制器状态快照。
#[unsafe(no_mangle)]
pub extern "C" fn vldb_controller_ffi_client_get_status_json(
    client: *mut FfiControllerClientHandle,
    response_out: *mut *mut c_char,
    error_out: *mut *mut c_char,
) -> c_int {
    clear_out_ptr(response_out);
    let result = with_client_handle(client, |handle| {
        let snapshot = handle
            .runtime
            .block_on(handle.client.get_status())
            .map_err(error_to_string)?;
        serde_json::to_string(&snapshot)
            .map_err(|error| format!("failed to serialize get_status_json response: {error}"))
    });
    match result {
        Ok(response) => {
            write_out_ptr(response_out, string_into_raw(response));
            ffi_ok_status(error_out)
        }
        Err(error) => ffi_error_status(error_out, error),
    }
}

/// Free one native controller status snapshot.
/// 释放一份原生控制器状态快照。
#[unsafe(no_mangle)]
pub extern "C" fn vldb_controller_ffi_controller_status_free(
    status: *mut FfiControllerStatusSnapshot,
) {
    if !status.is_null() {
        unsafe {
            let status = Box::from_raw(status);
            vldb_controller_ffi_string_free(status.bind_addr);
        }
    }
}

/// Attach one runtime space using native structured input.
/// 使用原生结构化输入附着一个运行时空间。
#[unsafe(no_mangle)]
pub extern "C" fn vldb_controller_ffi_client_attach_space(
    client: *mut FfiControllerClientHandle,
    registration: *const FfiSpaceRegistration,
    space_out: *mut *mut FfiSpaceSnapshot,
    error_out: *mut *mut c_char,
) -> c_int {
    clear_out_ptr(space_out);
    let result = (|| -> Result<SpaceSnapshot, String> {
        let registration = ffi_space_registration_to_rust(registration)?;
        with_client_handle(client, |handle| {
            handle
                .runtime
                .block_on(handle.client.attach_space(registration))
                .map_err(error_to_string)
        })
    })();
    match result {
        Ok(snapshot) => {
            write_out_ptr(
                space_out,
                Box::into_raw(Box::new(map_space_snapshot(snapshot))),
            );
            ffi_ok_status(error_out)
        }
        Err(error) => ffi_error_status(error_out, error),
    }
}

/// Attach one runtime space using JSON input and JSON output.
/// 使用 JSON 输入与 JSON 输出附着一个运行时空间。
#[unsafe(no_mangle)]
pub extern "C" fn vldb_controller_ffi_client_attach_space_json(
    client: *mut FfiControllerClientHandle,
    request_json: *const c_char,
    response_out: *mut *mut c_char,
    error_out: *mut *mut c_char,
) -> c_int {
    clear_out_ptr(response_out);
    let result = (|| -> Result<String, String> {
        let request: AttachSpaceJsonRequest = parse_json_input(request_json)?;
        let snapshot = with_client_handle(client, |handle| {
            handle
                .runtime
                .block_on(handle.client.attach_space(request.registration))
                .map_err(error_to_string)
        })?;
        serde_json::to_string(&snapshot)
            .map_err(|error| format!("failed to serialize attach_space_json response: {error}"))
    })();
    match result {
        Ok(response) => {
            write_out_ptr(response_out, string_into_raw(response));
            ffi_ok_status(error_out)
        }
        Err(error) => ffi_error_status(error_out, error),
    }
}

/// Detach one runtime space using native structured input.
/// 使用原生结构化输入解除一个运行时空间附着。
#[unsafe(no_mangle)]
pub extern "C" fn vldb_controller_ffi_client_detach_space(
    client: *mut FfiControllerClientHandle,
    space_id: *const c_char,
    detached_out: *mut c_uchar,
    error_out: *mut *mut c_char,
) -> c_int {
    clear_out_u8(detached_out);
    let result = (|| -> Result<bool, String> {
        let space_id = required_c_string(space_id, "space_id")?;
        with_client_handle(client, |handle| {
            handle
                .runtime
                .block_on(handle.client.detach_space(space_id))
                .map_err(error_to_string)
        })
    })();
    match result {
        Ok(detached) => {
            write_out_u8(detached_out, if detached { 1 } else { 0 });
            ffi_ok_status(error_out)
        }
        Err(error) => ffi_error_status(error_out, error),
    }
}

/// Detach one runtime space using JSON input and JSON output.
/// 使用 JSON 输入与 JSON 输出解除一个运行时空间附着。
#[unsafe(no_mangle)]
pub extern "C" fn vldb_controller_ffi_client_detach_space_json(
    client: *mut FfiControllerClientHandle,
    request_json: *const c_char,
    response_out: *mut *mut c_char,
    error_out: *mut *mut c_char,
) -> c_int {
    clear_out_ptr(response_out);
    let result = (|| -> Result<String, String> {
        let request: DetachSpaceJsonRequest = parse_json_input(request_json)?;
        let detached = with_client_handle(client, |handle| {
            handle
                .runtime
                .block_on(handle.client.detach_space(request.space_id))
                .map_err(error_to_string)
        })?;
        serde_json::to_string(&detached)
            .map_err(|error| format!("failed to serialize detach_space_json response: {error}"))
    })();
    match result {
        Ok(response) => {
            write_out_ptr(response_out, string_into_raw(response));
            ffi_ok_status(error_out)
        }
        Err(error) => ffi_error_status(error_out, error),
    }
}

/// Return all known spaces using a native array payload.
/// 使用原生数组载荷返回全部已知空间。
#[unsafe(no_mangle)]
pub extern "C" fn vldb_controller_ffi_client_list_spaces(
    client: *mut FfiControllerClientHandle,
    spaces_out: *mut *mut FfiSpaceSnapshotArray,
    error_out: *mut *mut c_char,
) -> c_int {
    clear_out_ptr(spaces_out);
    let result = with_client_handle(client, |handle| {
        handle
            .runtime
            .block_on(handle.client.list_spaces())
            .map_err(error_to_string)
    });
    match result {
        Ok(spaces) => {
            let mapped = map_space_snapshot_array(spaces);
            write_out_ptr(spaces_out, Box::into_raw(Box::new(mapped)));
            ffi_ok_status(error_out)
        }
        Err(error) => ffi_error_status(error_out, error),
    }
}

/// Return all known spaces using JSON mode.
/// 使用 JSON 模式返回全部已知空间。
#[unsafe(no_mangle)]
pub extern "C" fn vldb_controller_ffi_client_list_spaces_json(
    client: *mut FfiControllerClientHandle,
    response_out: *mut *mut c_char,
    error_out: *mut *mut c_char,
) -> c_int {
    clear_out_ptr(response_out);
    let result = with_client_handle(client, |handle| {
        let spaces = handle
            .runtime
            .block_on(handle.client.list_spaces())
            .map_err(error_to_string)?;
        serde_json::to_string(&spaces)
            .map_err(|error| format!("failed to serialize list_spaces_json response: {error}"))
    });
    match result {
        Ok(response) => {
            write_out_ptr(response_out, string_into_raw(response));
            ffi_ok_status(error_out)
        }
        Err(error) => ffi_error_status(error_out, error),
    }
}

/// Free one native space snapshot array.
/// 释放一份原生空间快照数组。
#[unsafe(no_mangle)]
pub extern "C" fn vldb_controller_ffi_space_snapshot_array_free(
    spaces: *mut FfiSpaceSnapshotArray,
) {
    if spaces.is_null() {
        return;
    }
    unsafe {
        let spaces = Box::from_raw(spaces);
        if !spaces.items.is_null() {
            let items = Vec::from_raw_parts(spaces.items, spaces.len, spaces.len);
            for item in items {
                free_space_snapshot_fields(item);
            }
        }
    }
}

/// Free one native space snapshot returned by single-object APIs.
/// 释放由单对象接口返回的一份原生空间快照。
#[unsafe(no_mangle)]
pub extern "C" fn vldb_controller_ffi_space_snapshot_free(space: *mut FfiSpaceSnapshot) {
    if space.is_null() {
        return;
    }
    unsafe {
        let space = Box::from_raw(space);
        free_space_snapshot_fields(*space);
    }
}

/// Enable one SQLite backend using native structured input.
/// 使用原生结构化输入启用一个 SQLite 后端。
#[unsafe(no_mangle)]
pub extern "C" fn vldb_controller_ffi_client_enable_sqlite(
    client: *mut FfiControllerClientHandle,
    request: *const FfiControllerSqliteEnableRequest,
    error_out: *mut *mut c_char,
) -> c_int {
    let result = (|| -> Result<(), String> {
        let request = ffi_sqlite_enable_request_to_rust(request)?;
        with_client_handle(client, |handle| {
            handle
                .runtime
                .block_on(handle.client.enable_sqlite(request))
                .map_err(error_to_string)
        })
    })();
    match result {
        Ok(()) => ffi_ok_status(error_out),
        Err(error) => ffi_error_status(error_out, error),
    }
}

/// Enable one SQLite backend using JSON mode.
/// 使用 JSON 模式启用一个 SQLite 后端。
#[unsafe(no_mangle)]
pub extern "C" fn vldb_controller_ffi_client_enable_sqlite_json(
    client: *mut FfiControllerClientHandle,
    request_json: *const c_char,
    response_out: *mut *mut c_char,
    error_out: *mut *mut c_char,
) -> c_int {
    clear_out_ptr(response_out);
    let result = (|| -> Result<String, String> {
        let request: EnableSqliteJsonRequest = parse_json_input(request_json)?;
        with_client_handle(client, |handle| {
            handle
                .runtime
                .block_on(handle.client.enable_sqlite(request.request))
                .map_err(error_to_string)
        })?;
        serde_json::to_string(&SuccessJsonResponse { ok: true })
            .map_err(|error| format!("failed to serialize enable_sqlite_json response: {error}"))
    })();
    match result {
        Ok(response) => {
            write_out_ptr(response_out, string_into_raw(response));
            ffi_ok_status(error_out)
        }
        Err(error) => ffi_error_status(error_out, error),
    }
}

/// Disable one SQLite backend using native structured input.
/// 使用原生结构化输入关闭一个 SQLite 后端。
#[unsafe(no_mangle)]
pub extern "C" fn vldb_controller_ffi_client_disable_sqlite(
    client: *mut FfiControllerClientHandle,
    space_id: *const c_char,
    disabled_out: *mut c_uchar,
    error_out: *mut *mut c_char,
) -> c_int {
    clear_out_u8(disabled_out);
    let result = (|| -> Result<bool, String> {
        let space_id = required_c_string(space_id, "space_id")?;
        let binding_id = space_id.clone();
        with_client_handle(client, |handle| {
            handle
                .runtime
                .block_on(handle.client.disable_sqlite(space_id, binding_id))
                .map_err(error_to_string)
        })
    })();
    match result {
        Ok(disabled) => {
            write_out_u8(disabled_out, if disabled { 1 } else { 0 });
            ffi_ok_status(error_out)
        }
        Err(error) => ffi_error_status(error_out, error),
    }
}

/// Disable one SQLite backend using JSON mode.
/// 使用 JSON 模式关闭一个 SQLite 后端。
#[unsafe(no_mangle)]
pub extern "C" fn vldb_controller_ffi_client_disable_sqlite_json(
    client: *mut FfiControllerClientHandle,
    request_json: *const c_char,
    response_out: *mut *mut c_char,
    error_out: *mut *mut c_char,
) -> c_int {
    clear_out_ptr(response_out);
    let result = (|| -> Result<String, String> {
        let request: DisableBackendJsonRequest = parse_json_input(request_json)?;
        let binding_id = request.space_id.clone();
        let disabled = with_client_handle(client, |handle| {
            handle
                .runtime
                .block_on(handle.client.disable_sqlite(request.space_id, binding_id))
                .map_err(error_to_string)
        })?;
        serde_json::to_string(&disabled)
            .map_err(|error| format!("failed to serialize disable_sqlite_json response: {error}"))
    })();
    match result {
        Ok(response) => {
            write_out_ptr(response_out, string_into_raw(response));
            ffi_ok_status(error_out)
        }
        Err(error) => ffi_error_status(error_out, error),
    }
}

/// Execute one SQLite script using native arguments.
/// 使用原生参数执行一条 SQLite 脚本。
#[unsafe(no_mangle)]
pub extern "C" fn vldb_controller_ffi_client_execute_sqlite_script(
    client: *mut FfiControllerClientHandle,
    space_id: *const c_char,
    sql: *const c_char,
    params: *const FfiSqliteValue,
    params_len: usize,
    result_out: *mut *mut FfiControllerSqliteExecuteResult,
    error_out: *mut *mut c_char,
) -> c_int {
    clear_out_ptr(result_out);
    let result = (|| -> Result<ControllerSqliteExecuteResult, String> {
        let space_id = required_c_string(space_id, "space_id")?;
        let binding_id = space_id.clone();
        let sql = required_c_string(sql, "sql")?;
        let params = read_sqlite_values(params, params_len, "params")?;
        with_client_handle(client, |handle| {
            handle
                .runtime
                .block_on(
                    handle
                        .client
                        .execute_sqlite_script_typed(space_id, binding_id, sql, params),
                )
                .map_err(error_to_string)
        })
    })();
    match result {
        Ok(result) => {
            write_out_ptr(
                result_out,
                Box::into_raw(Box::new(map_sqlite_execute_result(result))),
            );
            ffi_ok_status(error_out)
        }
        Err(error) => ffi_error_status(error_out, error),
    }
}

/// Execute one SQLite script using JSON mode.
/// 使用 JSON 模式执行一条 SQLite 脚本。
#[unsafe(no_mangle)]
pub extern "C" fn vldb_controller_ffi_client_execute_sqlite_script_json(
    client: *mut FfiControllerClientHandle,
    request_json: *const c_char,
    response_out: *mut *mut c_char,
    error_out: *mut *mut c_char,
) -> c_int {
    clear_out_ptr(response_out);
    let result = (|| -> Result<String, String> {
        let request: ExecuteSqliteScriptJsonRequest = parse_json_input(request_json)?;
        let binding_id = request.space_id.clone();
        let result = with_client_handle(client, |handle| {
            let params_json = serde_json::to_string(&request.params).map_err(|error| {
                format!("failed to serialize execute_sqlite_script_json params: {error}")
            })?;
            handle
                .runtime
                .block_on(handle.client.execute_sqlite_script(
                    request.space_id,
                    binding_id,
                    request.sql,
                    params_json,
                ))
                .map_err(error_to_string)
        })?;
        serde_json::to_string(&result).map_err(|error| {
            format!("failed to serialize execute_sqlite_script_json response: {error}")
        })
    })();
    match result {
        Ok(response) => {
            write_out_ptr(response_out, string_into_raw(response));
            ffi_ok_status(error_out)
        }
        Err(error) => ffi_error_status(error_out, error),
    }
}

/// Free one native SQLite execution result.
/// 释放一份原生 SQLite 执行结果。
#[unsafe(no_mangle)]
pub extern "C" fn vldb_controller_ffi_sqlite_execute_result_free(
    result: *mut FfiControllerSqliteExecuteResult,
) {
    if !result.is_null() {
        unsafe {
            let result = Box::from_raw(result);
            vldb_controller_ffi_string_free(result.message);
        }
    }
}

/// Execute one SQLite JSON query using native arguments.
/// 使用原生参数执行一条 SQLite JSON 查询。
#[unsafe(no_mangle)]
pub extern "C" fn vldb_controller_ffi_client_query_sqlite_json(
    client: *mut FfiControllerClientHandle,
    space_id: *const c_char,
    sql: *const c_char,
    params: *const FfiSqliteValue,
    params_len: usize,
    result_out: *mut *mut FfiControllerSqliteQueryResult,
    error_out: *mut *mut c_char,
) -> c_int {
    clear_out_ptr(result_out);
    let result = (|| -> Result<ControllerSqliteQueryResult, String> {
        let space_id = required_c_string(space_id, "space_id")?;
        let binding_id = space_id.clone();
        let sql = required_c_string(sql, "sql")?;
        let params = read_sqlite_values(params, params_len, "params")?;
        with_client_handle(client, |handle| {
            handle
                .runtime
                .block_on(
                    handle
                        .client
                        .query_sqlite_json_typed(space_id, binding_id, sql, params),
                )
                .map_err(error_to_string)
        })
    })();
    match result {
        Ok(result) => {
            write_out_ptr(
                result_out,
                Box::into_raw(Box::new(map_sqlite_query_result(result))),
            );
            ffi_ok_status(error_out)
        }
        Err(error) => ffi_error_status(error_out, error),
    }
}

/// Execute one SQLite JSON query using JSON mode.
/// 使用 JSON 模式执行一条 SQLite JSON 查询。
#[unsafe(no_mangle)]
pub extern "C" fn vldb_controller_ffi_client_query_sqlite_json_json(
    client: *mut FfiControllerClientHandle,
    request_json: *const c_char,
    response_out: *mut *mut c_char,
    error_out: *mut *mut c_char,
) -> c_int {
    clear_out_ptr(response_out);
    let result = (|| -> Result<String, String> {
        let request: QuerySqliteJsonJsonRequest = parse_json_input(request_json)?;
        let binding_id = request.space_id.clone();
        let result = with_client_handle(client, |handle| {
            let params_json = serde_json::to_string(&request.params).map_err(|error| {
                format!("failed to serialize query_sqlite_json_json params: {error}")
            })?;
            handle
                .runtime
                .block_on(handle.client.query_sqlite_json(
                    request.space_id,
                    binding_id,
                    request.sql,
                    params_json,
                ))
                .map_err(error_to_string)
        })?;
        serde_json::to_string(&result).map_err(|error| {
            format!("failed to serialize query_sqlite_json_json response: {error}")
        })
    })();
    match result {
        Ok(response) => {
            write_out_ptr(response_out, string_into_raw(response));
            ffi_ok_status(error_out)
        }
        Err(error) => ffi_error_status(error_out, error),
    }
}

/// Free one native SQLite JSON query result.
/// 释放一份原生 SQLite JSON 查询结果。
#[unsafe(no_mangle)]
pub extern "C" fn vldb_controller_ffi_sqlite_query_result_free(
    result: *mut FfiControllerSqliteQueryResult,
) {
    if !result.is_null() {
        unsafe {
            let result = Box::from_raw(result);
            vldb_controller_ffi_string_free(result.json_data);
        }
    }
}

/// Execute one SQLite batch using native arguments.
/// 使用原生参数执行一组 SQLite 批量语句。
#[unsafe(no_mangle)]
pub extern "C" fn vldb_controller_ffi_client_execute_sqlite_batch(
    client: *mut FfiControllerClientHandle,
    space_id: *const c_char,
    sql: *const c_char,
    items: *const FfiSqliteBatchItem,
    items_len: usize,
    result_out: *mut *mut FfiControllerSqliteExecuteBatchResult,
    error_out: *mut *mut c_char,
) -> c_int {
    clear_out_ptr(result_out);
    let result = (|| -> Result<ControllerSqliteExecuteBatchResult, String> {
        let space_id = required_c_string(space_id, "space_id")?;
        let binding_id = space_id.clone();
        let sql = required_c_string(sql, "sql")?;
        let items = read_sqlite_batch_items(items, items_len, "items")?;
        with_client_handle(client, |handle| {
            handle
                .runtime
                .block_on(
                    handle
                        .client
                        .execute_sqlite_batch_typed(space_id, binding_id, sql, items),
                )
                .map_err(error_to_string)
        })
    })();
    match result {
        Ok(result) => {
            write_out_ptr(
                result_out,
                Box::into_raw(Box::new(map_sqlite_execute_batch_result(result))),
            );
            ffi_ok_status(error_out)
        }
        Err(error) => ffi_error_status(error_out, error),
    }
}

/// Execute one SQLite batch using JSON mode.
/// 使用 JSON 模式执行一组 SQLite 批量语句。
#[unsafe(no_mangle)]
pub extern "C" fn vldb_controller_ffi_client_execute_sqlite_batch_json(
    client: *mut FfiControllerClientHandle,
    request_json: *const c_char,
    response_out: *mut *mut c_char,
    error_out: *mut *mut c_char,
) -> c_int {
    clear_out_ptr(response_out);
    let result = (|| -> Result<String, String> {
        let request: ExecuteSqliteBatchJsonRequest = parse_json_input(request_json)?;
        let binding_id = request.space_id.clone();
        let result = with_client_handle(client, |handle| {
            let batch_params_json = request
                .batch_params
                .iter()
                .map(|item| {
                    serde_json::to_string(item).map_err(|error| {
                        format!(
                            "failed to serialize execute_sqlite_batch_json batch_params item: {error}"
                        )
                    })
                })
                .collect::<Result<Vec<_>, _>>()?;
            handle
                .runtime
                .block_on(handle.client.execute_sqlite_batch(
                    request.space_id,
                    binding_id,
                    request.sql,
                    batch_params_json,
                ))
                .map_err(error_to_string)
        })?;
        serde_json::to_string(&result).map_err(|error| {
            format!("failed to serialize execute_sqlite_batch_json response: {error}")
        })
    })();
    match result {
        Ok(response) => {
            write_out_ptr(response_out, string_into_raw(response));
            ffi_ok_status(error_out)
        }
        Err(error) => ffi_error_status(error_out, error),
    }
}

/// Free one native SQLite batch execution result.
/// 释放一份原生 SQLite 批量执行结果。
#[unsafe(no_mangle)]
pub extern "C" fn vldb_controller_ffi_sqlite_execute_batch_result_free(
    result: *mut FfiControllerSqliteExecuteBatchResult,
) {
    if !result.is_null() {
        unsafe {
            let result = Box::from_raw(result);
            vldb_controller_ffi_string_free(result.message);
        }
    }
}

/// Execute one SQLite streaming query using native arguments.
/// 使用原生参数执行一条 SQLite 流式查询。
#[unsafe(no_mangle)]
pub extern "C" fn vldb_controller_ffi_client_query_sqlite_stream(
    client: *mut FfiControllerClientHandle,
    space_id: *const c_char,
    sql: *const c_char,
    params: *const FfiSqliteValue,
    params_len: usize,
    target_chunk_size: c_ulonglong,
    result_out: *mut *mut FfiControllerSqliteQueryStreamResult,
    error_out: *mut *mut c_char,
) -> c_int {
    clear_out_ptr(result_out);
    let result = (|| -> Result<ControllerSqliteQueryStreamCompatResult, String> {
        let space_id = required_c_string(space_id, "space_id")?;
        let binding_id = space_id.clone();
        let sql = required_c_string(sql, "sql")?;
        let params = read_sqlite_values(params, params_len, "params")?;
        let chunk_size = if target_chunk_size == 0 {
            None
        } else {
            Some(target_chunk_size)
        };
        with_client_handle(client, |handle| {
            collect_sqlite_query_stream_result(
                &handle.runtime,
                &handle.client,
                space_id,
                binding_id,
                sql,
                params,
                chunk_size,
            )
        })
    })();
    match result {
        Ok(result) => {
            write_out_ptr(
                result_out,
                Box::into_raw(Box::new(map_sqlite_query_stream_result(result))),
            );
            ffi_ok_status(error_out)
        }
        Err(error) => ffi_error_status(error_out, error),
    }
}

/// Execute one SQLite streaming query using JSON mode.
/// 使用 JSON 模式执行一条 SQLite 流式查询。
#[unsafe(no_mangle)]
pub extern "C" fn vldb_controller_ffi_client_query_sqlite_stream_json(
    client: *mut FfiControllerClientHandle,
    request_json: *const c_char,
    response_out: *mut *mut c_char,
    error_out: *mut *mut c_char,
) -> c_int {
    clear_out_ptr(response_out);
    let result = (|| -> Result<String, String> {
        let request: QuerySqliteStreamJsonRequest = parse_json_input(request_json)?;
        let binding_id = request.space_id.clone();
        let result = with_client_handle(client, |handle| {
            let params = request
                .params
                .iter()
                .cloned()
                .map(json_to_sqlite_value)
                .collect::<Result<Vec<_>, _>>()?;
            collect_sqlite_query_stream_result(
                &handle.runtime,
                &handle.client,
                request.space_id,
                binding_id,
                request.sql,
                params,
                request.target_chunk_size,
            )
        })?;
        serde_json::to_string(&QuerySqliteStreamJsonResponse {
            chunks_base64: result
                .chunks
                .iter()
                .map(|chunk| encode_base64(chunk))
                .collect(),
            row_count: result.row_count,
            chunk_count: result.chunk_count,
            total_bytes: result.total_bytes,
        })
        .map_err(|error| format!("failed to serialize query_sqlite_stream_json response: {error}"))
    })();
    match result {
        Ok(response) => {
            write_out_ptr(response_out, string_into_raw(response));
            ffi_ok_status(error_out)
        }
        Err(error) => ffi_error_status(error_out, error),
    }
}

/// Free one native SQLite streaming query result.
/// 释放一份原生 SQLite 流式查询结果。
#[unsafe(no_mangle)]
pub extern "C" fn vldb_controller_ffi_sqlite_query_stream_result_free(
    result: *mut FfiControllerSqliteQueryStreamResult,
) {
    if !result.is_null() {
        unsafe {
            let result = Box::from_raw(result);
            vldb_controller_ffi_byte_buffer_array_free(result.chunks);
        }
    }
}

/// Free one native byte-buffer array.
/// 释放一份原生字节缓冲数组。
#[unsafe(no_mangle)]
pub extern "C" fn vldb_controller_ffi_byte_buffer_array_free(value: *mut FfiByteBufferArray) {
    if value.is_null() {
        return;
    }
    unsafe {
        let value = Box::from_raw(value);
        if !value.items.is_null() {
            let items = Vec::from_raw_parts(value.items, value.len, value.len);
            for item in items {
                vldb_controller_ffi_bytes_free(item.data, item.len);
            }
        }
    }
}

/// Tokenize SQLite text using native arguments.
/// 使用原生参数执行 SQLite 文本分词。
#[unsafe(no_mangle)]
pub extern "C" fn vldb_controller_ffi_client_tokenize_sqlite_text(
    client: *mut FfiControllerClientHandle,
    space_id: *const c_char,
    tokenizer_mode: *const c_char,
    text: *const c_char,
    search_mode: c_uchar,
    result_out: *mut *mut FfiControllerSqliteTokenizeResult,
    error_out: *mut *mut c_char,
) -> c_int {
    clear_out_ptr(result_out);
    let result = (|| -> Result<ControllerSqliteTokenizeResult, String> {
        let space_id = required_c_string(space_id, "space_id")?;
        let binding_id = space_id.clone();
        let tokenizer_mode = parse_sqlite_tokenizer_mode_text(tokenizer_mode)?;
        let text = required_c_string_preserve(text, "text")?;
        with_client_handle(client, |handle| {
            handle
                .runtime
                .block_on(handle.client.tokenize_sqlite_text(
                    space_id,
                    binding_id,
                    tokenizer_mode,
                    text,
                    search_mode != 0,
                ))
                .map_err(error_to_string)
        })
    })();
    match result {
        Ok(result) => {
            write_out_ptr(
                result_out,
                Box::into_raw(Box::new(map_sqlite_tokenize_result(result))),
            );
            ffi_ok_status(error_out)
        }
        Err(error) => ffi_error_status(error_out, error),
    }
}

/// Tokenize SQLite text using JSON mode.
/// 使用 JSON 模式执行 SQLite 文本分词。
#[unsafe(no_mangle)]
pub extern "C" fn vldb_controller_ffi_client_tokenize_sqlite_text_json(
    client: *mut FfiControllerClientHandle,
    request_json: *const c_char,
    response_out: *mut *mut c_char,
    error_out: *mut *mut c_char,
) -> c_int {
    clear_out_ptr(response_out);
    let result = (|| -> Result<String, String> {
        let request: TokenizeSqliteTextJsonRequest = parse_json_input(request_json)?;
        let binding_id = request.space_id.clone();
        let tokenizer_mode = parse_sqlite_tokenizer_mode_name(&request.tokenizer_mode)?;
        let result = with_client_handle(client, |handle| {
            handle
                .runtime
                .block_on(handle.client.tokenize_sqlite_text(
                    request.space_id,
                    binding_id,
                    tokenizer_mode,
                    request.text,
                    request.search_mode,
                ))
                .map_err(error_to_string)
        })?;
        serde_json::to_string(&result).map_err(|error| {
            format!("failed to serialize tokenize_sqlite_text_json response: {error}")
        })
    })();
    match result {
        Ok(response) => {
            write_out_ptr(response_out, string_into_raw(response));
            ffi_ok_status(error_out)
        }
        Err(error) => ffi_error_status(error_out, error),
    }
}

/// Free one native SQLite tokenize result.
/// 释放一份原生 SQLite 分词结果。
#[unsafe(no_mangle)]
pub extern "C" fn vldb_controller_ffi_sqlite_tokenize_result_free(
    result: *mut FfiControllerSqliteTokenizeResult,
) {
    if !result.is_null() {
        unsafe {
            let result = Box::from_raw(result);
            vldb_controller_ffi_string_free(result.tokenizer_mode);
            vldb_controller_ffi_string_free(result.normalized_text);
            vldb_controller_ffi_string_free(result.tokens_json);
            vldb_controller_ffi_string_free(result.fts_query);
        }
    }
}

/// List SQLite custom words using native arguments.
/// 使用原生参数列出 SQLite 自定义词。
#[unsafe(no_mangle)]
pub extern "C" fn vldb_controller_ffi_client_list_sqlite_custom_words(
    client: *mut FfiControllerClientHandle,
    space_id: *const c_char,
    result_out: *mut *mut FfiControllerSqliteListCustomWordsResult,
    error_out: *mut *mut c_char,
) -> c_int {
    clear_out_ptr(result_out);
    let result = (|| -> Result<ControllerSqliteListCustomWordsResult, String> {
        let space_id = required_c_string(space_id, "space_id")?;
        let binding_id = space_id.clone();
        with_client_handle(client, |handle| {
            handle
                .runtime
                .block_on(handle.client.list_sqlite_custom_words(space_id, binding_id))
                .map_err(error_to_string)
        })
    })();
    match result {
        Ok(result) => {
            write_out_ptr(
                result_out,
                Box::into_raw(Box::new(map_sqlite_list_custom_words_result(result))),
            );
            ffi_ok_status(error_out)
        }
        Err(error) => ffi_error_status(error_out, error),
    }
}

/// List SQLite custom words using JSON mode.
/// 使用 JSON 模式列出 SQLite 自定义词。
#[unsafe(no_mangle)]
pub extern "C" fn vldb_controller_ffi_client_list_sqlite_custom_words_json(
    client: *mut FfiControllerClientHandle,
    request_json: *const c_char,
    response_out: *mut *mut c_char,
    error_out: *mut *mut c_char,
) -> c_int {
    clear_out_ptr(response_out);
    let result = (|| -> Result<String, String> {
        let request: ListSqliteCustomWordsJsonRequest = parse_json_input(request_json)?;
        let binding_id = request.space_id.clone();
        let result = with_client_handle(client, |handle| {
            handle
                .runtime
                .block_on(
                    handle
                        .client
                        .list_sqlite_custom_words(request.space_id, binding_id),
                )
                .map_err(error_to_string)
        })?;
        serde_json::to_string(&result).map_err(|error| {
            format!("failed to serialize list_sqlite_custom_words_json response: {error}")
        })
    })();
    match result {
        Ok(response) => {
            write_out_ptr(response_out, string_into_raw(response));
            ffi_ok_status(error_out)
        }
        Err(error) => ffi_error_status(error_out, error),
    }
}

/// Free one native SQLite custom-word array.
/// 释放一份原生 SQLite 自定义词数组。
#[unsafe(no_mangle)]
pub extern "C" fn vldb_controller_ffi_sqlite_custom_word_array_free(
    value: *mut FfiControllerSqliteCustomWordArray,
) {
    if value.is_null() {
        return;
    }
    unsafe {
        let value = Box::from_raw(value);
        if !value.items.is_null() {
            let items = Vec::from_raw_parts(value.items, value.len, value.len);
            for item in items {
                vldb_controller_ffi_string_free(item.word);
            }
        }
    }
}

/// Free one native SQLite custom-word listing result.
/// 释放一份原生 SQLite 自定义词列表结果。
#[unsafe(no_mangle)]
pub extern "C" fn vldb_controller_ffi_sqlite_list_custom_words_result_free(
    result: *mut FfiControllerSqliteListCustomWordsResult,
) {
    if !result.is_null() {
        unsafe {
            let result = Box::from_raw(result);
            vldb_controller_ffi_string_free(result.message);
            vldb_controller_ffi_sqlite_custom_word_array_free(result.words);
        }
    }
}

/// Upsert one SQLite custom word using native arguments.
/// 使用原生参数写入一条 SQLite 自定义词。
#[unsafe(no_mangle)]
pub extern "C" fn vldb_controller_ffi_client_upsert_sqlite_custom_word(
    client: *mut FfiControllerClientHandle,
    space_id: *const c_char,
    word: *const c_char,
    weight: u32,
    result_out: *mut *mut FfiControllerSqliteDictionaryMutationResult,
    error_out: *mut *mut c_char,
) -> c_int {
    clear_out_ptr(result_out);
    let result = (|| -> Result<ControllerSqliteDictionaryMutationResult, String> {
        let space_id = required_c_string(space_id, "space_id")?;
        let binding_id = space_id.clone();
        let word = required_c_string(word, "word")?;
        with_client_handle(client, |handle| {
            handle
                .runtime
                .block_on(
                    handle
                        .client
                        .upsert_sqlite_custom_word(space_id, binding_id, word, weight),
                )
                .map_err(error_to_string)
        })
    })();
    match result {
        Ok(result) => {
            write_out_ptr(
                result_out,
                Box::into_raw(Box::new(map_sqlite_dictionary_mutation_result(result))),
            );
            ffi_ok_status(error_out)
        }
        Err(error) => ffi_error_status(error_out, error),
    }
}

/// Upsert one SQLite custom word using JSON mode.
/// 使用 JSON 模式写入一条 SQLite 自定义词。
#[unsafe(no_mangle)]
pub extern "C" fn vldb_controller_ffi_client_upsert_sqlite_custom_word_json(
    client: *mut FfiControllerClientHandle,
    request_json: *const c_char,
    response_out: *mut *mut c_char,
    error_out: *mut *mut c_char,
) -> c_int {
    clear_out_ptr(response_out);
    let result = (|| -> Result<String, String> {
        let request: UpsertSqliteCustomWordJsonRequest = parse_json_input(request_json)?;
        let binding_id = request.space_id.clone();
        let result = with_client_handle(client, |handle| {
            handle
                .runtime
                .block_on(handle.client.upsert_sqlite_custom_word(
                    request.space_id,
                    binding_id,
                    request.word,
                    request.weight,
                ))
                .map_err(error_to_string)
        })?;
        serde_json::to_string(&result).map_err(|error| {
            format!("failed to serialize upsert_sqlite_custom_word_json response: {error}")
        })
    })();
    match result {
        Ok(response) => {
            write_out_ptr(response_out, string_into_raw(response));
            ffi_ok_status(error_out)
        }
        Err(error) => ffi_error_status(error_out, error),
    }
}

/// Remove one SQLite custom word using native arguments.
/// 使用原生参数删除一条 SQLite 自定义词。
#[unsafe(no_mangle)]
pub extern "C" fn vldb_controller_ffi_client_remove_sqlite_custom_word(
    client: *mut FfiControllerClientHandle,
    space_id: *const c_char,
    word: *const c_char,
    result_out: *mut *mut FfiControllerSqliteDictionaryMutationResult,
    error_out: *mut *mut c_char,
) -> c_int {
    clear_out_ptr(result_out);
    let result = (|| -> Result<ControllerSqliteDictionaryMutationResult, String> {
        let space_id = required_c_string(space_id, "space_id")?;
        let binding_id = space_id.clone();
        let word = required_c_string(word, "word")?;
        with_client_handle(client, |handle| {
            handle
                .runtime
                .block_on(
                    handle
                        .client
                        .remove_sqlite_custom_word(space_id, binding_id, word),
                )
                .map_err(error_to_string)
        })
    })();
    match result {
        Ok(result) => {
            write_out_ptr(
                result_out,
                Box::into_raw(Box::new(map_sqlite_dictionary_mutation_result(result))),
            );
            ffi_ok_status(error_out)
        }
        Err(error) => ffi_error_status(error_out, error),
    }
}

/// Remove one SQLite custom word using JSON mode.
/// 使用 JSON 模式删除一条 SQLite 自定义词。
#[unsafe(no_mangle)]
pub extern "C" fn vldb_controller_ffi_client_remove_sqlite_custom_word_json(
    client: *mut FfiControllerClientHandle,
    request_json: *const c_char,
    response_out: *mut *mut c_char,
    error_out: *mut *mut c_char,
) -> c_int {
    clear_out_ptr(response_out);
    let result = (|| -> Result<String, String> {
        let request: RemoveSqliteCustomWordJsonRequest = parse_json_input(request_json)?;
        let binding_id = request.space_id.clone();
        let result = with_client_handle(client, |handle| {
            handle
                .runtime
                .block_on(handle.client.remove_sqlite_custom_word(
                    request.space_id,
                    binding_id,
                    request.word,
                ))
                .map_err(error_to_string)
        })?;
        serde_json::to_string(&result).map_err(|error| {
            format!("failed to serialize remove_sqlite_custom_word_json response: {error}")
        })
    })();
    match result {
        Ok(response) => {
            write_out_ptr(response_out, string_into_raw(response));
            ffi_ok_status(error_out)
        }
        Err(error) => ffi_error_status(error_out, error),
    }
}

/// Free one native SQLite dictionary mutation result.
/// 释放一份原生 SQLite 词典变更结果。
#[unsafe(no_mangle)]
pub extern "C" fn vldb_controller_ffi_sqlite_dictionary_mutation_result_free(
    result: *mut FfiControllerSqliteDictionaryMutationResult,
) {
    if !result.is_null() {
        unsafe {
            let result = Box::from_raw(result);
            vldb_controller_ffi_string_free(result.message);
        }
    }
}

/// Ensure one SQLite FTS index using native arguments.
/// 使用原生参数确认一条 SQLite FTS 索引。
#[unsafe(no_mangle)]
pub extern "C" fn vldb_controller_ffi_client_ensure_sqlite_fts_index(
    client: *mut FfiControllerClientHandle,
    space_id: *const c_char,
    index_name: *const c_char,
    tokenizer_mode: *const c_char,
    result_out: *mut *mut FfiControllerSqliteEnsureFtsIndexResult,
    error_out: *mut *mut c_char,
) -> c_int {
    clear_out_ptr(result_out);
    let result = (|| -> Result<ControllerSqliteEnsureFtsIndexResult, String> {
        let space_id = required_c_string(space_id, "space_id")?;
        let binding_id = space_id.clone();
        let index_name = required_c_string(index_name, "index_name")?;
        let tokenizer_mode = parse_sqlite_tokenizer_mode_text(tokenizer_mode)?;
        with_client_handle(client, |handle| {
            handle
                .runtime
                .block_on(handle.client.ensure_sqlite_fts_index(
                    space_id,
                    binding_id,
                    index_name,
                    tokenizer_mode,
                ))
                .map_err(error_to_string)
        })
    })();
    match result {
        Ok(result) => {
            write_out_ptr(
                result_out,
                Box::into_raw(Box::new(map_sqlite_ensure_fts_index_result(result))),
            );
            ffi_ok_status(error_out)
        }
        Err(error) => ffi_error_status(error_out, error),
    }
}

/// Ensure one SQLite FTS index using JSON mode.
/// 使用 JSON 模式确认一条 SQLite FTS 索引。
#[unsafe(no_mangle)]
pub extern "C" fn vldb_controller_ffi_client_ensure_sqlite_fts_index_json(
    client: *mut FfiControllerClientHandle,
    request_json: *const c_char,
    response_out: *mut *mut c_char,
    error_out: *mut *mut c_char,
) -> c_int {
    clear_out_ptr(response_out);
    let result = (|| -> Result<String, String> {
        let request: EnsureSqliteFtsIndexJsonRequest = parse_json_input(request_json)?;
        let binding_id = request.space_id.clone();
        let tokenizer_mode = parse_sqlite_tokenizer_mode_name(&request.tokenizer_mode)?;
        let result = with_client_handle(client, |handle| {
            handle
                .runtime
                .block_on(handle.client.ensure_sqlite_fts_index(
                    request.space_id,
                    binding_id,
                    request.index_name,
                    tokenizer_mode,
                ))
                .map_err(error_to_string)
        })?;
        serde_json::to_string(&result).map_err(|error| {
            format!("failed to serialize ensure_sqlite_fts_index_json response: {error}")
        })
    })();
    match result {
        Ok(response) => {
            write_out_ptr(response_out, string_into_raw(response));
            ffi_ok_status(error_out)
        }
        Err(error) => ffi_error_status(error_out, error),
    }
}

/// Free one native SQLite ensure-FTS result.
/// 释放一份原生 SQLite FTS 确认结果。
#[unsafe(no_mangle)]
pub extern "C" fn vldb_controller_ffi_sqlite_ensure_fts_index_result_free(
    result: *mut FfiControllerSqliteEnsureFtsIndexResult,
) {
    if !result.is_null() {
        unsafe {
            let result = Box::from_raw(result);
            vldb_controller_ffi_string_free(result.message);
            vldb_controller_ffi_string_free(result.index_name);
            vldb_controller_ffi_string_free(result.tokenizer_mode);
        }
    }
}

/// Rebuild one SQLite FTS index using native arguments.
/// 使用原生参数重建一条 SQLite FTS 索引。
#[unsafe(no_mangle)]
pub extern "C" fn vldb_controller_ffi_client_rebuild_sqlite_fts_index(
    client: *mut FfiControllerClientHandle,
    space_id: *const c_char,
    index_name: *const c_char,
    tokenizer_mode: *const c_char,
    result_out: *mut *mut FfiControllerSqliteRebuildFtsIndexResult,
    error_out: *mut *mut c_char,
) -> c_int {
    clear_out_ptr(result_out);
    let result = (|| -> Result<ControllerSqliteRebuildFtsIndexResult, String> {
        let space_id = required_c_string(space_id, "space_id")?;
        let binding_id = space_id.clone();
        let index_name = required_c_string(index_name, "index_name")?;
        let tokenizer_mode = parse_sqlite_tokenizer_mode_text(tokenizer_mode)?;
        with_client_handle(client, |handle| {
            handle
                .runtime
                .block_on(handle.client.rebuild_sqlite_fts_index(
                    space_id,
                    binding_id,
                    index_name,
                    tokenizer_mode,
                ))
                .map_err(error_to_string)
        })
    })();
    match result {
        Ok(result) => {
            write_out_ptr(
                result_out,
                Box::into_raw(Box::new(map_sqlite_rebuild_fts_index_result(result))),
            );
            ffi_ok_status(error_out)
        }
        Err(error) => ffi_error_status(error_out, error),
    }
}

/// Rebuild one SQLite FTS index using JSON mode.
/// 使用 JSON 模式重建一条 SQLite FTS 索引。
#[unsafe(no_mangle)]
pub extern "C" fn vldb_controller_ffi_client_rebuild_sqlite_fts_index_json(
    client: *mut FfiControllerClientHandle,
    request_json: *const c_char,
    response_out: *mut *mut c_char,
    error_out: *mut *mut c_char,
) -> c_int {
    clear_out_ptr(response_out);
    let result = (|| -> Result<String, String> {
        let request: RebuildSqliteFtsIndexJsonRequest = parse_json_input(request_json)?;
        let binding_id = request.space_id.clone();
        let tokenizer_mode = parse_sqlite_tokenizer_mode_name(&request.tokenizer_mode)?;
        let result = with_client_handle(client, |handle| {
            handle
                .runtime
                .block_on(handle.client.rebuild_sqlite_fts_index(
                    request.space_id,
                    binding_id,
                    request.index_name,
                    tokenizer_mode,
                ))
                .map_err(error_to_string)
        })?;
        serde_json::to_string(&result).map_err(|error| {
            format!("failed to serialize rebuild_sqlite_fts_index_json response: {error}")
        })
    })();
    match result {
        Ok(response) => {
            write_out_ptr(response_out, string_into_raw(response));
            ffi_ok_status(error_out)
        }
        Err(error) => ffi_error_status(error_out, error),
    }
}

/// Free one native SQLite rebuild-FTS result.
/// 释放一份原生 SQLite FTS 重建结果。
#[unsafe(no_mangle)]
pub extern "C" fn vldb_controller_ffi_sqlite_rebuild_fts_index_result_free(
    result: *mut FfiControllerSqliteRebuildFtsIndexResult,
) {
    if !result.is_null() {
        unsafe {
            let result = Box::from_raw(result);
            vldb_controller_ffi_string_free(result.message);
            vldb_controller_ffi_string_free(result.index_name);
            vldb_controller_ffi_string_free(result.tokenizer_mode);
        }
    }
}

/// Upsert one SQLite FTS document using native arguments.
/// 使用原生参数写入一条 SQLite FTS 文档。
#[unsafe(no_mangle)]
#[allow(clippy::too_many_arguments)]
pub extern "C" fn vldb_controller_ffi_client_upsert_sqlite_fts_document(
    client: *mut FfiControllerClientHandle,
    space_id: *const c_char,
    index_name: *const c_char,
    tokenizer_mode: *const c_char,
    id: *const c_char,
    file_path: *const c_char,
    title: *const c_char,
    content: *const c_char,
    result_out: *mut *mut FfiControllerSqliteFtsMutationResult,
    error_out: *mut *mut c_char,
) -> c_int {
    clear_out_ptr(result_out);
    let result = (|| -> Result<ControllerSqliteFtsMutationResult, String> {
        let space_id = required_c_string(space_id, "space_id")?;
        let binding_id = space_id.clone();
        let index_name = required_c_string(index_name, "index_name")?;
        let tokenizer_mode = parse_sqlite_tokenizer_mode_text(tokenizer_mode)?;
        let id = required_c_string(id, "id")?;
        let file_path = required_c_string_preserve(file_path, "file_path")?;
        let title = required_c_string_preserve(title, "title")?;
        let content = required_c_string_preserve(content, "content")?;
        with_client_handle(client, |handle| {
            handle
                .runtime
                .block_on(handle.client.upsert_sqlite_fts_document(
                    space_id,
                    binding_id,
                    index_name,
                    tokenizer_mode,
                    id,
                    file_path,
                    title,
                    content,
                ))
                .map_err(error_to_string)
        })
    })();
    match result {
        Ok(result) => {
            write_out_ptr(
                result_out,
                Box::into_raw(Box::new(map_sqlite_fts_mutation_result(result))),
            );
            ffi_ok_status(error_out)
        }
        Err(error) => ffi_error_status(error_out, error),
    }
}

/// Upsert one SQLite FTS document using JSON mode.
/// 使用 JSON 模式写入一条 SQLite FTS 文档。
#[unsafe(no_mangle)]
pub extern "C" fn vldb_controller_ffi_client_upsert_sqlite_fts_document_json(
    client: *mut FfiControllerClientHandle,
    request_json: *const c_char,
    response_out: *mut *mut c_char,
    error_out: *mut *mut c_char,
) -> c_int {
    clear_out_ptr(response_out);
    let result = (|| -> Result<String, String> {
        let request: UpsertSqliteFtsDocumentJsonRequest = parse_json_input(request_json)?;
        let binding_id = request.space_id.clone();
        let tokenizer_mode = parse_sqlite_tokenizer_mode_name(&request.tokenizer_mode)?;
        let result = with_client_handle(client, |handle| {
            handle
                .runtime
                .block_on(handle.client.upsert_sqlite_fts_document(
                    request.space_id,
                    binding_id,
                    request.index_name,
                    tokenizer_mode,
                    request.id,
                    request.file_path,
                    request.title,
                    request.content,
                ))
                .map_err(error_to_string)
        })?;
        serde_json::to_string(&result).map_err(|error| {
            format!("failed to serialize upsert_sqlite_fts_document_json response: {error}")
        })
    })();
    match result {
        Ok(response) => {
            write_out_ptr(response_out, string_into_raw(response));
            ffi_ok_status(error_out)
        }
        Err(error) => ffi_error_status(error_out, error),
    }
}

/// Delete one SQLite FTS document using native arguments.
/// 使用原生参数删除一条 SQLite FTS 文档。
#[unsafe(no_mangle)]
pub extern "C" fn vldb_controller_ffi_client_delete_sqlite_fts_document(
    client: *mut FfiControllerClientHandle,
    space_id: *const c_char,
    index_name: *const c_char,
    id: *const c_char,
    result_out: *mut *mut FfiControllerSqliteFtsMutationResult,
    error_out: *mut *mut c_char,
) -> c_int {
    clear_out_ptr(result_out);
    let result = (|| -> Result<ControllerSqliteFtsMutationResult, String> {
        let space_id = required_c_string(space_id, "space_id")?;
        let binding_id = space_id.clone();
        let index_name = required_c_string(index_name, "index_name")?;
        let id = required_c_string(id, "id")?;
        with_client_handle(client, |handle| {
            handle
                .runtime
                .block_on(
                    handle
                        .client
                        .delete_sqlite_fts_document(space_id, binding_id, index_name, id),
                )
                .map_err(error_to_string)
        })
    })();
    match result {
        Ok(result) => {
            write_out_ptr(
                result_out,
                Box::into_raw(Box::new(map_sqlite_fts_mutation_result(result))),
            );
            ffi_ok_status(error_out)
        }
        Err(error) => ffi_error_status(error_out, error),
    }
}

/// Delete one SQLite FTS document using JSON mode.
/// 使用 JSON 模式删除一条 SQLite FTS 文档。
#[unsafe(no_mangle)]
pub extern "C" fn vldb_controller_ffi_client_delete_sqlite_fts_document_json(
    client: *mut FfiControllerClientHandle,
    request_json: *const c_char,
    response_out: *mut *mut c_char,
    error_out: *mut *mut c_char,
) -> c_int {
    clear_out_ptr(response_out);
    let result = (|| -> Result<String, String> {
        let request: DeleteSqliteFtsDocumentJsonRequest = parse_json_input(request_json)?;
        let binding_id = request.space_id.clone();
        let result = with_client_handle(client, |handle| {
            handle
                .runtime
                .block_on(handle.client.delete_sqlite_fts_document(
                    request.space_id,
                    binding_id,
                    request.index_name,
                    request.id,
                ))
                .map_err(error_to_string)
        })?;
        serde_json::to_string(&result).map_err(|error| {
            format!("failed to serialize delete_sqlite_fts_document_json response: {error}")
        })
    })();
    match result {
        Ok(response) => {
            write_out_ptr(response_out, string_into_raw(response));
            ffi_ok_status(error_out)
        }
        Err(error) => ffi_error_status(error_out, error),
    }
}

/// Free one native SQLite FTS document mutation result.
/// 释放一份原生 SQLite FTS 文档变更结果。
#[unsafe(no_mangle)]
pub extern "C" fn vldb_controller_ffi_sqlite_fts_mutation_result_free(
    result: *mut FfiControllerSqliteFtsMutationResult,
) {
    if !result.is_null() {
        unsafe {
            let result = Box::from_raw(result);
            vldb_controller_ffi_string_free(result.message);
            vldb_controller_ffi_string_free(result.index_name);
        }
    }
}

/// Search one SQLite FTS index using native arguments.
/// 使用原生参数检索一条 SQLite FTS 索引。
#[unsafe(no_mangle)]
pub extern "C" fn vldb_controller_ffi_client_search_sqlite_fts(
    client: *mut FfiControllerClientHandle,
    space_id: *const c_char,
    index_name: *const c_char,
    tokenizer_mode: *const c_char,
    query: *const c_char,
    limit: u32,
    offset: u32,
    result_out: *mut *mut FfiControllerSqliteSearchFtsResult,
    error_out: *mut *mut c_char,
) -> c_int {
    clear_out_ptr(result_out);
    let result = (|| -> Result<ControllerSqliteSearchFtsResult, String> {
        let space_id = required_c_string(space_id, "space_id")?;
        let binding_id = space_id.clone();
        let index_name = required_c_string(index_name, "index_name")?;
        let tokenizer_mode = parse_sqlite_tokenizer_mode_text(tokenizer_mode)?;
        let query = required_c_string(query, "query")?;
        with_client_handle(client, |handle| {
            handle
                .runtime
                .block_on(handle.client.search_sqlite_fts(
                    space_id,
                    binding_id,
                    index_name,
                    tokenizer_mode,
                    query,
                    limit,
                    offset,
                ))
                .map_err(error_to_string)
        })
    })();
    match result {
        Ok(result) => {
            write_out_ptr(
                result_out,
                Box::into_raw(Box::new(map_sqlite_search_fts_result(result))),
            );
            ffi_ok_status(error_out)
        }
        Err(error) => ffi_error_status(error_out, error),
    }
}

/// Search one SQLite FTS index using JSON mode.
/// 使用 JSON 模式检索一条 SQLite FTS 索引。
#[unsafe(no_mangle)]
pub extern "C" fn vldb_controller_ffi_client_search_sqlite_fts_json(
    client: *mut FfiControllerClientHandle,
    request_json: *const c_char,
    response_out: *mut *mut c_char,
    error_out: *mut *mut c_char,
) -> c_int {
    clear_out_ptr(response_out);
    let result = (|| -> Result<String, String> {
        let request: SearchSqliteFtsJsonRequest = parse_json_input(request_json)?;
        let binding_id = request.space_id.clone();
        let tokenizer_mode = parse_sqlite_tokenizer_mode_name(&request.tokenizer_mode)?;
        let result = with_client_handle(client, |handle| {
            handle
                .runtime
                .block_on(handle.client.search_sqlite_fts(
                    request.space_id,
                    binding_id,
                    request.index_name,
                    tokenizer_mode,
                    request.query,
                    request.limit,
                    request.offset,
                ))
                .map_err(error_to_string)
        })?;
        serde_json::to_string(&result).map_err(|error| {
            format!("failed to serialize search_sqlite_fts_json response: {error}")
        })
    })();
    match result {
        Ok(response) => {
            write_out_ptr(response_out, string_into_raw(response));
            ffi_ok_status(error_out)
        }
        Err(error) => ffi_error_status(error_out, error),
    }
}

/// Free one native SQLite FTS hit array.
/// 释放一份原生 SQLite FTS 命中数组。
#[unsafe(no_mangle)]
pub extern "C" fn vldb_controller_ffi_sqlite_search_fts_hit_array_free(
    value: *mut FfiControllerSqliteSearchFtsHitArray,
) {
    if value.is_null() {
        return;
    }
    unsafe {
        let value = Box::from_raw(value);
        if !value.items.is_null() {
            let items = Vec::from_raw_parts(value.items, value.len, value.len);
            for item in items {
                vldb_controller_ffi_string_free(item.id);
                vldb_controller_ffi_string_free(item.file_path);
                vldb_controller_ffi_string_free(item.title);
                vldb_controller_ffi_string_free(item.title_highlight);
                vldb_controller_ffi_string_free(item.content_snippet);
            }
        }
    }
}

/// Free one native SQLite FTS search result.
/// 释放一份原生 SQLite FTS 检索结果。
#[unsafe(no_mangle)]
pub extern "C" fn vldb_controller_ffi_sqlite_search_fts_result_free(
    result: *mut FfiControllerSqliteSearchFtsResult,
) {
    if !result.is_null() {
        unsafe {
            let result = Box::from_raw(result);
            vldb_controller_ffi_string_free(result.message);
            vldb_controller_ffi_string_free(result.index_name);
            vldb_controller_ffi_string_free(result.tokenizer_mode);
            vldb_controller_ffi_string_free(result.normalized_query);
            vldb_controller_ffi_string_free(result.fts_query);
            vldb_controller_ffi_string_free(result.source);
            vldb_controller_ffi_string_free(result.query_mode);
            vldb_controller_ffi_sqlite_search_fts_hit_array_free(result.hits);
        }
    }
}

/// Enable one LanceDB backend using native structured input.
/// 使用原生结构化输入启用一个 LanceDB 后端。
#[unsafe(no_mangle)]
pub extern "C" fn vldb_controller_ffi_client_enable_lancedb(
    client: *mut FfiControllerClientHandle,
    request: *const FfiControllerLanceDbEnableRequest,
    error_out: *mut *mut c_char,
) -> c_int {
    let result = (|| -> Result<(), String> {
        let request = ffi_lancedb_enable_request_to_rust(request)?;
        with_client_handle(client, |handle| {
            handle
                .runtime
                .block_on(handle.client.enable_lancedb(request))
                .map_err(error_to_string)
        })
    })();
    match result {
        Ok(()) => ffi_ok_status(error_out),
        Err(error) => ffi_error_status(error_out, error),
    }
}

/// Enable one LanceDB backend using JSON mode.
/// 使用 JSON 模式启用一个 LanceDB 后端。
#[unsafe(no_mangle)]
pub extern "C" fn vldb_controller_ffi_client_enable_lancedb_json(
    client: *mut FfiControllerClientHandle,
    request_json: *const c_char,
    response_out: *mut *mut c_char,
    error_out: *mut *mut c_char,
) -> c_int {
    clear_out_ptr(response_out);
    let result = (|| -> Result<String, String> {
        let request: EnableLanceDbJsonRequest = parse_json_input(request_json)?;
        with_client_handle(client, |handle| {
            handle
                .runtime
                .block_on(handle.client.enable_lancedb(request.request))
                .map_err(error_to_string)
        })?;
        serde_json::to_string(&SuccessJsonResponse { ok: true })
            .map_err(|error| format!("failed to serialize enable_lancedb_json response: {error}"))
    })();
    match result {
        Ok(response) => {
            write_out_ptr(response_out, string_into_raw(response));
            ffi_ok_status(error_out)
        }
        Err(error) => ffi_error_status(error_out, error),
    }
}

/// Disable one LanceDB backend using native structured input.
/// 使用原生结构化输入关闭一个 LanceDB 后端。
#[unsafe(no_mangle)]
pub extern "C" fn vldb_controller_ffi_client_disable_lancedb(
    client: *mut FfiControllerClientHandle,
    space_id: *const c_char,
    disabled_out: *mut c_uchar,
    error_out: *mut *mut c_char,
) -> c_int {
    clear_out_u8(disabled_out);
    let result = (|| -> Result<bool, String> {
        let space_id = required_c_string(space_id, "space_id")?;
        let binding_id = space_id.clone();
        with_client_handle(client, |handle| {
            handle
                .runtime
                .block_on(handle.client.disable_lancedb(space_id, binding_id))
                .map_err(error_to_string)
        })
    })();
    match result {
        Ok(disabled) => {
            write_out_u8(disabled_out, if disabled { 1 } else { 0 });
            ffi_ok_status(error_out)
        }
        Err(error) => ffi_error_status(error_out, error),
    }
}

/// Disable one LanceDB backend using JSON mode.
/// 使用 JSON 模式关闭一个 LanceDB 后端。
#[unsafe(no_mangle)]
pub extern "C" fn vldb_controller_ffi_client_disable_lancedb_json(
    client: *mut FfiControllerClientHandle,
    request_json: *const c_char,
    response_out: *mut *mut c_char,
    error_out: *mut *mut c_char,
) -> c_int {
    clear_out_ptr(response_out);
    let result = (|| -> Result<String, String> {
        let request: DisableBackendJsonRequest = parse_json_input(request_json)?;
        let binding_id = request.space_id.clone();
        let disabled = with_client_handle(client, |handle| {
            handle
                .runtime
                .block_on(handle.client.disable_lancedb(request.space_id, binding_id))
                .map_err(error_to_string)
        })?;
        serde_json::to_string(&disabled)
            .map_err(|error| format!("failed to serialize disable_lancedb_json response: {error}"))
    })();
    match result {
        Ok(response) => {
            write_out_ptr(response_out, string_into_raw(response));
            ffi_ok_status(error_out)
        }
        Err(error) => ffi_error_status(error_out, error),
    }
}

/// Create one LanceDB table using native input.
/// 使用原生输入创建一张 LanceDB 表。
#[unsafe(no_mangle)]
pub extern "C" fn vldb_controller_ffi_client_create_lancedb_table(
    client: *mut FfiControllerClientHandle,
    space_id: *const c_char,
    table_name: *const c_char,
    columns: *const FfiControllerLanceDbColumnDef,
    columns_len: usize,
    overwrite_if_exists: c_uchar,
    result_out: *mut *mut FfiControllerLanceDbCreateTableResult,
    error_out: *mut *mut c_char,
) -> c_int {
    clear_out_ptr(result_out);
    let result = (|| -> Result<ControllerLanceDbCreateTableResult, String> {
        let space_id = required_c_string(space_id, "space_id")?;
        let binding_id = space_id.clone();
        let table_name = required_c_string(table_name, "table_name")?;
        let columns = read_lancedb_columns(columns, columns_len, "columns")?;
        with_client_handle(client, |handle| {
            handle
                .runtime
                .block_on(handle.client.create_lancedb_table_typed(
                    space_id,
                    binding_id,
                    table_name,
                    columns,
                    overwrite_if_exists != 0,
                ))
                .map_err(error_to_string)
        })
    })();
    match result {
        Ok(result) => {
            write_out_ptr(
                result_out,
                Box::into_raw(Box::new(map_lancedb_create_table_result(result))),
            );
            ffi_ok_status(error_out)
        }
        Err(error) => ffi_error_status(error_out, error),
    }
}

/// Create one LanceDB table using JSON mode.
/// 使用 JSON 模式创建一张 LanceDB 表。
#[unsafe(no_mangle)]
pub extern "C" fn vldb_controller_ffi_client_create_lancedb_table_json(
    client: *mut FfiControllerClientHandle,
    request_json: *const c_char,
    response_out: *mut *mut c_char,
    error_out: *mut *mut c_char,
) -> c_int {
    clear_out_ptr(response_out);
    let result = (|| -> Result<String, String> {
        let request: CreateLanceDbTableJsonRequest = parse_json_input(request_json)?;
        let space_id = request.space_id.clone();
        let binding_id = request.space_id.clone();
        let result = with_client_handle(client, |handle| {
            let inner_request_json =
                serde_json::to_string(&CreateLanceDbTableJsonCompatRequest::from(request))
                    .map_err(|error| {
                        format!("failed to serialize create_lancedb_table_json request: {error}")
                    })?;
            handle
                .runtime
                .block_on(handle.client.create_lancedb_table(
                    space_id,
                    binding_id,
                    inner_request_json,
                ))
                .map_err(error_to_string)
        })?;
        serde_json::to_string(&result).map_err(|error| {
            format!("failed to serialize create_lancedb_table_json response: {error}")
        })
    })();
    match result {
        Ok(response) => {
            write_out_ptr(response_out, string_into_raw(response));
            ffi_ok_status(error_out)
        }
        Err(error) => ffi_error_status(error_out, error),
    }
}

/// Free one native LanceDB create-table result.
/// 释放一份原生 LanceDB 建表结果。
#[unsafe(no_mangle)]
pub extern "C" fn vldb_controller_ffi_lancedb_create_table_result_free(
    result: *mut FfiControllerLanceDbCreateTableResult,
) {
    if !result.is_null() {
        unsafe {
            let result = Box::from_raw(result);
            vldb_controller_ffi_string_free(result.message);
        }
    }
}

/// Upsert LanceDB rows using native input.
/// 使用原生输入写入 LanceDB 行数据。
#[unsafe(no_mangle)]
pub extern "C" fn vldb_controller_ffi_client_upsert_lancedb(
    client: *mut FfiControllerClientHandle,
    space_id: *const c_char,
    table_name: *const c_char,
    input_format: c_int,
    data: *const u8,
    data_len: usize,
    key_columns: *const *const c_char,
    key_columns_len: usize,
    result_out: *mut *mut FfiControllerLanceDbUpsertResult,
    error_out: *mut *mut c_char,
) -> c_int {
    clear_out_ptr(result_out);
    let result = (|| -> Result<ControllerLanceDbUpsertResult, String> {
        let space_id = required_c_string(space_id, "space_id")?;
        let binding_id = space_id.clone();
        let table_name = required_c_string(table_name, "table_name")?;
        let input_format = map_lancedb_input_format_native(input_format)?;
        let data = required_bytes(data, data_len, "data")?.to_vec();
        let key_columns = optional_string_array(key_columns, key_columns_len, "key_columns")?;
        with_client_handle(client, |handle| {
            handle
                .runtime
                .block_on(handle.client.upsert_lancedb_typed(
                    space_id,
                    binding_id,
                    table_name,
                    input_format,
                    data,
                    key_columns,
                ))
                .map_err(error_to_string)
        })
    })();
    match result {
        Ok(result) => {
            write_out_ptr(
                result_out,
                Box::into_raw(Box::new(map_lancedb_upsert_result(result))),
            );
            ffi_ok_status(error_out)
        }
        Err(error) => ffi_error_status(error_out, error),
    }
}

/// Upsert LanceDB rows using JSON mode.
/// 使用 JSON 模式写入 LanceDB 行数据。
#[unsafe(no_mangle)]
pub extern "C" fn vldb_controller_ffi_client_upsert_lancedb_json(
    client: *mut FfiControllerClientHandle,
    request_json: *const c_char,
    response_out: *mut *mut c_char,
    error_out: *mut *mut c_char,
) -> c_int {
    clear_out_ptr(response_out);
    let result = (|| -> Result<String, String> {
        let request: UpsertLanceDbJsonRequest = parse_json_input(request_json)?;
        let binding_id = request.space_id.clone();
        let data = decode_base64(&request.data_base64)?;
        let result = with_client_handle(client, |handle| {
            let inner_request_json = serde_json::to_string(&UpsertLanceDbJsonCompatRequest::from(
                &request,
            ))
            .map_err(|error| format!("failed to serialize upsert_lancedb_json request: {error}"))?;
            handle
                .runtime
                .block_on(handle.client.upsert_lancedb(
                    request.space_id,
                    binding_id,
                    inner_request_json,
                    data,
                ))
                .map_err(error_to_string)
        })?;
        serde_json::to_string(&result)
            .map_err(|error| format!("failed to serialize upsert_lancedb_json response: {error}"))
    })();
    match result {
        Ok(response) => {
            write_out_ptr(response_out, string_into_raw(response));
            ffi_ok_status(error_out)
        }
        Err(error) => ffi_error_status(error_out, error),
    }
}

/// Free one native LanceDB upsert result.
/// 释放一份原生 LanceDB 写入结果。
#[unsafe(no_mangle)]
pub extern "C" fn vldb_controller_ffi_lancedb_upsert_result_free(
    result: *mut FfiControllerLanceDbUpsertResult,
) {
    if !result.is_null() {
        unsafe {
            let result = Box::from_raw(result);
            vldb_controller_ffi_string_free(result.message);
        }
    }
}

/// Search LanceDB rows using native input.
/// 使用原生输入检索 LanceDB 行数据。
#[unsafe(no_mangle)]
pub extern "C" fn vldb_controller_ffi_client_search_lancedb(
    client: *mut FfiControllerClientHandle,
    space_id: *const c_char,
    table_name: *const c_char,
    vector: *const c_float,
    vector_len: usize,
    limit: u32,
    filter: *const c_char,
    vector_column: *const c_char,
    output_format: c_int,
    result_out: *mut *mut FfiControllerLanceDbSearchResult,
    error_out: *mut *mut c_char,
) -> c_int {
    clear_out_ptr(result_out);
    let result = (|| -> Result<ControllerLanceDbSearchResult, String> {
        let space_id = required_c_string(space_id, "space_id")?;
        let binding_id = space_id.clone();
        let table_name = required_c_string(table_name, "table_name")?;
        let vector = required_f32_slice(vector, vector_len, "vector")?.to_vec();
        let filter = optional_c_string(filter)?.unwrap_or_default();
        let vector_column = optional_c_string(vector_column)?.unwrap_or_default();
        let output_format = map_lancedb_output_format_native(output_format)?;
        with_client_handle(client, |handle| {
            handle
                .runtime
                .block_on(handle.client.search_lancedb_typed(
                    space_id,
                    binding_id,
                    table_name,
                    vector,
                    limit,
                    filter,
                    vector_column,
                    output_format,
                ))
                .map_err(error_to_string)
        })
    })();
    match result {
        Ok(result) => {
            write_out_ptr(
                result_out,
                Box::into_raw(Box::new(map_lancedb_search_result(result))),
            );
            ffi_ok_status(error_out)
        }
        Err(error) => ffi_error_status(error_out, error),
    }
}

/// Search LanceDB rows using JSON mode.
/// 使用 JSON 模式检索 LanceDB 行数据。
#[unsafe(no_mangle)]
pub extern "C" fn vldb_controller_ffi_client_search_lancedb_json(
    client: *mut FfiControllerClientHandle,
    request_json: *const c_char,
    response_out: *mut *mut c_char,
    error_out: *mut *mut c_char,
) -> c_int {
    clear_out_ptr(response_out);
    let result = (|| -> Result<String, String> {
        let request: SearchLanceDbJsonRequest = parse_json_input(request_json)?;
        let binding_id = request.space_id.clone();
        let result = with_client_handle(client, |handle| {
            let inner_request_json = serde_json::to_string(&SearchLanceDbJsonCompatRequest::from(
                &request,
            ))
            .map_err(|error| format!("failed to serialize search_lancedb_json request: {error}"))?;
            handle
                .runtime
                .block_on(handle.client.search_lancedb(
                    request.space_id,
                    binding_id,
                    inner_request_json,
                ))
                .map_err(error_to_string)
        })?;
        serde_json::to_string(&SearchLanceDbJsonResponse::from(result))
            .map_err(|error| format!("failed to serialize search_lancedb_json response: {error}"))
    })();
    match result {
        Ok(response) => {
            write_out_ptr(response_out, string_into_raw(response));
            ffi_ok_status(error_out)
        }
        Err(error) => ffi_error_status(error_out, error),
    }
}

/// Free one native LanceDB search result.
/// 释放一份原生 LanceDB 检索结果。
#[unsafe(no_mangle)]
pub extern "C" fn vldb_controller_ffi_lancedb_search_result_free(
    result: *mut FfiControllerLanceDbSearchResult,
) {
    if !result.is_null() {
        unsafe {
            let result = Box::from_raw(result);
            vldb_controller_ffi_string_free(result.message);
            vldb_controller_ffi_string_free(result.format);
            vldb_controller_ffi_bytes_free(result.data, result.data_len);
        }
    }
}

/// Delete LanceDB rows using native input.
/// 使用原生输入删除 LanceDB 行数据。
#[unsafe(no_mangle)]
pub extern "C" fn vldb_controller_ffi_client_delete_lancedb(
    client: *mut FfiControllerClientHandle,
    space_id: *const c_char,
    table_name: *const c_char,
    condition: *const c_char,
    result_out: *mut *mut FfiControllerLanceDbDeleteResult,
    error_out: *mut *mut c_char,
) -> c_int {
    clear_out_ptr(result_out);
    let result = (|| -> Result<ControllerLanceDbDeleteResult, String> {
        let space_id = required_c_string(space_id, "space_id")?;
        let binding_id = space_id.clone();
        let table_name = required_c_string(table_name, "table_name")?;
        let condition = required_c_string(condition, "condition")?;
        with_client_handle(client, |handle| {
            handle
                .runtime
                .block_on(
                    handle
                        .client
                        .delete_lancedb_typed(space_id, binding_id, table_name, condition),
                )
                .map_err(error_to_string)
        })
    })();
    match result {
        Ok(result) => {
            write_out_ptr(
                result_out,
                Box::into_raw(Box::new(map_lancedb_delete_result(result))),
            );
            ffi_ok_status(error_out)
        }
        Err(error) => ffi_error_status(error_out, error),
    }
}

/// Delete LanceDB rows using JSON mode.
/// 使用 JSON 模式删除 LanceDB 行数据。
#[unsafe(no_mangle)]
pub extern "C" fn vldb_controller_ffi_client_delete_lancedb_json(
    client: *mut FfiControllerClientHandle,
    request_json: *const c_char,
    response_out: *mut *mut c_char,
    error_out: *mut *mut c_char,
) -> c_int {
    clear_out_ptr(response_out);
    let result = (|| -> Result<String, String> {
        let request: DeleteLanceDbJsonRequest = parse_json_input(request_json)?;
        let binding_id = request.space_id.clone();
        let result = with_client_handle(client, |handle| {
            let inner_request_json = serde_json::to_string(&DeleteLanceDbJsonCompatRequest::from(
                &request,
            ))
            .map_err(|error| format!("failed to serialize delete_lancedb_json request: {error}"))?;
            handle
                .runtime
                .block_on(handle.client.delete_lancedb(
                    request.space_id,
                    binding_id,
                    inner_request_json,
                ))
                .map_err(error_to_string)
        })?;
        serde_json::to_string(&result)
            .map_err(|error| format!("failed to serialize delete_lancedb_json response: {error}"))
    })();
    match result {
        Ok(response) => {
            write_out_ptr(response_out, string_into_raw(response));
            ffi_ok_status(error_out)
        }
        Err(error) => ffi_error_status(error_out, error),
    }
}

/// Free one native LanceDB delete result.
/// 释放一份原生 LanceDB 删除结果。
#[unsafe(no_mangle)]
pub extern "C" fn vldb_controller_ffi_lancedb_delete_result_free(
    result: *mut FfiControllerLanceDbDeleteResult,
) {
    if !result.is_null() {
        unsafe {
            let result = Box::from_raw(result);
            vldb_controller_ffi_string_free(result.message);
        }
    }
}

/// Drop one LanceDB table using native input.
/// 使用原生输入删除一张 LanceDB 表。
#[unsafe(no_mangle)]
pub extern "C" fn vldb_controller_ffi_client_drop_lancedb_table(
    client: *mut FfiControllerClientHandle,
    space_id: *const c_char,
    table_name: *const c_char,
    result_out: *mut *mut FfiControllerLanceDbDropTableResult,
    error_out: *mut *mut c_char,
) -> c_int {
    clear_out_ptr(result_out);
    let result = (|| -> Result<ControllerLanceDbDropTableResult, String> {
        let space_id = required_c_string(space_id, "space_id")?;
        let binding_id = space_id.clone();
        let table_name = required_c_string(table_name, "table_name")?;
        with_client_handle(client, |handle| {
            handle
                .runtime
                .block_on(
                    handle
                        .client
                        .drop_lancedb_table(space_id, binding_id, table_name),
                )
                .map_err(error_to_string)
        })
    })();
    match result {
        Ok(result) => {
            write_out_ptr(
                result_out,
                Box::into_raw(Box::new(map_lancedb_drop_table_result(result))),
            );
            ffi_ok_status(error_out)
        }
        Err(error) => ffi_error_status(error_out, error),
    }
}

/// Drop one LanceDB table using JSON mode.
/// 使用 JSON 模式删除一张 LanceDB 表。
#[unsafe(no_mangle)]
pub extern "C" fn vldb_controller_ffi_client_drop_lancedb_table_json(
    client: *mut FfiControllerClientHandle,
    request_json: *const c_char,
    response_out: *mut *mut c_char,
    error_out: *mut *mut c_char,
) -> c_int {
    clear_out_ptr(response_out);
    let result = (|| -> Result<String, String> {
        let request: DropLanceDbTableJsonRequest = parse_json_input(request_json)?;
        let binding_id = request.space_id.clone();
        let result = with_client_handle(client, |handle| {
            handle
                .runtime
                .block_on(handle.client.drop_lancedb_table(
                    request.space_id,
                    binding_id,
                    request.table_name,
                ))
                .map_err(error_to_string)
        })?;
        serde_json::to_string(&result).map_err(|error| {
            format!("failed to serialize drop_lancedb_table_json response: {error}")
        })
    })();
    match result {
        Ok(response) => {
            write_out_ptr(response_out, string_into_raw(response));
            ffi_ok_status(error_out)
        }
        Err(error) => ffi_error_status(error_out, error),
    }
}

/// Free one native LanceDB drop-table result.
/// 释放一份原生 LanceDB 删表结果。
#[unsafe(no_mangle)]
pub extern "C" fn vldb_controller_ffi_lancedb_drop_table_result_free(
    result: *mut FfiControllerLanceDbDropTableResult,
) {
    if !result.is_null() {
        unsafe {
            let result = Box::from_raw(result);
            vldb_controller_ffi_string_free(result.message);
        }
    }
}

/// JSON-friendly LanceDB search response that encodes binary payloads as base64 text.
/// 将二进制载荷编码成 base64 文本的 JSON 友好 LanceDB 检索响应。
#[derive(Serialize)]
struct SearchLanceDbJsonResponse {
    message: String,
    format: String,
    rows: u64,
    data_base64: String,
}

impl From<ControllerLanceDbSearchResult> for SearchLanceDbJsonResponse {
    /// Convert one native search result into a JSON-safe response.
    /// 将一份原生检索结果转换成 JSON 安全响应。
    fn from(value: ControllerLanceDbSearchResult) -> Self {
        Self {
            message: value.message,
            format: value.format,
            rows: value.rows,
            data_base64: encode_base64(&value.data),
        }
    }
}

/// Build one client handle from already parsed Rust values.
/// 从已解析的 Rust 值构造一个客户端句柄。
fn build_client_handle(
    config: ControllerClientConfig,
    registration: ClientRegistration,
) -> Result<FfiControllerClientHandle, String> {
    let runtime =
        Runtime::new().map_err(|error| format!("failed to create tokio runtime: {error}"))?;
    let client = ControllerClient::new(config, registration);
    Ok(FfiControllerClientHandle { runtime, client })
}

/// Build one client handle from native input structures.
/// 从原生输入结构构造一个客户端句柄。
fn native_client_create(
    config: *const FfiControllerClientConfig,
    registration: *const FfiClientRegistration,
) -> Result<FfiControllerClientHandle, String> {
    let config = ffi_client_config_to_rust(config)?;
    let registration = ffi_client_registration_to_rust(registration)?;
    build_client_handle(config, registration)
}

/// Convert one native client configuration into the Rust SDK type.
/// 将一份原生客户端配置转换成 Rust SDK 类型。
fn ffi_client_config_to_rust(
    config: *const FfiControllerClientConfig,
) -> Result<ControllerClientConfig, String> {
    if config.is_null() {
        return Err("config pointer must not be null".to_string());
    }
    let config = unsafe { &*config };
    let defaults = ControllerClientConfig::default();
    Ok(ControllerClientConfig {
        endpoint: optional_c_string(config.endpoint)?.unwrap_or(defaults.endpoint),
        auto_spawn: config.auto_spawn != 0,
        spawn_executable: optional_c_string(config.spawn_executable)?,
        spawn_process_mode: map_process_mode_native(config.spawn_process_mode)?,
        minimum_uptime_secs: default_u64(config.minimum_uptime_secs, defaults.minimum_uptime_secs),
        idle_timeout_secs: default_u64(config.idle_timeout_secs, defaults.idle_timeout_secs),
        default_lease_ttl_secs: default_u64(
            config.default_lease_ttl_secs,
            defaults.default_lease_ttl_secs,
        ),
        connect_timeout_secs: default_u64(
            config.connect_timeout_secs,
            defaults.connect_timeout_secs,
        ),
        startup_timeout_secs: default_u64(
            config.startup_timeout_secs,
            defaults.startup_timeout_secs,
        ),
        startup_retry_interval_ms: default_u64(
            config.startup_retry_interval_ms,
            defaults.startup_retry_interval_ms,
        ),
        lease_renew_interval_secs: default_u64(
            config.lease_renew_interval_secs,
            defaults.lease_renew_interval_secs,
        ),
    })
}

/// Convert one native client registration into the Rust SDK type.
/// 将一份原生客户端注册转换成 Rust SDK 类型。
fn ffi_client_registration_to_rust(
    registration: *const FfiClientRegistration,
) -> Result<ClientRegistration, String> {
    if registration.is_null() {
        return Err("registration pointer must not be null".to_string());
    }
    let registration = unsafe { &*registration };
    Ok(ClientRegistration {
        client_id: required_c_string(registration.client_id, "client_id")?,
        host_kind: required_c_string(registration.host_kind, "host_kind")?,
        process_id: registration.process_id,
        process_name: required_c_string(registration.process_name, "process_name")?,
        lease_ttl_secs: optional_nonzero_u64(registration.lease_ttl_secs),
    })
}

/// Convert one native space registration into the Rust SDK type.
/// 将一份原生空间注册转换成 Rust SDK 类型。
fn ffi_space_registration_to_rust(
    registration: *const FfiSpaceRegistration,
) -> Result<SpaceRegistration, String> {
    if registration.is_null() {
        return Err("registration pointer must not be null".to_string());
    }
    let registration = unsafe { &*registration };
    Ok(SpaceRegistration {
        space_id: required_c_string(registration.space_id, "space_id")?,
        space_label: required_c_string(registration.space_label, "space_label")?,
        space_kind: map_space_kind_native(registration.space_kind)?,
        space_root: required_c_string(registration.space_root, "space_root")?,
    })
}

/// Convert one native SQLite enable request into the Rust SDK type.
/// 将一份原生 SQLite 启用请求转换成 Rust SDK 类型。
fn ffi_sqlite_enable_request_to_rust(
    request: *const FfiControllerSqliteEnableRequest,
) -> Result<ControllerSqliteEnableRequest, String> {
    if request.is_null() {
        return Err("sqlite enable request pointer must not be null".to_string());
    }
    let request = unsafe { &*request };
    let defaults = ControllerSqliteEnableRequest::default();
    let space_id = required_c_string(request.space_id, "space_id")?;
    Ok(ControllerSqliteEnableRequest {
        space_id: space_id.clone(),
        binding_id: space_id,
        db_path: required_c_string(request.db_path, "db_path")?,
        connection_pool_size: default_u64(
            request.connection_pool_size,
            defaults.connection_pool_size as u64,
        ) as usize,
        busy_timeout_ms: default_u64(request.busy_timeout_ms, defaults.busy_timeout_ms),
        journal_mode: optional_c_string(request.journal_mode)?.unwrap_or(defaults.journal_mode),
        synchronous: optional_c_string(request.synchronous)?.unwrap_or(defaults.synchronous),
        foreign_keys: request.foreign_keys != 0,
        temp_store: optional_c_string(request.temp_store)?.unwrap_or(defaults.temp_store),
        wal_autocheckpoint_pages: if request.wal_autocheckpoint_pages == 0 {
            defaults.wal_autocheckpoint_pages
        } else {
            request.wal_autocheckpoint_pages
        },
        cache_size_kib: if request.cache_size_kib == 0 {
            defaults.cache_size_kib
        } else {
            request.cache_size_kib
        },
        mmap_size_bytes: default_u64(request.mmap_size_bytes, defaults.mmap_size_bytes),
        enforce_db_file_lock: request.enforce_db_file_lock != 0,
        read_only: request.read_only != 0,
        allow_uri_filenames: request.allow_uri_filenames != 0,
        trusted_schema: request.trusted_schema != 0,
        defensive: request.defensive != 0,
    })
}

/// Convert one native LanceDB enable request into the Rust SDK type.
/// 将一份原生 LanceDB 启用请求转换成 Rust SDK 类型。
fn ffi_lancedb_enable_request_to_rust(
    request: *const FfiControllerLanceDbEnableRequest,
) -> Result<ControllerLanceDbEnableRequest, String> {
    if request.is_null() {
        return Err("lancedb enable request pointer must not be null".to_string());
    }
    let request = unsafe { &*request };
    let defaults = ControllerLanceDbEnableRequest::default();
    let space_id = required_c_string(request.space_id, "space_id")?;
    Ok(ControllerLanceDbEnableRequest {
        space_id: space_id.clone(),
        binding_id: space_id,
        default_db_path: required_c_string(request.default_db_path, "default_db_path")?,
        db_root: optional_c_string(request.db_root)?,
        read_consistency_interval_ms: optional_nonzero_u64(request.read_consistency_interval_ms),
        max_upsert_payload: default_u64(
            request.max_upsert_payload,
            defaults.max_upsert_payload as u64,
        ) as usize,
        max_search_limit: default_u64(request.max_search_limit, defaults.max_search_limit as u64)
            as usize,
        max_concurrent_requests: default_u64(
            request.max_concurrent_requests,
            defaults.max_concurrent_requests as u64,
        ) as usize,
    })
}

/// Convert one Rust status snapshot into the native FFI shape.
/// 将一份 Rust 状态快照转换成原生 FFI 结构。
fn map_status_snapshot(snapshot: ControllerStatusSnapshot) -> FfiControllerStatusSnapshot {
    FfiControllerStatusSnapshot {
        process_mode: map_process_mode_to_native(snapshot.process_mode),
        bind_addr: string_into_raw(snapshot.bind_addr),
        started_at_unix_ms: snapshot.started_at_unix_ms,
        last_request_at_unix_ms: snapshot.last_request_at_unix_ms,
        minimum_uptime_secs: snapshot.minimum_uptime_secs,
        idle_timeout_secs: snapshot.idle_timeout_secs,
        default_lease_ttl_secs: snapshot.default_lease_ttl_secs,
        active_clients: snapshot.active_clients as c_ulonglong,
        attached_spaces: snapshot.attached_spaces as c_ulonglong,
        inflight_requests: snapshot.inflight_requests as c_ulonglong,
        shutdown_candidate: if snapshot.shutdown_candidate { 1 } else { 0 },
    }
}

/// Convert one Rust space snapshot into the native FFI shape.
/// 将一份 Rust 空间快照转换成原生 FFI 结构。
fn map_space_snapshot(snapshot: SpaceSnapshot) -> FfiSpaceSnapshot {
    FfiSpaceSnapshot {
        space_id: string_into_raw(snapshot.space_id),
        space_label: string_into_raw(snapshot.space_label),
        space_kind: map_space_kind_to_native(snapshot.space_kind),
        space_root: string_into_raw(snapshot.space_root),
        attached_clients: snapshot.attached_clients as c_ulonglong,
        sqlite: map_backend_status_option(snapshot.sqlite),
        lancedb: map_backend_status_option(snapshot.lancedb),
    }
}

/// Convert one Rust space snapshot vector into the native FFI array wrapper.
/// 将一组 Rust 空间快照转换成原生 FFI 数组包装结构。
fn map_space_snapshot_array(spaces: Vec<SpaceSnapshot>) -> FfiSpaceSnapshotArray {
    let mut mapped: Vec<FfiSpaceSnapshot> = spaces.into_iter().map(map_space_snapshot).collect();
    let items = mapped.as_mut_ptr();
    let len = mapped.len();
    std::mem::forget(mapped);
    FfiSpaceSnapshotArray { items, len }
}

/// Convert one optional backend status into one optional native heap allocation.
/// 将一条可选后端状态转换成可选原生堆分配。
fn map_backend_status_option(status: Option<SpaceBackendStatus>) -> *mut FfiSpaceBackendStatus {
    status
        .map(|status| {
            Box::into_raw(Box::new(FfiSpaceBackendStatus {
                enabled: if status.enabled { 1 } else { 0 },
                mode: string_into_raw(status.mode),
                target: string_into_raw(status.target),
            }))
        })
        .unwrap_or(ptr::null_mut())
}

/// Convert one Rust SQLite execute result into the native FFI shape.
/// 将一份 Rust SQLite 执行结果转换成原生 FFI 结构。
fn map_sqlite_execute_result(
    result: ControllerSqliteExecuteResult,
) -> FfiControllerSqliteExecuteResult {
    FfiControllerSqliteExecuteResult {
        success: if result.success { 1 } else { 0 },
        message: string_into_raw(result.message),
        rows_changed: result.rows_changed,
        last_insert_rowid: result.last_insert_rowid,
    }
}

/// Convert one Rust SQLite query result into the native FFI shape.
/// 将一份 Rust SQLite 查询结果转换成原生 FFI 结构。
fn map_sqlite_query_result(result: ControllerSqliteQueryResult) -> FfiControllerSqliteQueryResult {
    FfiControllerSqliteQueryResult {
        json_data: string_into_raw(result.json_data),
        row_count: result.row_count,
    }
}

/// Convert one Rust SQLite batch execution result into the native FFI shape.
/// 将一份 Rust SQLite 批量执行结果转换成原生 FFI 结构。
fn map_sqlite_execute_batch_result(
    result: ControllerSqliteExecuteBatchResult,
) -> FfiControllerSqliteExecuteBatchResult {
    FfiControllerSqliteExecuteBatchResult {
        success: if result.success { 1 } else { 0 },
        message: string_into_raw(result.message),
        rows_changed: result.rows_changed,
        last_insert_rowid: result.last_insert_rowid,
        statements_executed: result.statements_executed,
    }
}

/// Convert one Rust SQLite streaming query result into the native FFI shape.
/// 将一份 Rust SQLite 流式查询结果转换成原生 FFI 结构。
fn map_sqlite_query_stream_result(
    result: ControllerSqliteQueryStreamCompatResult,
) -> FfiControllerSqliteQueryStreamResult {
    let chunks = map_byte_buffer_array(result.chunks);
    FfiControllerSqliteQueryStreamResult {
        chunks: Box::into_raw(Box::new(chunks)),
        row_count: result.row_count,
        chunk_count: result.chunk_count,
        total_bytes: result.total_bytes,
    }
}

/// Collect one controller-managed SQLite stream into the current FFI compatibility result shape.
/// 将一条由控制器管理的 SQLite 流收集成当前 FFI 兼容结果形态。
fn collect_sqlite_query_stream_result(
    runtime: &Runtime,
    client: &ControllerClient,
    space_id: String,
    binding_id: String,
    sql: String,
    params: Vec<ControllerSqliteValue>,
    chunk_size: Option<u64>,
) -> Result<ControllerSqliteQueryStreamCompatResult, String> {
    let stream = runtime
        .block_on(
            client.open_sqlite_query_stream_typed(space_id, binding_id, sql, params, chunk_size),
        )
        .map_err(error_to_string)?;
    let stream_id = stream.stream_id;

    let collected = (|| -> Result<ControllerSqliteQueryStreamCompatResult, String> {
        let metrics = runtime
            .block_on(client.wait_sqlite_query_stream_metrics(stream_id))
            .map_err(error_to_string)?;
        let mut chunks = Vec::with_capacity(metrics.chunk_count as usize);
        for index in 0..metrics.chunk_count {
            let chunk = runtime
                .block_on(client.read_sqlite_query_stream_chunk(stream_id, index))
                .map_err(error_to_string)?;
            chunks.push(chunk);
        }
        Ok(ControllerSqliteQueryStreamCompatResult {
            chunks,
            row_count: metrics.row_count,
            chunk_count: metrics.chunk_count,
            total_bytes: metrics.total_bytes,
        })
    })();

    let _ = runtime.block_on(client.close_sqlite_query_stream(stream_id));
    collected
}

/// Convert one Rust SQLite tokenize result into the native FFI shape.
/// 将一份 Rust SQLite 分词结果转换成原生 FFI 结构。
fn map_sqlite_tokenize_result(
    result: ControllerSqliteTokenizeResult,
) -> FfiControllerSqliteTokenizeResult {
    FfiControllerSqliteTokenizeResult {
        tokenizer_mode: string_into_raw(result.tokenizer_mode),
        normalized_text: string_into_raw(result.normalized_text),
        tokens_json: string_into_raw(
            serde_json::to_string(&result.tokens).expect("token list serialization must not fail"),
        ),
        fts_query: string_into_raw(result.fts_query),
    }
}

/// Convert one Rust SQLite list-custom-words result into the native FFI shape.
/// 将一份 Rust SQLite 自定义词列表结果转换成原生 FFI 结构。
fn map_sqlite_list_custom_words_result(
    result: ControllerSqliteListCustomWordsResult,
) -> FfiControllerSqliteListCustomWordsResult {
    FfiControllerSqliteListCustomWordsResult {
        success: if result.success { 1 } else { 0 },
        message: string_into_raw(result.message),
        words: Box::into_raw(Box::new(map_custom_word_array(result.words))),
    }
}

/// Convert one Rust SQLite dictionary mutation result into the native FFI shape.
/// 将一份 Rust SQLite 词典变更结果转换成原生 FFI 结构。
fn map_sqlite_dictionary_mutation_result(
    result: ControllerSqliteDictionaryMutationResult,
) -> FfiControllerSqliteDictionaryMutationResult {
    FfiControllerSqliteDictionaryMutationResult {
        success: if result.success { 1 } else { 0 },
        message: string_into_raw(result.message),
        affected_rows: result.affected_rows,
    }
}

/// Convert one Rust SQLite ensure-FTS result into the native FFI shape.
/// 将一份 Rust SQLite FTS 确认结果转换成原生 FFI 结构。
fn map_sqlite_ensure_fts_index_result(
    result: ControllerSqliteEnsureFtsIndexResult,
) -> FfiControllerSqliteEnsureFtsIndexResult {
    FfiControllerSqliteEnsureFtsIndexResult {
        success: if result.success { 1 } else { 0 },
        message: string_into_raw(result.message),
        index_name: string_into_raw(result.index_name),
        tokenizer_mode: string_into_raw(result.tokenizer_mode),
    }
}

/// Convert one Rust SQLite rebuild-FTS result into the native FFI shape.
/// 将一份 Rust SQLite FTS 重建结果转换成原生 FFI 结构。
fn map_sqlite_rebuild_fts_index_result(
    result: ControllerSqliteRebuildFtsIndexResult,
) -> FfiControllerSqliteRebuildFtsIndexResult {
    FfiControllerSqliteRebuildFtsIndexResult {
        success: if result.success { 1 } else { 0 },
        message: string_into_raw(result.message),
        index_name: string_into_raw(result.index_name),
        tokenizer_mode: string_into_raw(result.tokenizer_mode),
        reindexed_rows: result.reindexed_rows,
    }
}

/// Convert one Rust SQLite FTS mutation result into the native FFI shape.
/// 将一份 Rust SQLite FTS 文档变更结果转换成原生 FFI 结构。
fn map_sqlite_fts_mutation_result(
    result: ControllerSqliteFtsMutationResult,
) -> FfiControllerSqliteFtsMutationResult {
    FfiControllerSqliteFtsMutationResult {
        success: if result.success { 1 } else { 0 },
        message: string_into_raw(result.message),
        affected_rows: result.affected_rows,
        index_name: string_into_raw(result.index_name),
    }
}

/// Convert one Rust SQLite FTS search result into the native FFI shape.
/// 将一份 Rust SQLite FTS 检索结果转换成原生 FFI 结构。
fn map_sqlite_search_fts_result(
    result: ControllerSqliteSearchFtsResult,
) -> FfiControllerSqliteSearchFtsResult {
    FfiControllerSqliteSearchFtsResult {
        success: if result.success { 1 } else { 0 },
        message: string_into_raw(result.message),
        index_name: string_into_raw(result.index_name),
        tokenizer_mode: string_into_raw(result.tokenizer_mode),
        normalized_query: string_into_raw(result.normalized_query),
        fts_query: string_into_raw(result.fts_query),
        source: string_into_raw(result.source),
        query_mode: string_into_raw(result.query_mode),
        total: result.total,
        hits: Box::into_raw(Box::new(map_search_fts_hit_array(result.hits))),
    }
}

/// Convert one Rust LanceDB create-table result into the native FFI shape.
/// 将一份 Rust LanceDB 建表结果转换成原生 FFI 结构。
fn map_lancedb_create_table_result(
    result: ControllerLanceDbCreateTableResult,
) -> FfiControllerLanceDbCreateTableResult {
    FfiControllerLanceDbCreateTableResult {
        message: string_into_raw(result.message),
    }
}

/// Convert one Rust LanceDB upsert result into the native FFI shape.
/// 将一份 Rust LanceDB 写入结果转换成原生 FFI 结构。
fn map_lancedb_upsert_result(
    result: ControllerLanceDbUpsertResult,
) -> FfiControllerLanceDbUpsertResult {
    FfiControllerLanceDbUpsertResult {
        message: string_into_raw(result.message),
        version: result.version,
        input_rows: result.input_rows,
        inserted_rows: result.inserted_rows,
        updated_rows: result.updated_rows,
        deleted_rows: result.deleted_rows,
    }
}

/// Convert one Rust LanceDB search result into the native FFI shape.
/// 将一份 Rust LanceDB 检索结果转换成原生 FFI 结构。
fn map_lancedb_search_result(
    result: ControllerLanceDbSearchResult,
) -> FfiControllerLanceDbSearchResult {
    let (data, data_len) = bytes_into_raw(result.data);
    FfiControllerLanceDbSearchResult {
        message: string_into_raw(result.message),
        format: string_into_raw(result.format),
        rows: result.rows,
        data,
        data_len,
    }
}

/// Convert one Rust LanceDB delete result into the native FFI shape.
/// 将一份 Rust LanceDB 删除结果转换成原生 FFI 结构。
fn map_lancedb_delete_result(
    result: ControllerLanceDbDeleteResult,
) -> FfiControllerLanceDbDeleteResult {
    FfiControllerLanceDbDeleteResult {
        message: string_into_raw(result.message),
        version: result.version,
        deleted_rows: result.deleted_rows,
    }
}

/// Convert one Rust LanceDB drop-table result into the native FFI shape.
/// 将一份 Rust LanceDB 删表结果转换成原生 FFI 结构。
fn map_lancedb_drop_table_result(
    result: ControllerLanceDbDropTableResult,
) -> FfiControllerLanceDbDropTableResult {
    FfiControllerLanceDbDropTableResult {
        message: string_into_raw(result.message),
    }
}

/// Convert one byte-chunk vector into the native FFI array shape.
/// 将一组字节分块转换成原生 FFI 数组结构。
fn map_byte_buffer_array(chunks: Vec<Vec<u8>>) -> FfiByteBufferArray {
    let mut items: Vec<FfiByteBuffer> = chunks
        .into_iter()
        .map(|chunk| {
            let (data, len) = bytes_into_raw(chunk);
            FfiByteBuffer { data, len }
        })
        .collect();
    let ptr = items.as_mut_ptr();
    let len = items.len();
    std::mem::forget(items);
    FfiByteBufferArray { items: ptr, len }
}

/// Convert one custom-word vector into the native FFI array shape.
/// 将一组自定义词向量转换成原生 FFI 数组结构。
fn map_custom_word_array(
    words: Vec<ControllerSqliteCustomWordEntry>,
) -> FfiControllerSqliteCustomWordArray {
    let mut items: Vec<FfiControllerSqliteCustomWordEntry> = words
        .into_iter()
        .map(|entry| FfiControllerSqliteCustomWordEntry {
            word: string_into_raw(entry.word),
            weight: entry.weight as c_ulonglong,
        })
        .collect();
    let ptr = items.as_mut_ptr();
    let len = items.len();
    std::mem::forget(items);
    FfiControllerSqliteCustomWordArray { items: ptr, len }
}

/// Convert one FTS hit vector into the native FFI array shape.
/// 将一组 FTS 命中向量转换成原生 FFI 数组结构。
fn map_search_fts_hit_array(
    hits: Vec<ControllerSqliteSearchFtsHit>,
) -> FfiControllerSqliteSearchFtsHitArray {
    let mut items: Vec<FfiControllerSqliteSearchFtsHit> = hits
        .into_iter()
        .map(|hit| FfiControllerSqliteSearchFtsHit {
            id: string_into_raw(hit.id),
            file_path: string_into_raw(hit.file_path),
            title: string_into_raw(hit.title),
            title_highlight: string_into_raw(hit.title_highlight),
            content_snippet: string_into_raw(hit.content_snippet),
            score: hit.score,
            rank: hit.rank,
            raw_score: hit.raw_score,
        })
        .collect();
    let ptr = items.as_mut_ptr();
    let len = items.len();
    std::mem::forget(items);
    FfiControllerSqliteSearchFtsHitArray { items: ptr, len }
}

/// Read one required UTF-8 string from an FFI pointer.
/// 从 FFI 指针读取一条必填 UTF-8 字符串。
fn required_c_string(value: *const c_char, field_name: &str) -> Result<String, String> {
    if value.is_null() {
        return Err(format!("{field_name} pointer must not be null"));
    }
    let value = unsafe { CStr::from_ptr(value) };
    let text = value
        .to_str()
        .map_err(|error| format!("{field_name} must be valid UTF-8: {error}"))?
        .trim()
        .to_string();
    if text.is_empty() {
        return Err(format!("{field_name} must not be empty"));
    }
    Ok(text)
}

/// Read one required UTF-8 string from an FFI pointer without trimming or empty rejection.
/// 从 FFI 指针读取一条必填 UTF-8 字符串，但不裁剪空白且允许空串。
fn required_c_string_preserve(value: *const c_char, field_name: &str) -> Result<String, String> {
    if value.is_null() {
        return Err(format!("{field_name} pointer must not be null"));
    }
    let value = unsafe { CStr::from_ptr(value) };
    value
        .to_str()
        .map(|text| text.to_string())
        .map_err(|error| format!("{field_name} must be valid UTF-8: {error}"))
}

/// Read one required UTF-8 string array from FFI pointers.
/// 从 FFI 指针读取一组必填 UTF-8 字符串。
fn required_string_array(
    items: *const *const c_char,
    len: usize,
    field_name: &str,
) -> Result<Vec<String>, String> {
    if items.is_null() {
        return Err(format!("{field_name} pointer must not be null"));
    }
    let slice = unsafe { std::slice::from_raw_parts(items, len) };
    slice
        .iter()
        .enumerate()
        .map(|(index, item)| required_c_string(*item, &format!("{field_name}[{index}]")))
        .collect()
}

/// Read one optional UTF-8 string from an FFI pointer.
/// 从 FFI 指针读取一条可选 UTF-8 字符串。
fn optional_c_string(value: *const c_char) -> Result<Option<String>, String> {
    if value.is_null() {
        return Ok(None);
    }
    let value = unsafe { CStr::from_ptr(value) };
    let text = value
        .to_str()
        .map_err(|error| format!("optional string must be valid UTF-8: {error}"))?
        .trim()
        .to_string();
    if text.is_empty() {
        Ok(None)
    } else {
        Ok(Some(text))
    }
}

/// Parse one native tokenizer-mode string into the shared enum.
/// 将一条原生分词模式字符串解析成共享枚举。
fn parse_sqlite_tokenizer_mode_text(
    value: *const c_char,
) -> Result<ControllerSqliteTokenizerMode, String> {
    let value = required_c_string(value, "tokenizer_mode")?;
    parse_sqlite_tokenizer_mode_name(&value)
}

/// Parse one tokenizer-mode name into the shared enum.
/// 将一条分词模式名称解析成共享枚举。
fn parse_sqlite_tokenizer_mode_name(value: &str) -> Result<ControllerSqliteTokenizerMode, String> {
    match value.trim().to_ascii_lowercase().as_str() {
        "" | "none" => Ok(ControllerSqliteTokenizerMode::None),
        "jieba" => Ok(ControllerSqliteTokenizerMode::Jieba),
        other => Err(format!("unsupported tokenizer_mode: {other}")),
    }
}

/// Convert one JSON scalar or bytes wrapper into the shared SQLite typed value.
/// 将一条 JSON 标量或 bytes 包装对象转换成共享 SQLite 类型化值。
fn json_to_sqlite_value(value: JsonValue) -> Result<ControllerSqliteValue, String> {
    match value {
        JsonValue::Null => Ok(ControllerSqliteValue::Null),
        JsonValue::Bool(value) => Ok(ControllerSqliteValue::Bool(value)),
        JsonValue::Number(value) => {
            if let Some(value) = value.as_i64() {
                Ok(ControllerSqliteValue::Int64(value))
            } else if let Some(value) = value.as_u64() {
                Ok(ControllerSqliteValue::Int64(i64::try_from(value).map_err(
                    |_| "params contains an unsigned integer larger than i64".to_string(),
                )?))
            } else if let Some(value) = value.as_f64() {
                Ok(ControllerSqliteValue::Float64(value))
            } else {
                Err("params contains an unsupported numeric value".to_string())
            }
        }
        JsonValue::String(value) => Ok(ControllerSqliteValue::String(value)),
        JsonValue::Object(value) => {
            let wrapper = serde_json::from_value::<JsonSqliteBytesValue>(JsonValue::Object(value))
                .map_err(|_| {
                    "params object values must use {\"type\":\"bytes_base64\",\"base64\":\"...\"} or {\"__type\":\"bytes_base64\",\"base64\":\"...\"}".to_string()
                })?;
            let wrapper_type = if !wrapper.r#type.trim().is_empty() {
                wrapper.r#type.trim()
            } else {
                wrapper.__type.trim()
            };
            if wrapper_type != "bytes_base64" {
                return Err(
                    "params object values only support the bytes_base64 wrapper type".to_string(),
                );
            }
            Ok(ControllerSqliteValue::Bytes(decode_base64(
                &wrapper.base64,
            )?))
        }
        JsonValue::Array(_) => {
            Err("params only supports scalar JSON values or bytes wrapper objects".to_string())
        }
    }
}

/// Read one required byte slice from raw pointer and length.
/// 从原始指针与长度读取一段必填字节切片。
fn required_bytes<'a>(data: *const u8, len: usize, field_name: &str) -> Result<&'a [u8], String> {
    if data.is_null() {
        return Err(format!("{field_name} pointer must not be null"));
    }
    if len == 0 {
        return Err(format!("{field_name} length must be greater than zero"));
    }
    Ok(unsafe { std::slice::from_raw_parts(data, len) })
}

/// Read one byte slice from raw pointer and length while allowing an empty blob.
/// 从原始指针与长度读取一段字节切片，并允许空 blob。
fn optional_bytes<'a>(data: *const u8, len: usize, field_name: &str) -> Result<&'a [u8], String> {
    if len == 0 {
        return Ok(&[]);
    }
    if data.is_null() {
        return Err(format!(
            "{field_name} pointer must not be null when len > 0"
        ));
    }
    Ok(unsafe { std::slice::from_raw_parts(data, len) })
}

/// Read one optional UTF-8 string array from FFI pointers.
/// 从 FFI 指针读取一组可选 UTF-8 字符串。
fn optional_string_array(
    items: *const *const c_char,
    len: usize,
    field_name: &str,
) -> Result<Vec<String>, String> {
    if len == 0 {
        return Ok(Vec::new());
    }
    required_string_array(items, len, field_name)
}

/// Read one required float32 slice from raw pointer and length.
/// 从原始指针与长度读取一段必填 float32 切片。
fn required_f32_slice<'a>(
    data: *const c_float,
    len: usize,
    field_name: &str,
) -> Result<&'a [c_float], String> {
    if data.is_null() {
        return Err(format!("{field_name} pointer must not be null"));
    }
    if len == 0 {
        return Err(format!("{field_name} length must be greater than zero"));
    }
    Ok(unsafe { std::slice::from_raw_parts(data, len) })
}

/// Read one native SQLite value array into shared typed values.
/// 将一组原生 SQLite 值数组读取成共享类型化值。
fn read_sqlite_values(
    values: *const FfiSqliteValue,
    len: usize,
    field_name: &str,
) -> Result<Vec<ControllerSqliteValue>, String> {
    if len == 0 {
        return Ok(Vec::new());
    }
    if values.is_null() {
        return Err(format!("{field_name} pointer must not be null"));
    }
    let slice = unsafe { std::slice::from_raw_parts(values, len) };
    slice
        .iter()
        .enumerate()
        .map(|(index, value)| map_sqlite_value_native(value, &format!("{field_name}[{index}]")))
        .collect()
}

/// Read one native SQLite batch item array into shared typed values.
/// 将一组原生 SQLite 批量参数项读取成共享类型化值。
fn read_sqlite_batch_items(
    items: *const FfiSqliteBatchItem,
    len: usize,
    field_name: &str,
) -> Result<Vec<Vec<ControllerSqliteValue>>, String> {
    if len == 0 {
        return Ok(Vec::new());
    }
    if items.is_null() {
        return Err(format!("{field_name} pointer must not be null"));
    }
    let slice = unsafe { std::slice::from_raw_parts(items, len) };
    slice
        .iter()
        .enumerate()
        .map(|(index, item)| {
            read_sqlite_values(
                item.params,
                item.params_len,
                &format!("{field_name}[{index}].params"),
            )
        })
        .collect()
}

/// Read one native LanceDB column-definition array into shared typed values.
/// 将一组原生 LanceDB 列定义数组读取成共享类型化值。
fn read_lancedb_columns(
    columns: *const FfiControllerLanceDbColumnDef,
    len: usize,
    field_name: &str,
) -> Result<Vec<ControllerLanceDbColumnDef>, String> {
    if columns.is_null() {
        return Err(format!("{field_name} pointer must not be null"));
    }
    if len == 0 {
        return Err(format!("{field_name} length must be greater than zero"));
    }
    let slice = unsafe { std::slice::from_raw_parts(columns, len) };
    slice
        .iter()
        .enumerate()
        .map(|(index, column)| {
            Ok(ControllerLanceDbColumnDef {
                name: required_c_string(column.name, &format!("{field_name}[{index}].name"))?,
                column_type: map_lancedb_column_type_native(column.column_type)?,
                vector_dim: column.vector_dim,
                nullable: column.nullable != 0,
            })
        })
        .collect()
}

/// Convert one native SQLite value into the shared typed value.
/// 将一条原生 SQLite 值转换成共享类型化值。
fn map_sqlite_value_native(
    value: &FfiSqliteValue,
    field_name: &str,
) -> Result<ControllerSqliteValue, String> {
    match value.kind {
        0 => Ok(ControllerSqliteValue::Int64(value.int64_value)),
        1 => Ok(ControllerSqliteValue::Float64(value.float64_value)),
        2 => Ok(ControllerSqliteValue::String(required_c_string_preserve(
            value.string_value,
            &format!("{field_name}.string_value"),
        )?)),
        3 => Ok(ControllerSqliteValue::Bytes(
            optional_bytes(
                value.bytes_value,
                value.bytes_len,
                &format!("{field_name}.bytes_value"),
            )?
            .to_vec(),
        )),
        4 => Ok(ControllerSqliteValue::Bool(value.bool_value != 0)),
        5 => Ok(ControllerSqliteValue::Null),
        other => Err(format!("unsupported sqlite value kind `{other}`")),
    }
}

/// Return the default nullable flag for JSON LanceDB column definitions.
/// 返回 JSON LanceDB 列定义使用的默认可空标记。
fn default_nullable() -> bool {
    true
}

/// Return the default search limit for JSON LanceDB search requests.
/// 返回 JSON LanceDB 检索请求使用的默认 limit。
fn default_search_limit() -> u32 {
    10
}

/// Parse one JSON input pointer into the requested Rust type.
/// 将一个 JSON 输入指针解析成请求的 Rust 类型。
fn parse_json_input<T>(json: *const c_char) -> Result<T, String>
where
    T: for<'de> Deserialize<'de>,
{
    let json = required_c_string(json, "json")?;
    serde_json::from_str(&json).map_err(|error| format!("failed to parse json input: {error}"))
}

/// Encode one Rust string into a heap-owned C string pointer.
/// 将一条 Rust 字符串编码成堆拥有的 C 字符串指针。
fn string_into_raw(value: String) -> *mut c_char {
    let sanitized = if value.contains('\0') {
        value.replace('\0', "\u{FFFD}")
    } else {
        value
    };
    CString::new(sanitized)
        .expect("sanitized string must not contain interior null bytes")
        .into_raw()
}

/// Encode one byte vector into one heap-owned raw pointer and length.
/// 将一段字节向量编码成堆拥有的原始指针与长度。
fn bytes_into_raw(mut value: Vec<u8>) -> (*mut u8, usize) {
    let ptr = value.as_mut_ptr();
    let len = value.len();
    std::mem::forget(value);
    (ptr, len)
}

/// Run one closure with a mutable controller handle reference.
/// 使用一个可变控制器句柄引用执行闭包。
fn with_client_handle<T>(
    client: *mut FfiControllerClientHandle,
    func: impl FnOnce(&mut FfiControllerClientHandle) -> Result<T, String>,
) -> Result<T, String> {
    if client.is_null() {
        return Err("client handle pointer must not be null".to_string());
    }
    let client = unsafe { &mut *client };
    func(client)
}

/// Convert one process mode integer into the Rust enum.
/// 将一个进程模式整数转换成 Rust 枚举。
fn map_process_mode_native(raw: c_int) -> Result<ControllerProcessMode, String> {
    match raw {
        0 => Ok(ControllerProcessMode::Service),
        1 => Ok(ControllerProcessMode::Managed),
        _ => Err(format!("unsupported process mode value `{raw}`")),
    }
}

/// Convert one Rust process mode into the stable native integer.
/// 将一个 Rust 进程模式转换成稳定原生整数。
fn map_process_mode_to_native(mode: ControllerProcessMode) -> c_int {
    match mode {
        ControllerProcessMode::Service => 0,
        ControllerProcessMode::Managed => 1,
    }
}

/// Convert one space kind integer into the Rust enum.
/// 将一个空间类型整数转换成 Rust 枚举。
fn map_space_kind_native(raw: c_int) -> Result<SpaceKind, String> {
    match raw {
        0 => Ok(SpaceKind::Root),
        1 => Ok(SpaceKind::User),
        2 => Ok(SpaceKind::Project),
        _ => Err(format!("unsupported space kind value `{raw}`")),
    }
}

/// Convert one Rust space kind into the stable native integer.
/// 将一个 Rust 空间类型转换成稳定原生整数。
fn map_space_kind_to_native(kind: SpaceKind) -> c_int {
    match kind {
        SpaceKind::Root => 0,
        SpaceKind::User => 1,
        SpaceKind::Project => 2,
    }
}

/// Convert one native LanceDB column-type integer into the shared enum.
/// 将一个原生 LanceDB 列类型整数转换成共享枚举。
fn map_lancedb_column_type_native(raw: c_int) -> Result<ControllerLanceDbColumnType, String> {
    match raw {
        0 => Ok(ControllerLanceDbColumnType::Unspecified),
        1 => Ok(ControllerLanceDbColumnType::String),
        2 => Ok(ControllerLanceDbColumnType::Int64),
        3 => Ok(ControllerLanceDbColumnType::Float64),
        4 => Ok(ControllerLanceDbColumnType::Bool),
        5 => Ok(ControllerLanceDbColumnType::VectorFloat32),
        6 => Ok(ControllerLanceDbColumnType::Float32),
        7 => Ok(ControllerLanceDbColumnType::Uint64),
        8 => Ok(ControllerLanceDbColumnType::Int32),
        9 => Ok(ControllerLanceDbColumnType::Uint32),
        other => Err(format!("unsupported lancedb column type `{other}`")),
    }
}

/// Convert one native LanceDB input-format integer into the shared enum.
/// 将一个原生 LanceDB 输入格式整数转换成共享枚举。
fn map_lancedb_input_format_native(raw: c_int) -> Result<ControllerLanceDbInputFormat, String> {
    match raw {
        0 => Ok(ControllerLanceDbInputFormat::Unspecified),
        1 => Ok(ControllerLanceDbInputFormat::JsonRows),
        2 => Ok(ControllerLanceDbInputFormat::ArrowIpc),
        other => Err(format!("unsupported lancedb input format `{other}`")),
    }
}

/// Convert one native LanceDB output-format integer into the shared enum.
/// 将一个原生 LanceDB 输出格式整数转换成共享枚举。
fn map_lancedb_output_format_native(raw: c_int) -> Result<ControllerLanceDbOutputFormat, String> {
    match raw {
        0 => Ok(ControllerLanceDbOutputFormat::Unspecified),
        1 => Ok(ControllerLanceDbOutputFormat::ArrowIpc),
        2 => Ok(ControllerLanceDbOutputFormat::JsonRows),
        other => Err(format!("unsupported lancedb output format `{other}`")),
    }
}

/// Return the fallback when the provided value is zero.
/// 当提供值为零时返回回退值。
fn default_u64(value: c_ulonglong, fallback: u64) -> u64 {
    if value == 0 { fallback } else { value }
}

/// Convert zero into `None`.
/// 将零值转换成 `None`。
fn optional_nonzero_u64(value: c_ulonglong) -> Option<u64> {
    if value == 0 { None } else { Some(value) }
}

/// Write a native pointer output slot.
/// 写入一个原生指针输出槽位。
fn write_out_ptr<T>(slot: *mut *mut T, value: *mut T) {
    if !slot.is_null() {
        unsafe {
            *slot = value;
        }
    }
}

/// Clear a native pointer output slot.
/// 清空一个原生指针输出槽位。
fn clear_out_ptr<T>(slot: *mut *mut T) {
    if !slot.is_null() {
        unsafe {
            *slot = ptr::null_mut();
        }
    }
}

/// Write one byte flag output slot.
/// 写入一个字节标志输出槽位。
fn write_out_u8(slot: *mut c_uchar, value: c_uchar) {
    if !slot.is_null() {
        unsafe {
            *slot = value;
        }
    }
}

/// Clear one byte flag output slot.
/// 清空一个字节标志输出槽位。
fn clear_out_u8(slot: *mut c_uchar) {
    if !slot.is_null() {
        unsafe {
            *slot = 0;
        }
    }
}

/// Clear the error output slot.
/// 清空错误输出槽位。
fn clear_error_out(error_out: *mut *mut c_char) {
    clear_out_ptr(error_out);
}

/// Return one success status and clear any stale error output.
/// 返回成功状态并清空任何陈旧错误输出。
fn ffi_ok_status(error_out: *mut *mut c_char) -> c_int {
    clear_error_out(error_out);
    FFI_STATUS_OK
}

/// Return one error status and write the error string when possible.
/// 返回错误状态并在可能时写入错误字符串。
fn ffi_error_status(error_out: *mut *mut c_char, message: impl Into<String>) -> c_int {
    clear_error_out(error_out);
    if !error_out.is_null() {
        unsafe {
            *error_out = string_into_raw(message.into());
        }
    }
    FFI_STATUS_ERR
}

/// Convert one displayable error into a stable string.
/// 将一个可显示错误转换成稳定字符串。
fn error_to_string(error: impl std::fmt::Display) -> String {
    error.to_string()
}

/// Free one space snapshot and all nested heap allocations.
/// 释放一份空间快照及其全部嵌套堆分配。
fn free_space_snapshot_fields(snapshot: FfiSpaceSnapshot) {
    vldb_controller_ffi_string_free(snapshot.space_id);
    vldb_controller_ffi_string_free(snapshot.space_label);
    vldb_controller_ffi_string_free(snapshot.space_root);
    free_backend_status_ptr(snapshot.sqlite);
    free_backend_status_ptr(snapshot.lancedb);
}

/// Free one optional backend status allocation.
/// 释放一条可选后端状态分配。
fn free_backend_status_ptr(status: *mut FfiSpaceBackendStatus) {
    if !status.is_null() {
        unsafe {
            let status = Box::from_raw(status);
            vldb_controller_ffi_string_free(status.mode);
            vldb_controller_ffi_string_free(status.target);
        }
    }
}

/// Encode bytes as base64 without introducing extra dependencies.
/// 在不引入额外依赖的前提下将字节编码成 base64。
fn encode_base64(data: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut output = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let a = chunk[0];
        let b = *chunk.get(1).unwrap_or(&0);
        let c = *chunk.get(2).unwrap_or(&0);
        output.push(TABLE[(a >> 2) as usize] as char);
        output.push(TABLE[(((a & 0x03) << 4) | (b >> 4)) as usize] as char);
        if chunk.len() > 1 {
            output.push(TABLE[(((b & 0x0f) << 2) | (c >> 6)) as usize] as char);
        } else {
            output.push('=');
        }
        if chunk.len() > 2 {
            output.push(TABLE[(c & 0x3f) as usize] as char);
        } else {
            output.push('=');
        }
    }
    output
}

/// Decode base64 text without introducing extra dependencies.
/// 在不引入额外依赖的前提下解码 base64 文本。
fn decode_base64(input: &str) -> Result<Vec<u8>, String> {
    let bytes = input.as_bytes();
    if bytes.len() % 4 != 0 {
        return Err("base64 input length must be a multiple of 4".to_string());
    }
    let mut output = Vec::with_capacity(bytes.len() / 4 * 3);
    for chunk in bytes.chunks(4) {
        let a = decode_base64_char(chunk[0])?;
        let b = decode_base64_char(chunk[1])?;
        let c = if chunk[2] == b'=' {
            64
        } else {
            decode_base64_char(chunk[2])?
        };
        let d = if chunk[3] == b'=' {
            64
        } else {
            decode_base64_char(chunk[3])?
        };

        output.push((a << 2) | (b >> 4));
        if c != 64 {
            output.push(((b & 0x0f) << 4) | (c >> 2));
        }
        if d != 64 {
            output.push(((c & 0x03) << 6) | d);
        }
    }
    Ok(output)
}

/// Decode one base64 character into its 6-bit value.
/// 将一个 base64 字符解码成 6 位值。
fn decode_base64_char(value: u8) -> Result<u8, String> {
    match value {
        b'A'..=b'Z' => Ok(value - b'A'),
        b'a'..=b'z' => Ok(value - b'a' + 26),
        b'0'..=b'9' => Ok(value - b'0' + 52),
        b'+' => Ok(62),
        b'/' => Ok(63),
        _ => Err(format!("invalid base64 character `{}`", value as char)),
    }
}

#[cfg(test)]
mod tests {
    use super::{
        CreateLanceDbTableJsonRequest, DeleteLanceDbJsonRequest, ExecuteSqliteBatchJsonRequest,
        ExecuteSqliteScriptJsonRequest, FfiSqliteValue, QuerySqliteJsonJsonRequest,
        QuerySqliteStreamJsonRequest, SearchLanceDbJsonRequest, UpsertLanceDbJsonRequest,
        map_sqlite_value_native, required_c_string_preserve,
    };
    use serde_json::json;
    use std::ffi::CString;
    use vldb_controller_client::ControllerSqliteValue;

    #[test]
    fn sqlite_native_string_preserves_whitespace_and_allows_empty() {
        let spaced = CString::new("  a  ").expect("cstring should build");
        let empty = CString::new("").expect("cstring should build");
        let spaced_value = FfiSqliteValue {
            kind: 2,
            int64_value: 0,
            float64_value: 0.0,
            string_value: spaced.as_ptr(),
            bytes_value: std::ptr::null(),
            bytes_len: 0,
            bool_value: 0,
        };
        let empty_value = FfiSqliteValue {
            kind: 2,
            int64_value: 0,
            float64_value: 0.0,
            string_value: empty.as_ptr(),
            bytes_value: std::ptr::null(),
            bytes_len: 0,
            bool_value: 0,
        };

        assert_eq!(
            map_sqlite_value_native(&spaced_value, "value").expect("string should parse"),
            ControllerSqliteValue::String("  a  ".to_string())
        );
        assert_eq!(
            map_sqlite_value_native(&empty_value, "value").expect("empty string should parse"),
            ControllerSqliteValue::String(String::new())
        );
        assert_eq!(
            required_c_string_preserve(empty.as_ptr(), "value").expect("empty string should read"),
            String::new()
        );
    }

    #[test]
    fn sqlite_native_bytes_allow_empty_blob() {
        let value = FfiSqliteValue {
            kind: 3,
            int64_value: 0,
            float64_value: 0.0,
            string_value: std::ptr::null(),
            bytes_value: std::ptr::null(),
            bytes_len: 0,
            bool_value: 0,
        };

        assert_eq!(
            map_sqlite_value_native(&value, "value").expect("empty blob should parse"),
            ControllerSqliteValue::Bytes(Vec::new())
        );
    }

    #[test]
    fn preserve_string_reader_keeps_whitespace_for_text_payloads() {
        let spaced = CString::new("  keep  ").expect("cstring should build");
        assert_eq!(
            required_c_string_preserve(spaced.as_ptr(), "text").expect("text should read"),
            "  keep  ".to_string()
        );
    }

    #[test]
    fn preserve_string_reader_allows_empty_text_payloads() {
        let empty = CString::new("").expect("cstring should build");
        assert_eq!(
            required_c_string_preserve(empty.as_ptr(), "content")
                .expect("empty content should read"),
            String::new()
        );
    }

    #[test]
    fn sqlite_json_requests_accept_nested_params_without_inner_json_strings() {
        let execute: ExecuteSqliteScriptJsonRequest = serde_json::from_value(json!({
            "space_id": "ROOT",
            "sql": "SELECT ?",
            "params": [1, {"type": "bytes_base64", "base64": "AQID"}]
        }))
        .expect("execute request should parse");
        assert_eq!(execute.params.len(), 2);

        let query: QuerySqliteJsonJsonRequest = serde_json::from_value(json!({
            "space_id": "ROOT",
            "sql": "SELECT ?",
            "params": ["  keep  "]
        }))
        .expect("query request should parse");
        assert_eq!(query.params.len(), 1);

        let batch: ExecuteSqliteBatchJsonRequest = serde_json::from_value(json!({
            "space_id": "ROOT",
            "sql": "INSERT INTO demo VALUES (?)",
            "batch_params": [[1], [2], [{"__type": "bytes_base64", "base64": "AQ=="}]]
        }))
        .expect("batch request should parse");
        assert_eq!(batch.batch_params.len(), 3);

        let stream: QuerySqliteStreamJsonRequest = serde_json::from_value(json!({
            "space_id": "ROOT",
            "sql": "SELECT ?",
            "params": [],
            "target_chunk_size": 1024
        }))
        .expect("stream request should parse");
        assert_eq!(stream.target_chunk_size, Some(1024));
    }

    #[test]
    fn lancedb_json_requests_accept_nested_objects_without_request_json() {
        let create: CreateLanceDbTableJsonRequest = serde_json::from_value(json!({
            "space_id": "ROOT",
            "table_name": "memory",
            "columns": [
                {"name": "id", "column_type": "string", "nullable": false},
                {"name": "vector", "column_type": "vector", "vector_dim": 3, "nullable": false}
            ],
            "overwrite_if_exists": true
        }))
        .expect("create request should parse");
        assert_eq!(create.columns.len(), 2);

        let upsert: UpsertLanceDbJsonRequest = serde_json::from_value(json!({
            "space_id": "ROOT",
            "table_name": "memory",
            "input_format": "json_rows",
            "key_columns": ["id"],
            "data_base64": "W10="
        }))
        .expect("upsert request should parse");
        assert_eq!(upsert.key_columns, vec!["id".to_string()]);

        let search: SearchLanceDbJsonRequest = serde_json::from_value(json!({
            "space_id": "ROOT",
            "table_name": "memory",
            "vector": [0.1, 0.2, 0.3],
            "limit": 5,
            "filter": "kind = 'note'",
            "vector_column": "vector",
            "output_format": "json_rows"
        }))
        .expect("search request should parse");
        assert_eq!(search.vector.len(), 3);

        let delete: DeleteLanceDbJsonRequest = serde_json::from_value(json!({
            "space_id": "ROOT",
            "table_name": "memory",
            "condition": "id = 'a'"
        }))
        .expect("delete request should parse");
        assert_eq!(delete.condition, "id = 'a'");
    }
}

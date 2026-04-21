use std::error::Error;
use std::sync::Arc;

use serde::Deserialize;
use vldb_controller_client::types::{
    ControllerLanceDbCreateTableResult, ControllerLanceDbDeleteResult,
    ControllerLanceDbDropTableResult, ControllerLanceDbEnableRequest,
    ControllerLanceDbSearchResult, ControllerLanceDbUpsertResult,
};
use vldb_lancedb::engine::LanceDbEngineError;
use vldb_lancedb::engine::LanceDbEngineOptions;
use vldb_lancedb::manager::DatabaseRuntimeConfig;
use vldb_lancedb::runtime::LanceDbRuntime;
use vldb_lancedb::types::{
    LanceDbColumnDef, LanceDbColumnType, LanceDbCreateTableInput, LanceDbDeleteInput,
    LanceDbDropTableInput, LanceDbInputFormat, LanceDbOutputFormat, LanceDbSearchInput,
    LanceDbUpsertInput,
};

/// Shared error type used by the controller core.
/// 控制器核心复用的共享错误类型。
pub type BoxError = Box<dyn Error + Send + Sync + 'static>;

/// LanceDB backend wrapper managed by one controller space slot.
/// 由控制器单个空间槽位管理的 LanceDB 后端封装。
#[derive(Clone)]
pub struct ControllerLanceDbBackend {
    runtime: Arc<LanceDbRuntime>,
    runtime_config: DatabaseRuntimeConfig,
}

/// JSON create-table request accepted by the controller-side LanceDB data plane.
/// 控制器侧 LanceDB 数据面接受的 JSON 建表请求。
#[derive(Debug, Deserialize)]
struct CreateTableJsonInput {
    table_name: String,
    columns: Vec<CreateTableJsonColumn>,
    #[serde(default)]
    overwrite_if_exists: bool,
}

/// JSON column definition accepted by the controller-side LanceDB create-table path.
/// 控制器侧 LanceDB 建表路径接受的 JSON 列定义。
#[derive(Debug, Deserialize)]
struct CreateTableJsonColumn {
    name: String,
    column_type: String,
    #[serde(default)]
    vector_dim: u32,
    #[serde(default = "default_nullable")]
    nullable: bool,
}

/// JSON upsert request accepted by the controller-side LanceDB data plane.
/// 控制器侧 LanceDB 数据面接受的 JSON 写入请求。
#[derive(Debug, Deserialize)]
struct UpsertJsonInput {
    table_name: String,
    input_format: String,
    #[serde(default)]
    key_columns: Vec<String>,
}

/// JSON search request accepted by the controller-side LanceDB data plane.
/// 控制器侧 LanceDB 数据面接受的 JSON 检索请求。
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

/// JSON delete request accepted by the controller-side LanceDB data plane.
/// 控制器侧 LanceDB 数据面接受的 JSON 删除请求。
#[derive(Debug, Deserialize)]
struct DeleteJsonInput {
    table_name: String,
    condition: String,
}

impl ControllerLanceDbBackend {
    /// Create one controller-managed LanceDB backend.
    /// 创建一个由控制器管理的 LanceDB 后端。
    pub fn new(request: &ControllerLanceDbEnableRequest) -> Result<Self, BoxError> {
        let runtime_config = lancedb_runtime_config_from_request(request);
        let engine_options = lancedb_engine_options_from_request(request);
        let runtime = LanceDbRuntime::new(runtime_config.clone(), engine_options.clone());
        // Resolve the default path eagerly so host-side path mistakes fail during enable instead of first data use.
        // 提前解析默认路径，确保宿主路径错误在启用阶段暴露，而不是等到第一次数据访问时才失败。
        runtime.database_path_for_name(None)?;
        Ok(Self {
            runtime: Arc::new(runtime),
            runtime_config,
        })
    }

    /// Return the default database path owned by this backend.
    /// 返回该后端持有的默认数据库路径。
    pub fn default_db_path(&self) -> &str {
        &self.runtime_config.default_db_path
    }

    /// Create one LanceDB table using the shared runtime backend.
    /// 使用共享运行时后端创建一张 LanceDB 表。
    pub async fn create_table(
        &self,
        request_json: &str,
    ) -> Result<ControllerLanceDbCreateTableResult, BoxError> {
        let input =
            map_create_table_input(parse_json_input::<CreateTableJsonInput>(request_json)?)?;
        let engine = self.runtime.open_default_engine().await?;
        let result = engine
            .create_table(input)
            .await
            .map_err(engine_error_to_box)?;
        Ok(ControllerLanceDbCreateTableResult {
            message: result.message,
        })
    }

    /// Upsert LanceDB rows using JSON metadata plus a binary payload.
    /// 使用 JSON 元信息与二进制载荷写入 LanceDB 行数据。
    pub async fn upsert(
        &self,
        request_json: &str,
        data: Vec<u8>,
    ) -> Result<ControllerLanceDbUpsertResult, BoxError> {
        let input = map_upsert_input(parse_json_input::<UpsertJsonInput>(request_json)?, data)?;
        let engine = self.runtime.open_default_engine().await?;
        let result = engine
            .vector_upsert(input)
            .await
            .map_err(engine_error_to_box)?;
        Ok(ControllerLanceDbUpsertResult {
            message: result.message,
            version: result.version,
            input_rows: result.input_rows,
            inserted_rows: result.inserted_rows,
            updated_rows: result.updated_rows,
            deleted_rows: result.deleted_rows,
        })
    }

    /// Search LanceDB rows using the JSON request body defined by the official backend contract.
    /// 使用官方后端协议定义的 JSON 请求体检索 LanceDB 行数据。
    pub async fn search(
        &self,
        request_json: &str,
    ) -> Result<ControllerLanceDbSearchResult, BoxError> {
        let input = map_search_input(parse_json_input::<SearchJsonInput>(request_json)?)?;
        let engine = self.runtime.open_default_engine().await?;
        let result = engine
            .vector_search(input)
            .await
            .map_err(engine_error_to_box)?;
        Ok(ControllerLanceDbSearchResult {
            message: result.message,
            format: result.format.as_wire_name().to_string(),
            rows: result.rows,
            data: result.data,
        })
    }

    /// Delete LanceDB rows using the JSON request body defined by the official backend contract.
    /// 使用官方后端协议定义的 JSON 请求体删除 LanceDB 行数据。
    pub async fn delete(
        &self,
        request_json: &str,
    ) -> Result<ControllerLanceDbDeleteResult, BoxError> {
        let request = parse_json_input::<DeleteJsonInput>(request_json)?;
        let engine = self.runtime.open_default_engine().await?;
        let result = engine
            .delete(LanceDbDeleteInput {
                table_name: request.table_name,
                condition: request.condition,
            })
            .await
            .map_err(engine_error_to_box)?;
        Ok(ControllerLanceDbDeleteResult {
            message: result.message,
            version: result.version,
            deleted_rows: result.deleted_rows,
        })
    }

    /// Drop one LanceDB table using the requested logical table name.
    /// 使用请求的逻辑表名删除一张 LanceDB 表。
    pub async fn drop_table(
        &self,
        table_name: &str,
    ) -> Result<ControllerLanceDbDropTableResult, BoxError> {
        let engine = self.runtime.open_default_engine().await?;
        let result = engine
            .drop_table(LanceDbDropTableInput {
                table_name: table_name.to_string(),
            })
            .await
            .map_err(engine_error_to_box)?;
        Ok(ControllerLanceDbDropTableResult {
            message: result.message,
        })
    }
}

/// Convert one controller request into upstream LanceDB runtime config.
/// 将一条控制器请求转换成上游 LanceDB 运行时配置。
fn lancedb_runtime_config_from_request(
    request: &ControllerLanceDbEnableRequest,
) -> DatabaseRuntimeConfig {
    DatabaseRuntimeConfig {
        default_db_path: request.default_db_path.clone(),
        db_root: request.db_root.clone(),
        read_consistency_interval_ms: request.read_consistency_interval_ms,
    }
}

/// Convert one controller request into upstream LanceDB engine options.
/// 将一条控制器请求转换成上游 LanceDB 引擎选项。
fn lancedb_engine_options_from_request(
    request: &ControllerLanceDbEnableRequest,
) -> LanceDbEngineOptions {
    LanceDbEngineOptions {
        max_upsert_payload: request.max_upsert_payload,
        max_search_limit: request.max_search_limit,
        max_concurrent_requests: request.max_concurrent_requests,
    }
}

/// Parse JSON text into the requested controller-side LanceDB input type.
/// 将 JSON 文本解析成请求的控制器侧 LanceDB 输入类型。
fn parse_json_input<T>(request_json: &str) -> Result<T, BoxError>
where
    T: for<'de> Deserialize<'de>,
{
    serde_json::from_str(request_json).map_err(|error| {
        Box::new(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!("failed to parse lancedb request_json: {error}"),
        )) as BoxError
    })
}

/// Map the JSON create-table input into the upstream `vldb-lancedb` create-table request.
/// 将 JSON 建表输入映射成上游 `vldb-lancedb` 建表请求。
fn map_create_table_input(
    input: CreateTableJsonInput,
) -> Result<LanceDbCreateTableInput, BoxError> {
    let columns = input
        .columns
        .into_iter()
        .map(map_create_table_column)
        .collect::<Result<Vec<_>, _>>()?;

    Ok(LanceDbCreateTableInput {
        table_name: input.table_name,
        columns,
        overwrite_if_exists: input.overwrite_if_exists,
    })
}

/// Map one JSON column definition into the upstream LanceDB column definition.
/// 将一条 JSON 列定义映射成上游 LanceDB 列定义。
fn map_create_table_column(input: CreateTableJsonColumn) -> Result<LanceDbColumnDef, BoxError> {
    Ok(LanceDbColumnDef {
        name: input.name,
        column_type: parse_column_type(&input.column_type)?,
        vector_dim: input.vector_dim,
        nullable: input.nullable,
    })
}

/// Map the JSON upsert input into the upstream `vldb-lancedb` upsert request.
/// 将 JSON 写入输入映射成上游 `vldb-lancedb` 写入请求。
fn map_upsert_input(input: UpsertJsonInput, data: Vec<u8>) -> Result<LanceDbUpsertInput, BoxError> {
    Ok(LanceDbUpsertInput {
        table_name: input.table_name,
        input_format: parse_input_format(&input.input_format)?,
        data,
        key_columns: input.key_columns,
    })
}

/// Map the JSON search input into the upstream `vldb-lancedb` search request.
/// 将 JSON 检索输入映射成上游 `vldb-lancedb` 检索请求。
fn map_search_input(input: SearchJsonInput) -> Result<LanceDbSearchInput, BoxError> {
    Ok(LanceDbSearchInput {
        table_name: input.table_name,
        vector: input.vector,
        limit: input.limit,
        filter: input.filter,
        vector_column: input.vector_column,
        output_format: parse_output_format(&input.output_format)?,
    })
}

/// Parse the upstream column type name used by the official backend FFI contract.
/// 解析官方后端 FFI 协议使用的上游列类型名。
fn parse_column_type(value: &str) -> Result<LanceDbColumnType, BoxError> {
    match value.trim().to_ascii_lowercase().as_str() {
        "string" => Ok(LanceDbColumnType::String),
        "int64" => Ok(LanceDbColumnType::Int64),
        "float64" => Ok(LanceDbColumnType::Float64),
        "bool" | "boolean" => Ok(LanceDbColumnType::Bool),
        "vector_float32" | "vector-float32" => Ok(LanceDbColumnType::VectorFloat32),
        "float32" => Ok(LanceDbColumnType::Float32),
        "uint64" => Ok(LanceDbColumnType::Uint64),
        "int32" => Ok(LanceDbColumnType::Int32),
        "uint32" => Ok(LanceDbColumnType::Uint32),
        "unspecified" | "" => Ok(LanceDbColumnType::Unspecified),
        other => Err(invalid_input(format!("unsupported column_type: {other}"))),
    }
}

/// Parse the upstream input-format name used by the official backend FFI contract.
/// 解析官方后端 FFI 协议使用的上游输入格式名。
fn parse_input_format(value: &str) -> Result<LanceDbInputFormat, BoxError> {
    match value.trim().to_ascii_lowercase().as_str() {
        "" | "unspecified" | "json" | "json_rows" | "json-rows" => Ok(LanceDbInputFormat::JsonRows),
        "arrow" | "arrow_ipc" | "arrow-ipc" => Ok(LanceDbInputFormat::ArrowIpc),
        other => Err(invalid_input(format!("unsupported input_format: {other}"))),
    }
}

/// Parse the upstream output-format name used by the official backend FFI contract.
/// 解析官方后端 FFI 协议使用的上游输出格式名。
fn parse_output_format(value: &str) -> Result<LanceDbOutputFormat, BoxError> {
    match value.trim().to_ascii_lowercase().as_str() {
        "" | "unspecified" | "arrow" | "arrow_ipc" | "arrow-ipc" => {
            Ok(LanceDbOutputFormat::ArrowIpc)
        }
        "json" | "json_rows" | "json-rows" => Ok(LanceDbOutputFormat::JsonRows),
        other => Err(invalid_input(format!("unsupported output_format: {other}"))),
    }
}

/// Convert one upstream LanceDB engine error into the controller boxed error type.
/// 将一条上游 LanceDB 引擎错误转换成控制器盒装错误类型。
fn engine_error_to_box(error: LanceDbEngineError) -> BoxError {
    match error.kind {
        vldb_lancedb::engine::LanceDbEngineErrorKind::InvalidArgument => {
            invalid_input(error.message)
        }
        vldb_lancedb::engine::LanceDbEngineErrorKind::Internal => {
            Box::new(std::io::Error::other(error.message))
        }
    }
}

/// Return the default nullable flag for JSON create-table columns.
/// 返回 JSON 建表列的默认可空标记。
fn default_nullable() -> bool {
    true
}

/// Return the default search limit used by the JSON search request.
/// 返回 JSON 检索请求使用的默认搜索限制。
fn default_search_limit() -> u32 {
    10
}

/// Build one boxed invalid-input error.
/// 构造一个盒装无效输入错误。
fn invalid_input(message: impl Into<String>) -> BoxError {
    Box::new(std::io::Error::new(
        std::io::ErrorKind::InvalidInput,
        message.into(),
    ))
}

use std::error::Error;
use std::sync::Arc;

use rusqlite::types::Value as RusqliteValue;
use vldb_controller_client::types::{
    ControllerSqliteCustomWordEntry, ControllerSqliteDictionaryMutationResult,
    ControllerSqliteEnableRequest, ControllerSqliteEnsureFtsIndexResult,
    ControllerSqliteExecuteBatchResult, ControllerSqliteExecuteResult,
    ControllerSqliteFtsMutationResult, ControllerSqliteListCustomWordsResult,
    ControllerSqliteQueryResult, ControllerSqliteQueryStreamResult,
    ControllerSqliteRebuildFtsIndexResult, ControllerSqliteSearchFtsHit,
    ControllerSqliteSearchFtsResult, ControllerSqliteTokenizeResult, ControllerSqliteTokenizerMode,
    ControllerSqliteValue,
};
use vldb_sqlite::fts::{
    delete_fts_document as sqlite_delete_fts_document, ensure_fts_index as sqlite_ensure_fts_index,
    rebuild_fts_index as sqlite_rebuild_fts_index, search_fts as sqlite_search_fts,
    upsert_fts_document as sqlite_upsert_fts_document,
};
use vldb_sqlite::runtime::{
    SqliteHardeningOptions, SqliteOpenOptions, SqlitePragmaOptions, SqliteRuntime,
};
use vldb_sqlite::sql_exec::{
    DEFAULT_IPC_CHUNK_BYTES, execute_batch as sqlite_execute_batch,
    execute_script as sqlite_execute_script, query_json as sqlite_query_json,
    query_stream as sqlite_query_stream,
};
use vldb_sqlite::tokenizer::{
    TokenizerMode as UpstreamTokenizerMode, list_custom_words as sqlite_list_custom_words,
    remove_custom_word as sqlite_remove_custom_word, tokenize_text as sqlite_tokenize_text,
    upsert_custom_word as sqlite_upsert_custom_word,
};

/// Shared error type used by the controller core.
/// 控制器核心复用的共享错误类型。
pub type BoxError = Box<dyn Error + Send + Sync + 'static>;

/// SQLite backend wrapper managed by one controller space slot.
/// 由控制器单个空间槽位管理的 SQLite 后端封装。
#[derive(Debug, Clone)]
pub struct ControllerSqliteBackend {
    runtime: Arc<SqliteRuntime>,
    db_path: String,
}

impl ControllerSqliteBackend {
    /// Create one controller-managed SQLite backend and eagerly validate the target path.
    /// 创建一个由控制器管理的 SQLite 后端，并提前校验目标路径。
    pub fn new(request: &ControllerSqliteEnableRequest) -> Result<Self, BoxError> {
        let options = sqlite_open_options_from_request(request);
        let runtime = SqliteRuntime::with_default_options(options.clone());
        runtime.open_database_with_options(&request.db_path, options.clone())?;
        Ok(Self {
            runtime: Arc::new(runtime),
            db_path: request.db_path.clone(),
        })
    }

    /// Return the concrete database path chosen by the host.
    /// 返回由宿主决定的实际数据库路径。
    pub fn db_path(&self) -> &str {
        &self.db_path
    }

    /// Execute one SQLite script against the backend-owned database using typed parameters.
    /// 使用类型化参数针对后端持有的数据库执行一条 SQLite 脚本。
    pub fn execute_script_typed(
        &self,
        sql: &str,
        params: &[ControllerSqliteValue],
    ) -> Result<ControllerSqliteExecuteResult, BoxError> {
        let bound_values = params
            .iter()
            .map(controller_sqlite_value_to_rusqlite)
            .collect::<Vec<_>>();
        self.execute_script_values(sql, &bound_values)
    }

    /// Execute one SQLite script against the backend-owned database using pre-bound values.
    /// 使用预绑定参数针对后端持有的数据库执行一条 SQLite 脚本。
    fn execute_script_values(
        &self,
        sql: &str,
        bound_values: &[RusqliteValue],
    ) -> Result<ControllerSqliteExecuteResult, BoxError> {
        let handle = self.runtime.open_database(&self.db_path)?;
        let mut connection = handle.open_connection()?;
        let result = sqlite_execute_script(&mut connection, sql, &bound_values)?;
        Ok(ControllerSqliteExecuteResult {
            success: result.success,
            message: result.message,
            rows_changed: result.rows_changed,
            last_insert_rowid: result.last_insert_rowid,
        })
    }

    /// Execute one SQLite batch against the backend-owned database using typed parameter groups.
    /// 使用类型化参数组针对后端持有的数据库执行一组 SQLite 批量语句。
    pub fn execute_batch_typed(
        &self,
        sql: &str,
        batch_params: &[Vec<ControllerSqliteValue>],
    ) -> Result<ControllerSqliteExecuteBatchResult, BoxError> {
        let batch_values = batch_params
            .iter()
            .map(|params| {
                params
                    .iter()
                    .map(controller_sqlite_value_to_rusqlite)
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>();
        self.execute_batch_values(sql, &batch_values)
    }

    /// Execute one SQLite batch against the backend-owned database using pre-bound values.
    /// 使用预绑定参数组针对后端持有的数据库执行一组 SQLite 批量语句。
    fn execute_batch_values(
        &self,
        sql: &str,
        batch_params: &[Vec<RusqliteValue>],
    ) -> Result<ControllerSqliteExecuteBatchResult, BoxError> {
        let handle = self.runtime.open_database(&self.db_path)?;
        let mut connection = handle.open_connection()?;
        let result = sqlite_execute_batch(&mut connection, sql, &batch_params)?;
        Ok(ControllerSqliteExecuteBatchResult {
            success: result.success,
            message: result.message,
            rows_changed: result.rows_changed,
            last_insert_rowid: result.last_insert_rowid,
            statements_executed: result.statements_executed,
        })
    }

    /// Execute one SQLite query and return the JSON row-set payload using typed parameters.
    /// 使用类型化参数执行一条 SQLite 查询并返回 JSON 行集载荷。
    pub fn query_json_typed(
        &self,
        sql: &str,
        params: &[ControllerSqliteValue],
    ) -> Result<ControllerSqliteQueryResult, BoxError> {
        let bound_values = params
            .iter()
            .map(controller_sqlite_value_to_rusqlite)
            .collect::<Vec<_>>();
        self.query_json_values(sql, &bound_values)
    }

    /// Execute one SQLite query using pre-bound values and return the JSON row-set payload.
    /// 使用预绑定参数执行一条 SQLite 查询并返回 JSON 行集载荷。
    fn query_json_values(
        &self,
        sql: &str,
        bound_values: &[RusqliteValue],
    ) -> Result<ControllerSqliteQueryResult, BoxError> {
        let handle = self.runtime.open_database(&self.db_path)?;
        let mut connection = handle.open_connection()?;
        let result = sqlite_query_json(&mut connection, sql, &bound_values)?;
        Ok(ControllerSqliteQueryResult {
            json_data: result.json_data,
            row_count: result.row_count,
        })
    }

    /// Execute one SQLite query and return Arrow IPC chunks using typed parameters.
    /// 使用类型化参数执行一条 SQLite 查询并返回 Arrow IPC 分块。
    pub fn query_stream_typed(
        &self,
        sql: &str,
        params: &[ControllerSqliteValue],
        target_chunk_size: Option<usize>,
    ) -> Result<ControllerSqliteQueryStreamResult, BoxError> {
        let bound_values = params
            .iter()
            .map(controller_sqlite_value_to_rusqlite)
            .collect::<Vec<_>>();
        self.query_stream_values(sql, &bound_values, target_chunk_size)
    }

    /// Execute one SQLite query using pre-bound values and return Arrow IPC chunks.
    /// 使用预绑定参数执行一条 SQLite 查询并返回 Arrow IPC 分块。
    fn query_stream_values(
        &self,
        sql: &str,
        bound_values: &[RusqliteValue],
        target_chunk_size: Option<usize>,
    ) -> Result<ControllerSqliteQueryStreamResult, BoxError> {
        let handle = self.runtime.open_database(&self.db_path)?;
        let mut connection = handle.open_connection()?;
        let result = sqlite_query_stream(
            &mut connection,
            sql,
            &bound_values,
            target_chunk_size.unwrap_or(DEFAULT_IPC_CHUNK_BYTES),
        )?;

        let mut chunks = Vec::with_capacity(usize::try_from(result.chunk_count).unwrap_or(0));
        for index in 0..result.chunk_count {
            chunks.push(result.read_chunk(index as usize)?);
        }

        Ok(ControllerSqliteQueryStreamResult {
            chunks,
            row_count: result.row_count,
            chunk_count: result.chunk_count,
            total_bytes: result.total_bytes,
        })
    }

    /// Tokenize text using the requested tokenizer mode.
    /// 使用请求的分词模式对文本进行切词。
    pub fn tokenize_text(
        &self,
        tokenizer_mode: ControllerSqliteTokenizerMode,
        text: &str,
        search_mode: bool,
    ) -> Result<ControllerSqliteTokenizeResult, BoxError> {
        let handle = self.runtime.open_database(&self.db_path)?;
        let connection = handle.open_connection()?;
        let result = sqlite_tokenize_text(
            Some(&connection),
            map_tokenizer_mode(tokenizer_mode),
            text,
            search_mode,
        )?;
        Ok(ControllerSqliteTokenizeResult {
            tokenizer_mode: result.tokenizer_mode,
            normalized_text: result.normalized_text,
            tokens: result.tokens,
            fts_query: result.fts_query,
        })
    }

    /// List enabled custom dictionary words from the SQLite backend.
    /// 从 SQLite 后端列出已启用的自定义词条目。
    pub fn list_custom_words(&self) -> Result<ControllerSqliteListCustomWordsResult, BoxError> {
        let handle = self.runtime.open_database(&self.db_path)?;
        let connection = handle.open_connection()?;
        let result = sqlite_list_custom_words(&connection)?;
        Ok(ControllerSqliteListCustomWordsResult {
            success: result.success,
            message: result.message,
            words: result
                .words
                .into_iter()
                .map(|entry| ControllerSqliteCustomWordEntry {
                    word: entry.word,
                    weight: entry.weight,
                })
                .collect(),
        })
    }

    /// Insert or update one custom dictionary word.
    /// 插入或更新一条自定义词条目。
    pub fn upsert_custom_word(
        &self,
        word: &str,
        weight: usize,
    ) -> Result<ControllerSqliteDictionaryMutationResult, BoxError> {
        let handle = self.runtime.open_database(&self.db_path)?;
        let connection = handle.open_connection()?;
        let result = sqlite_upsert_custom_word(&connection, word, weight)?;
        Ok(ControllerSqliteDictionaryMutationResult {
            success: result.success,
            message: result.message,
            affected_rows: result.affected_rows,
        })
    }

    /// Remove one custom dictionary word.
    /// 删除一条自定义词条目。
    pub fn remove_custom_word(
        &self,
        word: &str,
    ) -> Result<ControllerSqliteDictionaryMutationResult, BoxError> {
        let handle = self.runtime.open_database(&self.db_path)?;
        let connection = handle.open_connection()?;
        let result = sqlite_remove_custom_word(&connection, word)?;
        Ok(ControllerSqliteDictionaryMutationResult {
            success: result.success,
            message: result.message,
            affected_rows: result.affected_rows,
        })
    }

    /// Ensure the requested FTS index exists.
    /// 确保请求的 FTS 索引存在。
    pub fn ensure_fts_index(
        &self,
        index_name: &str,
        tokenizer_mode: ControllerSqliteTokenizerMode,
    ) -> Result<ControllerSqliteEnsureFtsIndexResult, BoxError> {
        let handle = self.runtime.open_database(&self.db_path)?;
        let connection = handle.open_connection()?;
        let result =
            sqlite_ensure_fts_index(&connection, index_name, map_tokenizer_mode(tokenizer_mode))?;
        Ok(ControllerSqliteEnsureFtsIndexResult {
            success: result.success,
            message: result.message,
            index_name: result.index_name,
            tokenizer_mode: result.tokenizer_mode,
        })
    }

    /// Rebuild the requested FTS index.
    /// 重建请求的 FTS 索引。
    pub fn rebuild_fts_index(
        &self,
        index_name: &str,
        tokenizer_mode: ControllerSqliteTokenizerMode,
    ) -> Result<ControllerSqliteRebuildFtsIndexResult, BoxError> {
        let handle = self.runtime.open_database(&self.db_path)?;
        let connection = handle.open_connection()?;
        let result =
            sqlite_rebuild_fts_index(&connection, index_name, map_tokenizer_mode(tokenizer_mode))?;
        Ok(ControllerSqliteRebuildFtsIndexResult {
            success: result.success,
            message: result.message,
            index_name: result.index_name,
            tokenizer_mode: result.tokenizer_mode,
            reindexed_rows: result.reindexed_rows,
        })
    }

    /// Insert or update one FTS document.
    /// 插入或更新一条 FTS 文档。
    pub fn upsert_fts_document(
        &self,
        index_name: &str,
        tokenizer_mode: ControllerSqliteTokenizerMode,
        id: &str,
        file_path: &str,
        title: &str,
        content: &str,
    ) -> Result<ControllerSqliteFtsMutationResult, BoxError> {
        let handle = self.runtime.open_database(&self.db_path)?;
        let connection = handle.open_connection()?;
        let result = sqlite_upsert_fts_document(
            &connection,
            index_name,
            map_tokenizer_mode(tokenizer_mode),
            id,
            file_path,
            title,
            content,
        )?;
        Ok(ControllerSqliteFtsMutationResult {
            success: result.success,
            message: result.message,
            affected_rows: result.affected_rows,
            index_name: result.index_name,
        })
    }

    /// Delete one FTS document by business identifier.
    /// 按业务标识符删除一条 FTS 文档。
    pub fn delete_fts_document(
        &self,
        index_name: &str,
        id: &str,
    ) -> Result<ControllerSqliteFtsMutationResult, BoxError> {
        let handle = self.runtime.open_database(&self.db_path)?;
        let connection = handle.open_connection()?;
        let result = sqlite_delete_fts_document(&connection, index_name, id)?;
        Ok(ControllerSqliteFtsMutationResult {
            success: result.success,
            message: result.message,
            affected_rows: result.affected_rows,
            index_name: result.index_name,
        })
    }

    /// Execute one FTS search against the requested index.
    /// 针对请求的索引执行一次 FTS 检索。
    pub fn search_fts(
        &self,
        index_name: &str,
        tokenizer_mode: ControllerSqliteTokenizerMode,
        query: &str,
        limit: u32,
        offset: u32,
    ) -> Result<ControllerSqliteSearchFtsResult, BoxError> {
        let handle = self.runtime.open_database(&self.db_path)?;
        let connection = handle.open_connection()?;
        let result = sqlite_search_fts(
            &connection,
            index_name,
            map_tokenizer_mode(tokenizer_mode),
            query,
            limit,
            offset,
        )?;
        Ok(ControllerSqliteSearchFtsResult {
            success: result.success,
            message: result.message,
            index_name: result.index_name,
            tokenizer_mode: result.tokenizer_mode,
            normalized_query: result.normalized_query,
            fts_query: result.fts_query,
            source: result.source,
            query_mode: result.query_mode,
            total: result.total,
            hits: result
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
}

/// Convert one controller request into upstream SQLite open options.
/// 将一条控制器请求转换成上游 SQLite 打开选项。
fn sqlite_open_options_from_request(request: &ControllerSqliteEnableRequest) -> SqliteOpenOptions {
    SqliteOpenOptions {
        connection_pool_size: request.connection_pool_size,
        busy_timeout_ms: request.busy_timeout_ms,
        pragmas: SqlitePragmaOptions {
            journal_mode: request.journal_mode.clone(),
            synchronous: request.synchronous.clone(),
            foreign_keys: request.foreign_keys,
            temp_store: request.temp_store.clone(),
            wal_autocheckpoint_pages: request.wal_autocheckpoint_pages,
            cache_size_kib: request.cache_size_kib,
            mmap_size_bytes: request.mmap_size_bytes,
        },
        hardening: SqliteHardeningOptions {
            enforce_db_file_lock: request.enforce_db_file_lock,
            read_only: request.read_only,
            allow_uri_filenames: request.allow_uri_filenames,
            trusted_schema: request.trusted_schema,
            defensive: request.defensive,
        },
    }
}

/// Convert one controller tokenizer mode into the upstream tokenizer mode.
/// 将控制器分词模式转换成上游分词模式。
fn map_tokenizer_mode(mode: ControllerSqliteTokenizerMode) -> UpstreamTokenizerMode {
    match mode {
        ControllerSqliteTokenizerMode::None => UpstreamTokenizerMode::None,
        ControllerSqliteTokenizerMode::Jieba => UpstreamTokenizerMode::Jieba,
    }
}

/// Convert one shared typed SQLite value into the upstream rusqlite value representation.
/// 将一条共享类型化 SQLite 值转换成上游 rusqlite 值表示。
fn controller_sqlite_value_to_rusqlite(value: &ControllerSqliteValue) -> RusqliteValue {
    match value {
        ControllerSqliteValue::Int64(value) => RusqliteValue::Integer(*value),
        ControllerSqliteValue::Float64(value) => RusqliteValue::Real(*value),
        ControllerSqliteValue::String(value) => RusqliteValue::Text(value.clone()),
        ControllerSqliteValue::Bytes(value) => RusqliteValue::Blob(value.clone()),
        ControllerSqliteValue::Bool(value) => RusqliteValue::Integer(i64::from(*value)),
        ControllerSqliteValue::Null => RusqliteValue::Null,
    }
}

use std::error::Error;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::thread;
use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::types::Value as RusqliteValue;
use vldb_controller_client::types::{
    ControllerSqliteCustomWordEntry, ControllerSqliteDictionaryMutationResult,
    ControllerSqliteEnableRequest, ControllerSqliteEnsureFtsIndexResult,
    ControllerSqliteExecuteBatchResult, ControllerSqliteExecuteResult,
    ControllerSqliteFtsMutationResult, ControllerSqliteListCustomWordsResult,
    ControllerSqliteQueryResult, ControllerSqliteQueryStreamMetrics,
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
    DEFAULT_IPC_CHUNK_BYTES, QueryStreamChunkWriter, QueryStreamMetrics,
    execute_batch as sqlite_execute_batch, execute_script as sqlite_execute_script,
    query_json as sqlite_query_json, query_stream_with_writer as sqlite_query_stream_with_writer,
};
use vldb_sqlite::tokenizer::{
    TokenizerMode as UpstreamTokenizerMode, list_custom_words as sqlite_list_custom_words,
    remove_custom_word as sqlite_remove_custom_word, tokenize_text as sqlite_tokenize_text,
    upsert_custom_word as sqlite_upsert_custom_word,
};

/// Shared error type used by the controller core.
/// 控制器核心复用的共享错误类型。
pub type BoxError = Box<dyn Error + Send + Sync + 'static>;

/// Monotonic counter used to disambiguate controller-owned SQLite spool files.
/// 用于区分控制器持有的 SQLite 暂存文件的单调计数器。
static NEXT_CONTROLLER_SQLITE_STREAM_ID: AtomicU64 = AtomicU64::new(1);

/// SQLite backend wrapper managed by one controller space slot.
/// 由控制器单个空间槽位管理的 SQLite 后端封装。
#[derive(Debug, Clone)]
pub struct ControllerSqliteBackend {
    runtime: Arc<SqliteRuntime>,
    db_path: String,
}

/// Controller-owned file-offset descriptor for one SQLite query-stream chunk.
/// 控制器持有的一条 SQLite 查询流分块文件偏移描述。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ControllerSqliteQueryStreamChunkDescriptor {
    /// Starting offset of the chunk in the spool file.
    /// 分块在暂存文件中的起始偏移。
    offset: u64,
    /// Byte length of the chunk.
    /// 分块字节长度。
    len: u64,
}

/// Shared mutable SQLite query-stream state owned by the controller.
/// 控制器持有的共享可变 SQLite 查询流状态。
#[derive(Debug)]
struct ControllerSqliteQueryStreamSharedInner {
    /// Spool file path used to persist emitted Arrow IPC chunks.
    /// 用于持久化 Arrow IPC 分块的暂存文件路径。
    file_path: PathBuf,
    /// Chunk descriptors that have already been produced.
    /// 已经生成的分块描述符集合。
    chunk_descriptors: Vec<ControllerSqliteQueryStreamChunkDescriptor>,
    /// Final row count once streaming finishes.
    /// 流结束后的最终行数。
    row_count: u64,
    /// Final chunk count once streaming finishes.
    /// 流结束后的最终分块数量。
    chunk_count: u64,
    /// Final total byte size once streaming finishes.
    /// 流结束后的最终总字节数。
    total_bytes: u64,
    /// Whether streaming has finished.
    /// 流式查询是否已经完成。
    complete: bool,
    /// Failure message when background execution fails.
    /// 后台执行失败时记录的错误消息。
    error: Option<String>,
}

/// Shared SQLite query-stream state wrapper owned by the controller.
/// 控制器持有的共享 SQLite 查询流状态包装器。
#[derive(Debug)]
pub struct ControllerSqliteQueryStreamHandle {
    /// Shared mutable state guarded by a mutex.
    /// 由互斥量保护的共享可变状态。
    inner: Mutex<ControllerSqliteQueryStreamSharedInner>,
    /// Condition variable used to wait for chunks or terminal metrics.
    /// 用于等待分块或终态指标的条件变量。
    ready: Condvar,
}

/// Controller-owned spool writer used by background SQLite streaming execution.
/// 控制器后台 SQLite 流式执行使用的暂存文件写入器。
struct ControllerStreamingTempFileWriter {
    file: File,
    state: Arc<ControllerSqliteQueryStreamHandle>,
    pending: Vec<u8>,
    target_chunk_size: usize,
    emitted_chunks: usize,
    emitted_bytes: usize,
    current_offset: u64,
}

impl ControllerSqliteQueryStreamHandle {
    /// Create one shared controller query-stream state backed by a temp spool path.
    /// 创建一个由临时暂存路径支撑的共享控制器查询流状态。
    fn new(file_path: PathBuf) -> Arc<Self> {
        Arc::new(Self {
            inner: Mutex::new(ControllerSqliteQueryStreamSharedInner {
                file_path,
                chunk_descriptors: Vec::new(),
                row_count: 0,
                chunk_count: 0,
                total_bytes: 0,
                complete: false,
                error: None,
            }),
            ready: Condvar::new(),
        })
    }

    /// Register one emitted chunk descriptor and wake waiting readers.
    /// 注册一条已输出分块描述符并唤醒等待读取方。
    fn register_chunk(&self, offset: u64, len: u64) {
        let mut guard = self
            .inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        guard
            .chunk_descriptors
            .push(ControllerSqliteQueryStreamChunkDescriptor { offset, len });
        self.ready.notify_all();
    }

    /// Mark the stream as finished with final metrics.
    /// 使用最终指标将查询流标记为已完成。
    fn finish(&self, metrics: QueryStreamMetrics) {
        let mut guard = self
            .inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        guard.row_count = metrics.row_count;
        guard.chunk_count = metrics.chunk_count;
        guard.total_bytes = metrics.total_bytes;
        guard.complete = true;
        self.ready.notify_all();
    }

    /// Mark the stream as failed and wake all waiters.
    /// 将查询流标记为失败并唤醒全部等待方。
    fn fail(&self, error: String) {
        let mut guard = self
            .inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        guard.error = Some(error);
        guard.complete = true;
        self.ready.notify_all();
    }

    /// Wait for final metrics and return them once ready.
    /// 等待最终指标就绪并在可用时返回。
    pub fn wait_for_metrics(&self) -> Result<ControllerSqliteQueryStreamMetrics, BoxError> {
        let mut guard = self
            .inner
            .lock()
            .map_err(|_| invalid_input("failed to lock sqlite query stream state"))?;
        while !guard.complete {
            guard = self
                .ready
                .wait(guard)
                .map_err(|_| invalid_input("failed to wait for sqlite query stream completion"))?;
        }
        if let Some(error) = guard.error.clone() {
            return Err(invalid_input(error));
        }
        Ok(ControllerSqliteQueryStreamMetrics {
            row_count: guard.row_count,
            chunk_count: guard.chunk_count,
            total_bytes: guard.total_bytes,
        })
    }

    /// Read one chunk by index, blocking until the chunk is available or the stream finishes.
    /// 按下标读取一个分块，在分块可用或流结束前会阻塞等待。
    pub fn read_chunk(&self, index: usize) -> Result<Vec<u8>, BoxError> {
        let (file_path, descriptor) = {
            let mut guard = self
                .inner
                .lock()
                .map_err(|_| invalid_input("failed to lock sqlite query stream state"))?;
            loop {
                if let Some(descriptor) = guard.chunk_descriptors.get(index).copied() {
                    break (guard.file_path.clone(), descriptor);
                }
                if let Some(error) = guard.error.clone() {
                    return Err(invalid_input(error));
                }
                if guard.complete {
                    return Err(invalid_input(format!(
                        "sqlite query stream chunk index `{index}` is out of bounds"
                    )));
                }
                guard = self
                    .ready
                    .wait(guard)
                    .map_err(|_| invalid_input("failed to wait for sqlite query stream chunk"))?;
            }
        };

        let mut file = File::open(&file_path).map_err(|error| {
            invalid_input(format!(
                "open sqlite query stream spool file failed: {error}"
            ))
        })?;
        file.seek(SeekFrom::Start(descriptor.offset))
            .map_err(|error| {
                invalid_input(format!(
                    "seek sqlite query stream spool file failed: {error}"
                ))
            })?;
        let chunk_len = usize::try_from(descriptor.len)
            .map_err(|_| invalid_input("sqlite query stream chunk length exceeds usize"))?;
        let mut chunk = vec![0_u8; chunk_len];
        file.read_exact(&mut chunk).map_err(|error| {
            invalid_input(format!(
                "read sqlite query stream spool chunk failed: {error}"
            ))
        })?;
        Ok(chunk)
    }
}

impl Drop for ControllerSqliteQueryStreamHandle {
    /// Remove the backing spool file when the last stream-handle reference goes away.
    /// 在最后一个流句柄引用释放时删除底层暂存文件。
    fn drop(&mut self) {
        let file_path = self
            .inner
            .lock()
            .map(|guard| guard.file_path.clone())
            .unwrap_or_else(|poisoned| poisoned.into_inner().file_path.clone());
        let _ = std::fs::remove_file(file_path);
    }
}

impl ControllerStreamingTempFileWriter {
    /// Create one temp-file writer that records chunk offsets into the shared stream state.
    /// 创建一个把分块偏移写入共享流状态的临时文件写入器。
    fn new(
        state: Arc<ControllerSqliteQueryStreamHandle>,
        target_chunk_size: usize,
    ) -> Result<Self, BoxError> {
        let chunk_size = target_chunk_size.max(64 * 1024);
        let file_path = {
            let guard = state
                .inner
                .lock()
                .map_err(|_| invalid_input("failed to lock sqlite query stream state"))?;
            guard.file_path.clone()
        };
        let file = File::create(&file_path).map_err(|error| {
            invalid_input(format!(
                "create sqlite query stream spool file failed: {error}"
            ))
        })?;
        Ok(Self {
            file,
            state,
            pending: Vec::with_capacity(chunk_size),
            target_chunk_size: chunk_size,
            emitted_chunks: 0,
            emitted_bytes: 0,
            current_offset: 0,
        })
    }

    /// Emit all currently full chunks from the pending buffer.
    /// 从待写缓冲区中输出当前所有已满分块。
    fn emit_full_chunks(&mut self) -> std::io::Result<()> {
        while self.pending.len() >= self.target_chunk_size {
            let remainder = self.pending.split_off(self.target_chunk_size);
            let chunk = std::mem::replace(&mut self.pending, remainder);
            self.write_chunk(chunk)?;
        }
        Ok(())
    }

    /// Emit the final partial chunk when flushing the writer.
    /// 在刷新写入器时输出最后一个未满分块。
    fn emit_remaining(&mut self) -> std::io::Result<()> {
        if self.pending.is_empty() {
            return Ok(());
        }
        let chunk = std::mem::take(&mut self.pending);
        self.write_chunk(chunk)
    }

    /// Write one chunk into the spool file and register its descriptor.
    /// 把一个分块写入暂存文件并注册描述符。
    fn write_chunk(&mut self, chunk: Vec<u8>) -> std::io::Result<()> {
        self.file.write_all(&chunk)?;
        let chunk_len = u64::try_from(chunk.len()).unwrap_or(u64::MAX);
        self.state.register_chunk(self.current_offset, chunk_len);
        self.current_offset = self.current_offset.saturating_add(chunk_len);
        self.emitted_chunks = self.emitted_chunks.saturating_add(1);
        self.emitted_bytes = self.emitted_bytes.saturating_add(chunk.len());
        Ok(())
    }
}

impl QueryStreamChunkWriter for ControllerStreamingTempFileWriter {
    /// Return the number of chunks emitted so far.
    /// 返回当前已经输出的分块数量。
    fn emitted_chunk_count(&self) -> u64 {
        u64::try_from(self.emitted_chunks).unwrap_or(u64::MAX)
    }

    /// Return the total byte count emitted so far.
    /// 返回当前已经输出的总字节数。
    fn emitted_total_bytes(&self) -> u64 {
        u64::try_from(self.emitted_bytes).unwrap_or(u64::MAX)
    }
}

impl Write for ControllerStreamingTempFileWriter {
    /// Write bytes into the pending buffer and emit full chunks as needed.
    /// 将字节写入待写缓冲区，并按需输出已满分块。
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        if buf.is_empty() {
            return Ok(0);
        }
        self.pending.extend_from_slice(buf);
        self.emit_full_chunks()?;
        Ok(buf.len())
    }

    /// Flush the remaining chunk and the underlying file.
    /// 刷新最后一个分块并同步到底层文件。
    fn flush(&mut self) -> std::io::Result<()> {
        self.emit_remaining()?;
        self.file.flush()
    }
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

    /// Start one controller-managed SQLite query stream using typed parameters.
    /// 使用类型化参数启动一个由控制器管理的 SQLite 查询流。
    pub fn start_query_stream_typed(
        &self,
        sql: &str,
        params: &[ControllerSqliteValue],
        target_chunk_size: Option<usize>,
    ) -> Result<Arc<ControllerSqliteQueryStreamHandle>, BoxError> {
        let bound_values = params
            .iter()
            .map(controller_sqlite_value_to_rusqlite)
            .collect::<Vec<_>>();
        self.start_query_stream_values(sql, &bound_values, target_chunk_size)
    }

    /// Start one controller-managed SQLite query stream using pre-bound values.
    /// 使用预绑定参数启动一个由控制器管理的 SQLite 查询流。
    fn start_query_stream_values(
        &self,
        sql: &str,
        bound_values: &[RusqliteValue],
        target_chunk_size: Option<usize>,
    ) -> Result<Arc<ControllerSqliteQueryStreamHandle>, BoxError> {
        let runtime = Arc::clone(&self.runtime);
        let db_path = self.db_path.clone();
        let sql = sql.to_string();
        let bound_values = bound_values.to_vec();
        let target_chunk_size = target_chunk_size.unwrap_or(DEFAULT_IPC_CHUNK_BYTES);
        let state =
            ControllerSqliteQueryStreamHandle::new(make_controller_query_stream_spool_path());
        let worker_state = Arc::clone(&state);
        thread::spawn(move || {
            let result = (|| -> Result<QueryStreamMetrics, String> {
                let handle = runtime.open_database(&db_path).map_err(|error| {
                    format!("failed to open controller-managed sqlite database: {error}")
                })?;
                let mut connection = handle.open_connection().map_err(|error| {
                    format!("failed to open controller-managed sqlite connection: {error}")
                })?;
                let writer = ControllerStreamingTempFileWriter::new(
                    Arc::clone(&worker_state),
                    target_chunk_size,
                )
                .map_err(|error| error.to_string())?;
                let (_writer, metrics) =
                    sqlite_query_stream_with_writer(&mut connection, &sql, &bound_values, writer)
                        .map_err(|error| error.to_string())?;
                Ok(metrics)
            })();

            match result {
                Ok(metrics) => worker_state.finish(metrics),
                Err(error) => worker_state.fail(error),
            }
        });
        Ok(state)
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

/// Build one unique spool-file path for a controller-owned SQLite query stream.
/// 为控制器持有的 SQLite 查询流构造一个唯一的暂存文件路径。
fn make_controller_query_stream_spool_path() -> PathBuf {
    let unique = NEXT_CONTROLLER_SQLITE_STREAM_ID.fetch_add(1, Ordering::Relaxed);
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|value| value.as_millis())
        .unwrap_or_default();
    std::env::temp_dir().join(format!(
        "vldb-controller-sqlite-query-stream-{}-{}-{}.bin",
        std::process::id(),
        unique,
        millis
    ))
}

/// Build one boxed invalid-input error for local stream helpers.
/// 为本地查询流辅助逻辑构造一个盒装无效输入错误。
fn invalid_input(message: impl Into<String>) -> BoxError {
    Box::new(std::io::Error::new(
        std::io::ErrorKind::InvalidInput,
        message.into(),
    ))
}

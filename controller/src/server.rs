use std::io;
use std::sync::Arc;
use std::time::Duration;

use serde_json::{Value as JsonValue, json};
use tonic::{Request, Response, Status};
use vldb_controller_client::rpc::controller_service_server::ControllerService;
use vldb_controller_client::rpc::{
    AttachSpaceRequest, AttachSpaceResponse, BackendStatus, ClientLeaseSnapshot,
    ControllerProcessMode as ProtoControllerProcessMode,
    ControllerStatusSnapshot as ProtoControllerStatusSnapshot, CreateLanceDbTableRequest,
    CreateLanceDbTableResponse, DeleteLanceDbRequest, DeleteLanceDbResponse,
    DeleteSqliteFtsDocumentRequest, DetachSpaceRequest, DetachSpaceResponse, DisableBackendRequest,
    DisableBackendResponse, DropLanceDbTableRequest, DropLanceDbTableResponse,
    EnableLanceDbRequest, EnableLanceDbResponse, EnableSqliteRequest, EnableSqliteResponse,
    EnsureSqliteFtsIndexRequest, EnsureSqliteFtsIndexResponse, ExecuteSqliteBatchRequest,
    ExecuteSqliteBatchResponse, ExecuteSqliteScriptRequest, ExecuteSqliteScriptResponse,
    GetStatusRequest, GetStatusResponse, LanceDbColumnType as ProtoLanceDbColumnType,
    LanceDbInputFormat as ProtoLanceDbInputFormat, LanceDbOutputFormat as ProtoLanceDbOutputFormat,
    ListClientsRequest, ListClientsResponse, ListSpacesRequest, ListSpacesResponse,
    ListSqliteCustomWordsRequest, ListSqliteCustomWordsResponse, QuerySqliteJsonRequest,
    QuerySqliteJsonResponse, QuerySqliteStreamChunkRequest, QuerySqliteStreamChunkResponse,
    QuerySqliteStreamCloseRequest, QuerySqliteStreamCloseResponse, QuerySqliteStreamRequest,
    QuerySqliteStreamResponse, QuerySqliteStreamWaitMetricsRequest,
    QuerySqliteStreamWaitMetricsResponse, RebuildSqliteFtsIndexRequest,
    RebuildSqliteFtsIndexResponse, RegisterClientRequest, RegisterClientResponse,
    RemoveSqliteCustomWordRequest, RenewClientLeaseRequest, RenewClientLeaseResponse,
    SearchLanceDbRequest, SearchLanceDbResponse, SearchSqliteFtsRequest, SearchSqliteFtsResponse,
    SpaceKind as ProtoSpaceKind, SpaceSnapshot, SqliteDictionaryMutationResponse,
    SqliteFtsMutationResponse, SqliteSearchFtsHit as ProtoSqliteSearchFtsHit,
    SqliteTokenizerMode as ProtoSqliteTokenizerMode, TokenizeSqliteTextRequest,
    TokenizeSqliteTextResponse, UnregisterClientRequest, UnregisterClientResponse,
    UpsertLanceDbRequest, UpsertLanceDbResponse, UpsertSqliteCustomWordRequest,
    UpsertSqliteFtsDocumentRequest,
};
use vldb_controller_client::types::{
    ClientRegistration, ControllerLanceDbEnableRequest, ControllerProcessMode,
    ControllerSqliteEnableRequest, ControllerSqliteSearchFtsHit, ControllerSqliteTokenizerMode,
    ControllerStatusSnapshot, SpaceKind, SpaceRegistration, VldbControllerError,
};

use crate::core::runtime::VldbControllerRuntime;

/// Execute one blocking SQLite operation on Tokio's blocking thread pool.
/// This prevents blocking the async worker threads while SQLite performs I/O.
/// The SQLite connection pool ensures serialized access per database.
/// 在 Tokio 阻塞线程池上执行一条阻塞 SQLite 操作。
/// 这防止在 SQLite 执行 I/O 时阻塞异步 worker 线程。
/// SQLite 连接池保证每个数据库的序列化访问。
async fn run_sqlite_blocking<T, F>(op: F) -> Result<T, Status>
where
    T: Send + 'static,
    F: FnOnce() -> Result<T, Box<dyn std::error::Error + Send + Sync + 'static>> + Send + 'static,
{
    tokio::task::spawn_blocking(op)
        .await
        .map_err(|e| Status::internal(format!("blocking task panicked: {e}")))?
        .map_err(map_box_error)
}

/// gRPC service implementation for the controller control plane and data plane.
/// 控制器控制面与数据面的 gRPC 服务实现。
#[derive(Clone)]
pub struct ControllerGrpcService {
    runtime: Arc<VldbControllerRuntime>,
}

impl ControllerGrpcService {
    /// Create one gRPC service instance from the shared controller runtime.
    /// 从共享控制器运行时创建一个 gRPC 服务实例。
    pub fn new(runtime: Arc<VldbControllerRuntime>) -> Self {
        Self { runtime }
    }

    /// Start the shared runtime maintenance loop and optionally trigger managed shutdown.
    /// 启动共享运行时维护循环，并在需要时触发托管模式自停。
    pub async fn run_runtime_maintenance_loop(
        runtime: Arc<VldbControllerRuntime>,
        shutdown_signal: Option<tokio::sync::oneshot::Sender<()>>,
    ) {
        let mut shutdown_signal = shutdown_signal;
        let mut interval = tokio::time::interval(Duration::from_secs(1));

        loop {
            interval.tick().await;
            if runtime.reap_expired_clients().is_err() {
                continue;
            }
            // Also reap stale query streams
            // 同时清理过期的查询流
            let _ = runtime.reap_stale_query_streams();

            if shutdown_signal.is_some() {
                match runtime.should_shutdown() {
                    Ok(true) => {
                        if let Some(signal) = shutdown_signal.take() {
                            let _ = signal.send(());
                        }
                        break;
                    }
                    Ok(false) => {}
                    Err(_) => {}
                }
            }
        }
    }

    /// Small wrapper to avoid repeating the runtime argument at every call site.
    /// 用于避免在每个调用点重复传递 runtime 参数的小封装。
    fn begin_request(
        &self,
        client_id: Option<&str>,
    ) -> Result<crate::core::runtime::ControllerRequestGuard, Status> {
        self.runtime.begin_request(client_id).map_err(map_box_error)
    }
}

#[tonic::async_trait]
impl ControllerService for ControllerGrpcService {
    /// Return the current controller status snapshot.
    /// 返回当前控制器状态快照。
    async fn get_status(
        &self,
        _request: Request<GetStatusRequest>,
    ) -> Result<Response<GetStatusResponse>, Status> {
        let _guard = self.begin_request(None)?;
        let status = self.runtime.status_snapshot().map_err(map_box_error)?;
        Ok(Response::new(GetStatusResponse {
            status: Some(map_controller_status_snapshot(status)),
        }))
    }

    /// Register one host client lease.
    /// 注册一个宿主客户端租约。
    async fn register_client(
        &self,
        request: Request<RegisterClientRequest>,
    ) -> Result<Response<RegisterClientResponse>, Status> {
        let req = request.into_inner();
        let _guard = self.begin_request(Some(&req.client_id))?;
        let client = self
            .runtime
            .register_client(ClientRegistration {
                client_id: req.client_id,
                host_kind: req.host_kind,
                process_id: req.process_id,
                process_name: req.process_name,
                lease_ttl_secs: zero_as_none(req.lease_ttl_secs),
            })
            .map_err(map_box_error)?;
        Ok(Response::new(RegisterClientResponse {
            client: Some(map_client_snapshot(client)),
        }))
    }

    /// Renew one existing client lease.
    /// 续约一个现有客户端租约。
    async fn renew_client_lease(
        &self,
        request: Request<RenewClientLeaseRequest>,
    ) -> Result<Response<RenewClientLeaseResponse>, Status> {
        let req = request.into_inner();
        let _guard = self.begin_request(Some(&req.client_id))?;
        let client = self
            .runtime
            .renew_client(&req.client_id, zero_as_none(req.lease_ttl_secs))
            .map_err(map_box_error)?;
        Ok(Response::new(RenewClientLeaseResponse {
            client: Some(map_client_snapshot(client)),
        }))
    }

    /// Remove one client lease and release all its attachments.
    /// 移除一个客户端租约并释放它的全部附着。
    async fn unregister_client(
        &self,
        request: Request<UnregisterClientRequest>,
    ) -> Result<Response<UnregisterClientResponse>, Status> {
        let req = request.into_inner();
        let _guard = self.begin_request(Some(&req.client_id))?;
        let removed = self
            .runtime
            .unregister_client(&req.client_id)
            .map_err(map_box_error)?;
        Ok(Response::new(UnregisterClientResponse { removed }))
    }

    /// Return all currently active client leases.
    /// 返回所有当前活跃的客户端租约。
    async fn list_clients(
        &self,
        _request: Request<ListClientsRequest>,
    ) -> Result<Response<ListClientsResponse>, Status> {
        let _guard = self.begin_request(None)?;
        let clients = self
            .runtime
            .list_clients()
            .map_err(map_box_error)?
            .into_iter()
            .map(map_client_snapshot)
            .collect();
        Ok(Response::new(ListClientsResponse { clients }))
    }

    /// Attach one runtime space into the controller registry.
    /// 将一个运行时空间附着到控制器注册表。
    async fn attach_space(
        &self,
        request: Request<AttachSpaceRequest>,
    ) -> Result<Response<AttachSpaceResponse>, Status> {
        let req = request.into_inner();
        let _guard = self.begin_request(optional_client_id(&req.client_id))?;
        let snapshot = self
            .runtime
            .attach_space(
                SpaceRegistration {
                    space_id: req.space_id,
                    space_label: req.space_label,
                    space_kind: map_proto_space_kind(req.space_kind)?,
                    space_root: req.space_root,
                },
                optional_client_id(&req.client_id),
            )
            .map_err(map_box_error)?;
        Ok(Response::new(AttachSpaceResponse {
            space: Some(map_space_snapshot(snapshot)),
        }))
    }

    /// Detach one runtime space from the target client or remove it entirely.
    /// 将一个运行时空间从目标客户端解除附着或整体移除。
    async fn detach_space(
        &self,
        request: Request<DetachSpaceRequest>,
    ) -> Result<Response<DetachSpaceResponse>, Status> {
        let req = request.into_inner();
        let _guard = self.begin_request(optional_client_id(&req.client_id))?;
        let detached = self
            .runtime
            .detach_space(&req.space_id, optional_client_id(&req.client_id))
            .map_err(map_box_error)?;
        let snapshot = self
            .runtime
            .list_spaces()
            .map_err(map_box_error)?
            .into_iter()
            .find(|space| space.space_id == req.space_id)
            .map(map_space_snapshot);
        Ok(Response::new(DetachSpaceResponse {
            detached,
            space: snapshot,
        }))
    }

    /// Return snapshots for all currently attached spaces.
    /// 返回所有当前已附着空间的快照。
    async fn list_spaces(
        &self,
        _request: Request<ListSpacesRequest>,
    ) -> Result<Response<ListSpacesResponse>, Status> {
        let _guard = self.begin_request(None)?;
        let spaces = self
            .runtime
            .list_spaces()
            .map_err(map_box_error)?
            .into_iter()
            .map(map_space_snapshot)
            .collect();
        Ok(Response::new(ListSpacesResponse { spaces }))
    }

    /// Enable one SQLite backend for the target runtime space.
    /// 为目标运行时空间启用一个 SQLite 后端。
    async fn enable_sqlite(
        &self,
        request: Request<EnableSqliteRequest>,
    ) -> Result<Response<EnableSqliteResponse>, Status> {
        let req = request.into_inner();
        let client_id = optional_client_id(&req.client_id).map(String::from);
        let _guard = self.begin_request(client_id.as_deref())?;
        let backend = self
            .runtime
            .enable_sqlite(
                ControllerSqliteEnableRequest {
                    space_id: req.space_id,
                    binding_id: req.binding_id,
                    db_path: req.db_path,
                    connection_pool_size: zero_u32_to_default(req.connection_pool_size, 8) as usize,
                    busy_timeout_ms: zero_u64_to_default(req.busy_timeout_ms, 5_000),
                    journal_mode: default_if_blank(req.journal_mode, "WAL"),
                    synchronous: default_if_blank(req.synchronous, "NORMAL"),
                    foreign_keys: req.foreign_keys,
                    temp_store: default_if_blank(req.temp_store, "MEMORY"),
                    wal_autocheckpoint_pages: zero_u32_to_default(
                        req.wal_autocheckpoint_pages,
                        1_000,
                    ),
                    cache_size_kib: if req.cache_size_kib == 0 {
                        65_536
                    } else {
                        req.cache_size_kib
                    },
                    mmap_size_bytes: zero_u64_to_default(req.mmap_size_bytes, 268_435_456),
                    enforce_db_file_lock: req.enforce_db_file_lock,
                    read_only: req.read_only,
                    allow_uri_filenames: req.allow_uri_filenames,
                    trusted_schema: req.trusted_schema,
                    defensive: req.defensive,
                },
                client_id.as_deref(),
            )
            .map_err(map_box_error)?;
        Ok(Response::new(EnableSqliteResponse {
            backend: Some(map_backend_status(backend)),
        }))
    }

    /// Disable the SQLite backend for the target runtime space.
    /// 为目标运行时空间关闭 SQLite 后端。
    async fn disable_sqlite(
        &self,
        request: Request<DisableBackendRequest>,
    ) -> Result<Response<DisableBackendResponse>, Status> {
        let req = request.into_inner();
        let client_id = optional_client_id(&req.client_id).map(String::from);
        let _guard = self.begin_request(client_id.as_deref())?;
        let disabled = self
            .runtime
            .disable_sqlite(&req.space_id, &req.binding_id, client_id.as_deref())
            .map_err(map_box_error)?;
        Ok(Response::new(DisableBackendResponse { disabled }))
    }

    /// Execute one SQLite script against the target runtime space backend.
    /// 针对目标运行时空间后端执行一条 SQLite 脚本。
    async fn execute_sqlite_script(
        &self,
        request: Request<ExecuteSqliteScriptRequest>,
    ) -> Result<Response<ExecuteSqliteScriptResponse>, Status> {
        let req = request.into_inner();
        let client_id = optional_client_id(&req.client_id).map(String::from);
        let _guard = self.begin_request(client_id.as_deref())?;
        let params: Vec<_> = req
            .params
            .iter()
            .map(proto_sqlite_value_to_typed)
            .collect::<Result<Vec<_>, _>>()?;
        let runtime = self.runtime.clone();
        let space_id = req.space_id.clone();
        let binding_id = req.binding_id.clone();
        let sql = req.sql.clone();
        let result = run_sqlite_blocking(move || {
            runtime.execute_sqlite_script_typed(
                &space_id,
                &binding_id,
                client_id.as_deref(),
                &sql,
                &params,
            )
        })
        .await?;
        Ok(Response::new(ExecuteSqliteScriptResponse {
            success: result.success,
            message: result.message,
            rows_changed: result.rows_changed,
            last_insert_rowid: result.last_insert_rowid,
        }))
    }

    /// Execute one SQLite batch against the target runtime space backend.
    /// 针对目标运行时空间后端执行一组 SQLite 批量语句。
    async fn execute_sqlite_batch(
        &self,
        request: Request<ExecuteSqliteBatchRequest>,
    ) -> Result<Response<ExecuteSqliteBatchResponse>, Status> {
        let req = request.into_inner();
        let client_id = optional_client_id(&req.client_id).map(String::from);
        let _guard = self.begin_request(client_id.as_deref())?;
        let batch_params: Vec<Vec<_>> = req
            .items
            .iter()
            .map(|item| {
                item.params
                    .iter()
                    .map(proto_sqlite_value_to_typed)
                    .collect::<Result<Vec<_>, _>>()
            })
            .collect::<Result<Vec<_>, _>>()?;
        let runtime = self.runtime.clone();
        let space_id = req.space_id.clone();
        let binding_id = req.binding_id.clone();
        let sql = req.sql.clone();
        let result = run_sqlite_blocking(move || {
            runtime.execute_sqlite_batch_typed(
                &space_id,
                &binding_id,
                client_id.as_deref(),
                &sql,
                &batch_params,
            )
        })
        .await?;
        Ok(Response::new(ExecuteSqliteBatchResponse {
            success: result.success,
            message: result.message,
            rows_changed: result.rows_changed,
            last_insert_rowid: result.last_insert_rowid,
            statements_executed: result.statements_executed,
        }))
    }

    /// Execute one SQLite JSON query against the target runtime space backend.
    /// 针对目标运行时空间后端执行一条 SQLite JSON 查询。
    async fn query_sqlite_json(
        &self,
        request: Request<QuerySqliteJsonRequest>,
    ) -> Result<Response<QuerySqliteJsonResponse>, Status> {
        let req = request.into_inner();
        let client_id = optional_client_id(&req.client_id).map(String::from);
        let _guard = self.begin_request(client_id.as_deref())?;
        let params: Vec<_> = req
            .params
            .iter()
            .map(proto_sqlite_value_to_typed)
            .collect::<Result<Vec<_>, _>>()?;
        let runtime = self.runtime.clone();
        let space_id = req.space_id.clone();
        let binding_id = req.binding_id.clone();
        let sql = req.sql.clone();
        let result = run_sqlite_blocking(move || {
            runtime.query_sqlite_json_typed(
                &space_id,
                &binding_id,
                client_id.as_deref(),
                &sql,
                &params,
            )
        })
        .await?;
        Ok(Response::new(QuerySqliteJsonResponse {
            json_data: result.json_data,
            row_count: result.row_count,
        }))
    }

    /// Execute one SQLite streaming query against the target runtime space backend.
    /// 针对目标运行时空间后端执行一条 SQLite 流式查询。
    async fn query_sqlite_stream(
        &self,
        request: Request<QuerySqliteStreamRequest>,
    ) -> Result<Response<QuerySqliteStreamResponse>, Status> {
        let req = request.into_inner();
        let client_id = optional_client_id(&req.client_id).map(String::from);
        let _guard = self.begin_request(client_id.as_deref())?;
        let target_chunk_size =
            (req.target_chunk_size != 0).then_some(req.target_chunk_size as usize);
        let result = self
            .runtime
            .open_sqlite_query_stream_typed(
                &req.space_id,
                &req.binding_id,
                client_id.as_deref(),
                &req.sql,
                &req.params
                    .iter()
                    .map(proto_sqlite_value_to_typed)
                    .collect::<Result<Vec<_>, _>>()?,
                target_chunk_size,
            )
            .map_err(map_box_error)?;
        Ok(Response::new(QuerySqliteStreamResponse {
            stream_id: result.stream_id,
            metrics_ready: result.metrics_ready,
        }))
    }

    /// Wait for one SQLite streaming query to publish terminal metrics.
    /// 等待一条 SQLite 流式查询发布终态指标。
    async fn query_sqlite_stream_wait_metrics(
        &self,
        request: Request<QuerySqliteStreamWaitMetricsRequest>,
    ) -> Result<Response<QuerySqliteStreamWaitMetricsResponse>, Status> {
        let req = request.into_inner();
        let client_id = optional_client_id(&req.client_id).map(String::from);
        let _guard = self.begin_request(client_id.as_deref())?;
        let runtime = self.runtime.clone();
        let stream_id = req.stream_id;
        let result = run_sqlite_blocking(move || {
            runtime.wait_sqlite_query_stream_metrics(stream_id, client_id.as_deref())
        })
        .await?;
        Ok(Response::new(QuerySqliteStreamWaitMetricsResponse {
            row_count: result.row_count,
            chunk_count: result.chunk_count,
            total_bytes: result.total_bytes,
        }))
    }

    /// Read one chunk from one SQLite streaming query.
    /// 从一条 SQLite 流式查询读取一个分块。
    async fn query_sqlite_stream_chunk(
        &self,
        request: Request<QuerySqliteStreamChunkRequest>,
    ) -> Result<Response<QuerySqliteStreamChunkResponse>, Status> {
        let req = request.into_inner();
        let client_id = optional_client_id(&req.client_id).map(String::from);
        let _guard = self.begin_request(client_id.as_deref())?;
        let runtime = self.runtime.clone();
        let stream_id = req.stream_id;
        let index = req.index as usize;
        let chunk = run_sqlite_blocking(move || {
            runtime.read_sqlite_query_stream_chunk(stream_id, index, client_id.as_deref())
        })
        .await?;
        Ok(Response::new(QuerySqliteStreamChunkResponse { chunk }))
    }

    /// Close one SQLite streaming query and release its backing spool resources.
    /// 关闭一条 SQLite 流式查询并释放其底层暂存资源。
    async fn query_sqlite_stream_close(
        &self,
        request: Request<QuerySqliteStreamCloseRequest>,
    ) -> Result<Response<QuerySqliteStreamCloseResponse>, Status> {
        let req = request.into_inner();
        let client_id = optional_client_id(&req.client_id).map(String::from);
        let _guard = self.begin_request(client_id.as_deref())?;
        let runtime = self.runtime.clone();
        let stream_id = req.stream_id;
        let closed = run_sqlite_blocking(move || {
            runtime.close_sqlite_query_stream(stream_id, client_id.as_deref())
        })
        .await?;
        Ok(Response::new(QuerySqliteStreamCloseResponse { closed }))
    }

    /// Tokenize text using the SQLite backend bound to the target runtime space.
    /// 使用绑定到目标运行时空间的 SQLite 后端对文本执行分词。
    async fn tokenize_sqlite_text(
        &self,
        request: Request<TokenizeSqliteTextRequest>,
    ) -> Result<Response<TokenizeSqliteTextResponse>, Status> {
        let req = request.into_inner();
        let client_id = optional_client_id(&req.client_id).map(String::from);
        let _guard = self.begin_request(client_id.as_deref())?;
        let tokenizer_mode = map_sqlite_tokenizer_mode(req.tokenizer_mode)?;
        let runtime = self.runtime.clone();
        let space_id = req.space_id.clone();
        let binding_id = req.binding_id.clone();
        let text = req.text.clone();
        let search_mode = req.search_mode;
        let result = run_sqlite_blocking(move || {
            runtime.tokenize_sqlite_text(
                &space_id,
                &binding_id,
                client_id.as_deref(),
                tokenizer_mode,
                &text,
                search_mode,
            )
        })
        .await?;
        Ok(Response::new(TokenizeSqliteTextResponse {
            tokenizer_mode: result.tokenizer_mode,
            normalized_text: result.normalized_text,
            tokens: result.tokens,
            fts_query: result.fts_query,
        }))
    }

    /// List custom dictionary words from the SQLite backend bound to the target runtime space.
    /// 从绑定到目标运行时空间的 SQLite 后端列出自定义词条目。
    async fn list_sqlite_custom_words(
        &self,
        request: Request<ListSqliteCustomWordsRequest>,
    ) -> Result<Response<ListSqliteCustomWordsResponse>, Status> {
        let req = request.into_inner();
        let client_id = optional_client_id(&req.client_id).map(String::from);
        let _guard = self.begin_request(client_id.as_deref())?;
        let runtime = self.runtime.clone();
        let space_id = req.space_id.clone();
        let binding_id = req.binding_id.clone();
        let result = run_sqlite_blocking(move || {
            runtime.list_sqlite_custom_words(&space_id, &binding_id, client_id.as_deref())
        })
        .await?;
        Ok(Response::new(ListSqliteCustomWordsResponse {
            success: result.success,
            message: result.message,
            words: result
                .words
                .into_iter()
                .map(|item| vldb_controller_client::rpc::SqliteCustomWordItem {
                    word: item.word,
                    weight: item.weight as u64,
                })
                .collect(),
        }))
    }

    /// Insert or update one SQLite custom dictionary word through the bound backend.
    /// 通过绑定后端插入或更新一条 SQLite 自定义词。
    async fn upsert_sqlite_custom_word(
        &self,
        request: Request<UpsertSqliteCustomWordRequest>,
    ) -> Result<Response<SqliteDictionaryMutationResponse>, Status> {
        let req = request.into_inner();
        let client_id = optional_client_id(&req.client_id).map(String::from);
        let _guard = self.begin_request(client_id.as_deref())?;
        let runtime = self.runtime.clone();
        let space_id = req.space_id.clone();
        let binding_id = req.binding_id.clone();
        let word = req.word.clone();
        let weight = req.weight as usize;
        let result = run_sqlite_blocking(move || {
            runtime.upsert_sqlite_custom_word(
                &space_id,
                &binding_id,
                client_id.as_deref(),
                &word,
                weight,
            )
        })
        .await?;
        Ok(Response::new(SqliteDictionaryMutationResponse {
            success: result.success,
            message: result.message,
            affected_rows: result.affected_rows,
        }))
    }

    /// Remove one SQLite custom dictionary word through the bound backend.
    /// 通过绑定后端删除一条 SQLite 自定义词。
    async fn remove_sqlite_custom_word(
        &self,
        request: Request<RemoveSqliteCustomWordRequest>,
    ) -> Result<Response<SqliteDictionaryMutationResponse>, Status> {
        let req = request.into_inner();
        let client_id = optional_client_id(&req.client_id).map(String::from);
        let _guard = self.begin_request(client_id.as_deref())?;
        let runtime = self.runtime.clone();
        let space_id = req.space_id.clone();
        let binding_id = req.binding_id.clone();
        let word = req.word.clone();
        let result = run_sqlite_blocking(move || {
            runtime.remove_sqlite_custom_word(&space_id, &binding_id, client_id.as_deref(), &word)
        })
        .await?;
        Ok(Response::new(SqliteDictionaryMutationResponse {
            success: result.success,
            message: result.message,
            affected_rows: result.affected_rows,
        }))
    }

    /// Ensure one SQLite FTS index through the backend bound to the target runtime space.
    /// 通过绑定到目标运行时空间的后端确认一条 SQLite FTS 索引。
    async fn ensure_sqlite_fts_index(
        &self,
        request: Request<EnsureSqliteFtsIndexRequest>,
    ) -> Result<Response<EnsureSqliteFtsIndexResponse>, Status> {
        let req = request.into_inner();
        let client_id = optional_client_id(&req.client_id).map(String::from);
        let _guard = self.begin_request(client_id.as_deref())?;
        let tokenizer_mode = map_sqlite_tokenizer_mode(req.tokenizer_mode)?;
        let runtime = self.runtime.clone();
        let space_id = req.space_id.clone();
        let binding_id = req.binding_id.clone();
        let index_name = req.index_name.clone();
        let result = run_sqlite_blocking(move || {
            runtime.ensure_sqlite_fts_index(
                &space_id,
                &binding_id,
                client_id.as_deref(),
                &index_name,
                tokenizer_mode,
            )
        })
        .await?;
        Ok(Response::new(EnsureSqliteFtsIndexResponse {
            success: result.success,
            message: result.message,
            index_name: result.index_name,
            tokenizer_mode: result.tokenizer_mode,
        }))
    }

    /// Rebuild one SQLite FTS index through the backend bound to the target runtime space.
    /// 通过绑定到目标运行时空间的后端重建一条 SQLite FTS 索引。
    async fn rebuild_sqlite_fts_index(
        &self,
        request: Request<RebuildSqliteFtsIndexRequest>,
    ) -> Result<Response<RebuildSqliteFtsIndexResponse>, Status> {
        let req = request.into_inner();
        let client_id = optional_client_id(&req.client_id).map(String::from);
        let _guard = self.begin_request(client_id.as_deref())?;
        let tokenizer_mode = map_sqlite_tokenizer_mode(req.tokenizer_mode)?;
        let runtime = self.runtime.clone();
        let space_id = req.space_id.clone();
        let binding_id = req.binding_id.clone();
        let index_name = req.index_name.clone();
        let result = run_sqlite_blocking(move || {
            runtime.rebuild_sqlite_fts_index(
                &space_id,
                &binding_id,
                client_id.as_deref(),
                &index_name,
                tokenizer_mode,
            )
        })
        .await?;
        Ok(Response::new(RebuildSqliteFtsIndexResponse {
            success: result.success,
            message: result.message,
            index_name: result.index_name,
            tokenizer_mode: result.tokenizer_mode,
            reindexed_rows: result.reindexed_rows,
        }))
    }

    /// Upsert one SQLite FTS document through the backend bound to the target runtime space.
    /// 通过绑定到目标运行时空间的后端写入一条 SQLite FTS 文档。
    async fn upsert_sqlite_fts_document(
        &self,
        request: Request<UpsertSqliteFtsDocumentRequest>,
    ) -> Result<Response<SqliteFtsMutationResponse>, Status> {
        let req = request.into_inner();
        let client_id = optional_client_id(&req.client_id).map(String::from);
        let _guard = self.begin_request(client_id.as_deref())?;
        let tokenizer_mode = map_sqlite_tokenizer_mode(req.tokenizer_mode)?;
        let runtime = self.runtime.clone();
        let space_id = req.space_id.clone();
        let binding_id = req.binding_id.clone();
        let index_name = req.index_name.clone();
        let id = req.id.clone();
        let file_path = req.file_path.clone();
        let title = req.title.clone();
        let content = req.content.clone();
        let result = run_sqlite_blocking(move || {
            runtime.upsert_sqlite_fts_document(
                &space_id,
                &binding_id,
                client_id.as_deref(),
                &index_name,
                tokenizer_mode,
                &id,
                &file_path,
                &title,
                &content,
            )
        })
        .await?;
        Ok(Response::new(SqliteFtsMutationResponse {
            success: result.success,
            message: result.message,
            affected_rows: result.affected_rows,
            index_name: result.index_name,
        }))
    }

    /// Delete one SQLite FTS document through the backend bound to the target runtime space.
    /// 通过绑定到目标运行时空间的后端删除一条 SQLite FTS 文档。
    async fn delete_sqlite_fts_document(
        &self,
        request: Request<DeleteSqliteFtsDocumentRequest>,
    ) -> Result<Response<SqliteFtsMutationResponse>, Status> {
        let req = request.into_inner();
        let client_id = optional_client_id(&req.client_id).map(String::from);
        let _guard = self.begin_request(client_id.as_deref())?;
        let runtime = self.runtime.clone();
        let space_id = req.space_id.clone();
        let binding_id = req.binding_id.clone();
        let index_name = req.index_name.clone();
        let id = req.id.clone();
        let result = run_sqlite_blocking(move || {
            runtime.delete_sqlite_fts_document(
                &space_id,
                &binding_id,
                client_id.as_deref(),
                &index_name,
                &id,
            )
        })
        .await?;
        Ok(Response::new(SqliteFtsMutationResponse {
            success: result.success,
            message: result.message,
            affected_rows: result.affected_rows,
            index_name: result.index_name,
        }))
    }

    /// Search one SQLite FTS index through the backend bound to the target runtime space.
    /// 通过绑定到目标运行时空间的后端检索一条 SQLite FTS 索引。
    async fn search_sqlite_fts(
        &self,
        request: Request<SearchSqliteFtsRequest>,
    ) -> Result<Response<SearchSqliteFtsResponse>, Status> {
        let req = request.into_inner();
        let client_id = optional_client_id(&req.client_id).map(String::from);
        let _guard = self.begin_request(client_id.as_deref())?;
        let tokenizer_mode = map_sqlite_tokenizer_mode(req.tokenizer_mode)?;
        let runtime = self.runtime.clone();
        let space_id = req.space_id.clone();
        let binding_id = req.binding_id.clone();
        let index_name = req.index_name.clone();
        let query = req.query.clone();
        let limit = req.limit;
        let offset = req.offset;
        let result = run_sqlite_blocking(move || {
            runtime.search_sqlite_fts(
                &space_id,
                &binding_id,
                client_id.as_deref(),
                &index_name,
                tokenizer_mode,
                &query,
                limit,
                offset,
            )
        })
        .await?;
        Ok(Response::new(SearchSqliteFtsResponse {
            success: result.success,
            message: result.message,
            index_name: result.index_name,
            tokenizer_mode: result.tokenizer_mode,
            normalized_query: result.normalized_query,
            fts_query: result.fts_query,
            source: result.source,
            query_mode: result.query_mode,
            total: result.total,
            hits: result.hits.into_iter().map(map_sqlite_search_hit).collect(),
        }))
    }

    /// Enable one LanceDB backend for the target runtime space.
    /// 为目标运行时空间启用一个 LanceDB 后端。
    async fn enable_lance_db(
        &self,
        request: Request<EnableLanceDbRequest>,
    ) -> Result<Response<EnableLanceDbResponse>, Status> {
        let req = request.into_inner();
        let client_id = optional_client_id(&req.client_id);
        let _guard = self.begin_request(client_id)?;
        let backend = self
            .runtime
            .enable_lancedb(
                ControllerLanceDbEnableRequest {
                    space_id: req.space_id,
                    binding_id: req.binding_id,
                    default_db_path: req.default_db_path,
                    db_root: blank_to_none(req.db_root),
                    read_consistency_interval_ms: zero_as_none(req.read_consistency_interval_ms),
                    max_upsert_payload: zero_u64_to_default(
                        req.max_upsert_payload,
                        50 * 1024 * 1024,
                    ) as usize,
                    max_search_limit: zero_u64_to_default(req.max_search_limit, 10_000) as usize,
                    max_concurrent_requests: zero_u64_to_default(req.max_concurrent_requests, 500)
                        as usize,
                },
                client_id,
            )
            .map_err(map_box_error)?;
        Ok(Response::new(EnableLanceDbResponse {
            backend: Some(map_backend_status(backend)),
        }))
    }

    /// Disable the LanceDB backend for the target runtime space.
    /// 为目标运行时空间关闭 LanceDB 后端。
    async fn disable_lance_db(
        &self,
        request: Request<DisableBackendRequest>,
    ) -> Result<Response<DisableBackendResponse>, Status> {
        let req = request.into_inner();
        let client_id = optional_client_id(&req.client_id);
        let _guard = self.begin_request(client_id)?;
        let disabled = self
            .runtime
            .disable_lancedb(&req.space_id, &req.binding_id, client_id)
            .map_err(map_box_error)?;
        Ok(Response::new(DisableBackendResponse { disabled }))
    }

    /// Create one LanceDB table against the backend bound to the target runtime space.
    /// 针对绑定到目标运行时空间的后端创建一张 LanceDB 表。
    async fn create_lance_db_table(
        &self,
        request: Request<CreateLanceDbTableRequest>,
    ) -> Result<Response<CreateLanceDbTableResponse>, Status> {
        let req = request.into_inner();
        let client_id = optional_client_id(&req.client_id);
        let _guard = self.begin_request(client_id)?;
        let request_json = serde_json::to_string(&json!({
            "table_name": req.table_name,
            "columns": req.columns.into_iter().map(proto_lancedb_column_to_json).collect::<Result<Vec<_>, _>>()?,
            "overwrite_if_exists": req.overwrite_if_exists,
        }))
        .map_err(map_serde_error)?;
        let result = self
            .runtime
            .create_lancedb_table(&req.space_id, &req.binding_id, client_id, &request_json)
            .await
            .map_err(map_box_error)?;
        Ok(Response::new(CreateLanceDbTableResponse {
            message: result.message,
        }))
    }

    /// Upsert LanceDB rows against the backend bound to the target runtime space.
    /// 针对绑定到目标运行时空间的后端写入 LanceDB 行数据。
    async fn upsert_lance_db(
        &self,
        request: Request<UpsertLanceDbRequest>,
    ) -> Result<Response<UpsertLanceDbResponse>, Status> {
        let req = request.into_inner();
        let client_id = optional_client_id(&req.client_id);
        let _guard = self.begin_request(client_id)?;
        let request_json = serde_json::to_string(&json!({
            "table_name": req.table_name,
            "input_format": proto_lancedb_input_format_name(req.input_format)?,
            "key_columns": req.key_columns,
        }))
        .map_err(map_serde_error)?;
        let result = self
            .runtime
            .upsert_lancedb(
                &req.space_id,
                &req.binding_id,
                client_id,
                &request_json,
                req.data,
            )
            .await
            .map_err(map_box_error)?;
        Ok(Response::new(UpsertLanceDbResponse {
            message: result.message,
            version: result.version,
            input_rows: result.input_rows,
            inserted_rows: result.inserted_rows,
            updated_rows: result.updated_rows,
            deleted_rows: result.deleted_rows,
        }))
    }

    /// Search LanceDB rows against the backend bound to the target runtime space.
    /// 针对绑定到目标运行时空间的后端检索 LanceDB 行数据。
    async fn search_lance_db(
        &self,
        request: Request<SearchLanceDbRequest>,
    ) -> Result<Response<SearchLanceDbResponse>, Status> {
        let req = request.into_inner();
        let client_id = optional_client_id(&req.client_id);
        let _guard = self.begin_request(client_id)?;
        let request_json = serde_json::to_string(&json!({
            "table_name": req.table_name,
            "vector": req.vector,
            "limit": req.limit,
            "filter": req.filter,
            "vector_column": req.vector_column,
            "output_format": proto_lancedb_output_format_name(req.output_format)?,
        }))
        .map_err(map_serde_error)?;
        let result = self
            .runtime
            .search_lancedb(&req.space_id, &req.binding_id, client_id, &request_json)
            .await
            .map_err(map_box_error)?;
        Ok(Response::new(SearchLanceDbResponse {
            message: result.message,
            format: result.format,
            rows: result.rows,
            data: result.data,
        }))
    }

    /// Delete LanceDB rows against the backend bound to the target runtime space.
    /// 针对绑定到目标运行时空间的后端删除 LanceDB 行数据。
    async fn delete_lance_db(
        &self,
        request: Request<DeleteLanceDbRequest>,
    ) -> Result<Response<DeleteLanceDbResponse>, Status> {
        let req = request.into_inner();
        let client_id = optional_client_id(&req.client_id);
        let _guard = self.begin_request(client_id)?;
        let request_json = serde_json::to_string(&json!({
            "table_name": req.table_name,
            "condition": req.condition,
        }))
        .map_err(map_serde_error)?;
        let result = self
            .runtime
            .delete_lancedb(&req.space_id, &req.binding_id, client_id, &request_json)
            .await
            .map_err(map_box_error)?;
        Ok(Response::new(DeleteLanceDbResponse {
            message: result.message,
            version: result.version,
            deleted_rows: result.deleted_rows,
        }))
    }

    /// Drop one LanceDB table against the backend bound to the target runtime space.
    /// 针对绑定到目标运行时空间的后端删除一张 LanceDB 表。
    async fn drop_lance_db_table(
        &self,
        request: Request<DropLanceDbTableRequest>,
    ) -> Result<Response<DropLanceDbTableResponse>, Status> {
        let req = request.into_inner();
        let client_id = optional_client_id(&req.client_id);
        let _guard = self.begin_request(client_id)?;
        let result = self
            .runtime
            .drop_lancedb_table(&req.space_id, &req.binding_id, client_id, &req.table_name)
            .await
            .map_err(map_box_error)?;
        Ok(Response::new(DropLanceDbTableResponse {
            message: result.message,
        }))
    }
}

/// Map one backend status into the protobuf representation.
/// 将一条后端状态映射成 protobuf 表示。
fn map_backend_status(status: vldb_controller_client::types::SpaceBackendStatus) -> BackendStatus {
    BackendStatus {
        enabled: status.enabled,
        mode: status.mode,
        target: status.target,
    }
}

/// Map one client snapshot into the protobuf representation.
/// 将一条客户端快照映射成 protobuf 表示。
fn map_client_snapshot(
    snapshot: vldb_controller_client::types::ClientLeaseSnapshot,
) -> ClientLeaseSnapshot {
    ClientLeaseSnapshot {
        client_id: snapshot.client_id,
        host_kind: snapshot.host_kind,
        process_id: snapshot.process_id,
        process_name: snapshot.process_name,
        last_seen_unix_ms: snapshot.last_seen_unix_ms,
        expires_at_unix_ms: snapshot.expires_at_unix_ms,
        attached_space_ids: snapshot.attached_space_ids,
    }
}

/// Map one space snapshot into the protobuf representation.
/// 将一条空间快照映射成 protobuf 表示。
fn map_space_snapshot(snapshot: vldb_controller_client::types::SpaceSnapshot) -> SpaceSnapshot {
    SpaceSnapshot {
        space_id: snapshot.space_id,
        space_label: snapshot.space_label,
        space_kind: map_space_kind(snapshot.space_kind) as i32,
        space_root: snapshot.space_root,
        attached_clients: snapshot.attached_clients as u32,
        sqlite: snapshot.sqlite.map(map_backend_status),
        lancedb: snapshot.lancedb.map(map_backend_status),
    }
}

/// Map one controller status snapshot into the protobuf representation.
/// 将一条控制器状态快照映射成 protobuf 表示。
fn map_controller_status_snapshot(
    snapshot: ControllerStatusSnapshot,
) -> ProtoControllerStatusSnapshot {
    ProtoControllerStatusSnapshot {
        process_mode: map_process_mode(snapshot.process_mode) as i32,
        bind_addr: snapshot.bind_addr,
        started_at_unix_ms: snapshot.started_at_unix_ms,
        last_request_at_unix_ms: snapshot.last_request_at_unix_ms,
        minimum_uptime_secs: snapshot.minimum_uptime_secs,
        idle_timeout_secs: snapshot.idle_timeout_secs,
        default_lease_ttl_secs: snapshot.default_lease_ttl_secs,
        active_clients: snapshot.active_clients as u32,
        attached_spaces: snapshot.attached_spaces as u32,
        inflight_requests: snapshot.inflight_requests as u32,
        shutdown_candidate: snapshot.shutdown_candidate,
    }
}

/// Map one core process mode into the protobuf enum.
/// 将一个核心进程模式映射成 protobuf 枚举。
fn map_process_mode(mode: ControllerProcessMode) -> ProtoControllerProcessMode {
    match mode {
        ControllerProcessMode::Service => ProtoControllerProcessMode::Service,
        ControllerProcessMode::Managed => ProtoControllerProcessMode::Managed,
    }
}

/// Map one core space kind into the protobuf enum.
/// 将一个核心空间类型映射成 protobuf 枚举。
fn map_space_kind(kind: SpaceKind) -> ProtoSpaceKind {
    match kind {
        SpaceKind::Root => ProtoSpaceKind::Root,
        SpaceKind::User => ProtoSpaceKind::User,
        SpaceKind::Project => ProtoSpaceKind::Project,
    }
}

/// Map one protobuf space kind into the core enum.
/// 将一个 protobuf 空间类型映射成核心枚举。
fn map_proto_space_kind(raw_kind: i32) -> Result<SpaceKind, Status> {
    match ProtoSpaceKind::try_from(raw_kind) {
        Ok(ProtoSpaceKind::Root) => Ok(SpaceKind::Root),
        Ok(ProtoSpaceKind::User) => Ok(SpaceKind::User),
        Ok(ProtoSpaceKind::Project) => Ok(SpaceKind::Project),
        _ => Err(Status::invalid_argument(
            "space_kind must be root, user, or project",
        )),
    }
}

/// Map one protobuf tokenizer mode into the shared tokenizer mode.
/// 将一个 protobuf 分词模式映射成共享分词模式。
fn map_sqlite_tokenizer_mode(raw_mode: i32) -> Result<ControllerSqliteTokenizerMode, Status> {
    match ProtoSqliteTokenizerMode::try_from(raw_mode) {
        Ok(ProtoSqliteTokenizerMode::None) => Ok(ControllerSqliteTokenizerMode::None),
        Ok(ProtoSqliteTokenizerMode::Jieba) => Ok(ControllerSqliteTokenizerMode::Jieba),
        _ => Err(Status::invalid_argument(
            "tokenizer_mode must be none or jieba",
        )),
    }
}

/// Convert one protobuf SQLite value into the shared typed representation.
/// 将一条 protobuf SQLite 值转换成共享类型化表示。
fn proto_sqlite_value_to_typed(
    value: &vldb_controller_client::rpc::SqliteValue,
) -> Result<vldb_controller_client::types::ControllerSqliteValue, Status> {
    use vldb_controller_client::rpc::sqlite_value::Kind;
    use vldb_controller_client::types::ControllerSqliteValue;

    match value.kind.as_ref() {
        Some(Kind::Int64Value(value)) => Ok(ControllerSqliteValue::Int64(*value)),
        Some(Kind::Float64Value(value)) => Ok(ControllerSqliteValue::Float64(*value)),
        Some(Kind::StringValue(value)) => Ok(ControllerSqliteValue::String(value.clone())),
        Some(Kind::BytesValue(value)) => Ok(ControllerSqliteValue::Bytes(value.clone())),
        Some(Kind::BoolValue(value)) => Ok(ControllerSqliteValue::Bool(*value)),
        Some(Kind::NullValue(_)) => Ok(ControllerSqliteValue::Null),
        None => Err(Status::invalid_argument(
            "sqlite value kind must be set for every parameter",
        )),
    }
}

/// Convert one protobuf LanceDB column definition into a JSON object expected by the transitional backend wrapper.
/// 将一条 protobuf LanceDB 列定义转换成过渡后端包装层需要的 JSON 对象。
fn proto_lancedb_column_to_json(
    column: vldb_controller_client::rpc::LanceDbColumnDef,
) -> Result<JsonValue, Status> {
    Ok(json!({
        "name": column.name,
        "column_type": proto_lancedb_column_type_name(column.column_type)?,
        "vector_dim": column.vector_dim,
        "nullable": column.nullable,
    }))
}

/// Return the legacy wire name for one protobuf LanceDB column type.
/// 返回一条 protobuf LanceDB 列类型对应的 legacy 线协议名称。
fn proto_lancedb_column_type_name(raw_kind: i32) -> Result<&'static str, Status> {
    match ProtoLanceDbColumnType::try_from(raw_kind) {
        Ok(ProtoLanceDbColumnType::LancedbColumnTypeUnspecified) => Ok("unspecified"),
        Ok(ProtoLanceDbColumnType::LancedbColumnTypeString) => Ok("string"),
        Ok(ProtoLanceDbColumnType::LancedbColumnTypeInt64) => Ok("int64"),
        Ok(ProtoLanceDbColumnType::LancedbColumnTypeFloat64) => Ok("float64"),
        Ok(ProtoLanceDbColumnType::LancedbColumnTypeBool) => Ok("bool"),
        Ok(ProtoLanceDbColumnType::LancedbColumnTypeVectorFloat32) => Ok("vector_float32"),
        Ok(ProtoLanceDbColumnType::LancedbColumnTypeFloat32) => Ok("float32"),
        Ok(ProtoLanceDbColumnType::LancedbColumnTypeUint64) => Ok("uint64"),
        Ok(ProtoLanceDbColumnType::LancedbColumnTypeInt32) => Ok("int32"),
        Ok(ProtoLanceDbColumnType::LancedbColumnTypeUint32) => Ok("uint32"),
        _ => Err(Status::invalid_argument(
            "unsupported lancedb column type value",
        )),
    }
}

/// Return the legacy wire name for one protobuf LanceDB input format.
/// 返回一条 protobuf LanceDB 输入格式对应的 legacy 线协议名称。
fn proto_lancedb_input_format_name(raw_kind: i32) -> Result<&'static str, Status> {
    match ProtoLanceDbInputFormat::try_from(raw_kind) {
        Ok(ProtoLanceDbInputFormat::LancedbInputFormatUnspecified) => Ok("unspecified"),
        Ok(ProtoLanceDbInputFormat::LancedbInputFormatJsonRows) => Ok("json_rows"),
        Ok(ProtoLanceDbInputFormat::LancedbInputFormatArrowIpc) => Ok("arrow_ipc"),
        _ => Err(Status::invalid_argument(
            "unsupported lancedb input_format value",
        )),
    }
}

/// Return the legacy wire name for one protobuf LanceDB output format.
/// 返回一条 protobuf LanceDB 输出格式对应的 legacy 线协议名称。
fn proto_lancedb_output_format_name(raw_kind: i32) -> Result<&'static str, Status> {
    match ProtoLanceDbOutputFormat::try_from(raw_kind) {
        Ok(ProtoLanceDbOutputFormat::LancedbOutputFormatUnspecified) => Ok("unspecified"),
        Ok(ProtoLanceDbOutputFormat::LancedbOutputFormatArrowIpc) => Ok("arrow_ipc"),
        Ok(ProtoLanceDbOutputFormat::LancedbOutputFormatJsonRows) => Ok("json_rows"),
        _ => Err(Status::invalid_argument(
            "unsupported lancedb output_format value",
        )),
    }
}

/// Map one SQLite FTS hit into the protobuf representation.
/// 将一条 SQLite FTS 命中记录映射成 protobuf 表示。
fn map_sqlite_search_hit(hit: ControllerSqliteSearchFtsHit) -> ProtoSqliteSearchFtsHit {
    ProtoSqliteSearchFtsHit {
        id: hit.id,
        file_path: hit.file_path,
        title: hit.title,
        title_highlight: hit.title_highlight,
        content_snippet: hit.content_snippet,
        score: hit.score,
        rank: hit.rank,
        raw_score: hit.raw_score,
    }
}

/// Map one controller-core boxed error into a gRPC status.
/// 将控制器核心的盒装错误映射成 gRPC 状态。
fn map_box_error(error: Box<dyn std::error::Error + Send + Sync>) -> Status {
    if let Some(controller_error) = error.downcast_ref::<VldbControllerError>() {
        match controller_error {
            VldbControllerError::InvalidInput(message)
            | VldbControllerError::ClientNotFound(message)
            | VldbControllerError::SpaceNotFound(message) => {
                Status::invalid_argument(message.clone())
            }
            VldbControllerError::TransportError(source) => Status::unavailable(source.to_string()),
            VldbControllerError::LockPoisoned(message) => {
                Status::internal(format!("lock poisoned: {message}"))
            }
            VldbControllerError::BackendError { backend, source } => {
                Status::internal(format!("{backend} backend error: {source}"))
            }
            VldbControllerError::Other(source) => Status::internal(source.to_string()),
        }
    } else if let Some(io_error) = error.downcast_ref::<io::Error>() {
        match io_error.kind() {
            io::ErrorKind::InvalidInput => Status::invalid_argument(io_error.to_string()),
            _ => Status::internal(io_error.to_string()),
        }
    } else {
        Status::internal(error.to_string())
    }
}

/// Map one serde serialization error into a gRPC invalid-argument status.
/// 将一条 serde 序列化错误映射成 gRPC 无效参数状态。
fn map_serde_error(error: serde_json::Error) -> Status {
    Status::invalid_argument(error.to_string())
}

/// Return `None` when the provided client identifier is blank.
/// 当提供的客户端标识符为空白时返回 `None`。
fn optional_client_id(client_id: &str) -> Option<&str> {
    let trimmed = client_id.trim();
    (!trimmed.is_empty()).then_some(trimmed)
}

/// Convert blank strings into `None`.
/// 将空白字符串转换成 `None`。
fn blank_to_none(value: String) -> Option<String> {
    let trimmed = value.trim();
    (!trimmed.is_empty()).then_some(trimmed.to_string())
}

/// Convert zero values into `None`.
/// 将零值转换成 `None`。
fn zero_as_none(value: u64) -> Option<u64> {
    (value != 0).then_some(value)
}

/// Return the fallback value when the given `u32` is zero.
/// 当给定 `u32` 为零时返回回退值。
fn zero_u32_to_default(value: u32, fallback: u32) -> u32 {
    if value == 0 { fallback } else { value }
}

/// Return the fallback value when the given `u64` is zero.
/// 当给定 `u64` 为零时返回回退值。
fn zero_u64_to_default(value: u64, fallback: u64) -> u64 {
    if value == 0 { fallback } else { value }
}

/// Return the fallback value when the given string is blank.
/// 当给定字符串为空白时返回回退值。
fn default_if_blank(value: String, fallback: &str) -> String {
    if value.trim().is_empty() {
        fallback.to_string()
    } else {
        value
    }
}

#[cfg(test)]
mod tests {
    use super::map_box_error;
    use tonic::Code;
    use vldb_controller_client::types::VldbControllerError;

    #[test]
    fn map_box_error_keeps_controller_invalid_input_as_invalid_argument() {
        let status = map_box_error(VldbControllerError::invalid_input("bad request"));
        assert_eq!(status.code(), Code::InvalidArgument);
    }
}

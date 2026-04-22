use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use crate::core::lancedb::ControllerLanceDbBackend;
use crate::core::sqlite::{ControllerSqliteBackend, ControllerSqliteQueryStreamHandle};
use vldb_controller_client::types::{
    BoxError, ClientLeaseSnapshot, ClientRegistration, ControllerLanceDbCreateTableResult,
    ControllerLanceDbDeleteResult, ControllerLanceDbDropTableResult,
    ControllerLanceDbEnableRequest, ControllerLanceDbSearchResult, ControllerLanceDbUpsertResult,
    ControllerServerConfig, ControllerSqliteDictionaryMutationResult,
    ControllerSqliteEnableRequest, ControllerSqliteEnsureFtsIndexResult,
    ControllerSqliteExecuteBatchResult, ControllerSqliteExecuteResult,
    ControllerSqliteFtsMutationResult, ControllerSqliteListCustomWordsResult,
    ControllerSqliteQueryResult, ControllerSqliteQueryStreamMetrics,
    ControllerSqliteQueryStreamOpenResult, ControllerSqliteRebuildFtsIndexResult,
    ControllerSqliteSearchFtsResult, ControllerSqliteTokenizeResult, ControllerSqliteTokenizerMode,
    ControllerSqliteValue, ControllerStatusSnapshot, SpaceBackendStatus, SpaceRegistration,
    SpaceSnapshot, VldbControllerError,
};

// pub type BoxError defined in types.rs, imported above
// 控制器核心复用的共享错误类型。

/// Space-aware runtime registry, lease tracker, and shutdown policy manager.
/// 面向空间的运行时注册表、租约跟踪器与关闭策略管理器。
pub struct VldbControllerRuntime {
    server_config: ControllerServerConfig,
    started_at: Instant,
    started_at_unix_ms: u64,
    state: Arc<std::sync::Mutex<ControllerState>>,
}

/// RAII request guard that tracks in-flight request counts for shutdown decisions.
/// 用于关闭判定的执行中请求计数 RAII 守卫。
pub struct ControllerRequestGuard {
    state: Arc<std::sync::Mutex<ControllerState>>,
}

/// In-memory controller state shared across requests.
/// 在请求之间共享的控制器内存状态。
struct ControllerState {
    last_request_at: Instant,
    last_request_unix_ms: u64,
    clients: BTreeMap<String, ClientState>,
    spaces: BTreeMap<String, SpaceState>,
    next_query_stream_id: u64,
    sqlite_query_streams: BTreeMap<u64, SqliteQueryStreamEntry>,
    inflight_requests: usize,
}

/// SQLite query stream entry with ownership tracking.
/// 带有所有权跟踪的 SQLite 查询流条目。
struct SqliteQueryStreamEntry {
    handle: Arc<ControllerSqliteQueryStreamHandle>,
    owner_client_id: Option<String>,
    owner_space_id: String,
    last_accessed_at_unix_ms: u64,
}

/// Per-client registration and lease state.
/// 单个客户端的注册与租约状态。
struct ClientState {
    registration: ClientRegistration,
    last_seen_unix_ms: u64,
    expires_at_unix_ms: u64,
    attached_space_ids: BTreeSet<String>,
}

/// In-memory controller state kept for one attached runtime space.
/// 为一个已附着运行时空间保存的控制器内存状态。
struct SpaceState {
    registration: SpaceRegistration,
    attached_clients: BTreeSet<String>,
    sqlite_bindings: BTreeMap<String, ControllerSqliteBackend>,
    lancedb_bindings: BTreeMap<String, ControllerLanceDbBackend>,
}

impl Drop for ControllerRequestGuard {
    /// Release one in-flight request slot when the request scope ends.
    /// 在请求作用域结束时释放一个执行中请求槽位。
    fn drop(&mut self) {
        match self.state.lock() {
            Ok(mut state) => {
                state.inflight_requests = state.inflight_requests.saturating_sub(1);
            }
            Err(poisoned) => {
                let mut state = poisoned.into_inner();
                state.inflight_requests = state.inflight_requests.saturating_sub(1);
            }
        }
    }
}

impl VldbControllerRuntime {
    /// Create one controller runtime from the resolved server configuration.
    /// 根据解析后的服务配置创建一个控制器运行时。
    pub fn new(server_config: ControllerServerConfig) -> Self {
        let now_unix_ms = unix_time_now_ms();
        Self {
            server_config,
            started_at: Instant::now(),
            started_at_unix_ms: now_unix_ms,
            state: Arc::new(std::sync::Mutex::new(ControllerState {
                last_request_at: Instant::now(),
                last_request_unix_ms: now_unix_ms,
                clients: BTreeMap::new(),
                spaces: BTreeMap::new(),
                next_query_stream_id: 1,
                sqlite_query_streams: BTreeMap::new(),
                inflight_requests: 0,
            })),
        }
    }

    /// Acquire the state lock with recovery from poisoning.
    /// 获取状态锁，支持从中毒状态恢复。
    fn lock_state(&self) -> Result<std::sync::MutexGuard<'_, ControllerState>, BoxError> {
        match self.state.lock() {
            Ok(state) => Ok(state),
            Err(poisoned) => {
                tracing::warn!("Controller state lock poisoned, recovering");
                Ok(poisoned.into_inner())
            }
        }
    }

    /// Mark the beginning of one incoming request and optionally refresh the caller lease.
    /// 标记一条传入请求的开始，并可选刷新调用方租约。
    pub fn begin_request(
        &self,
        client_id: Option<&str>,
    ) -> Result<ControllerRequestGuard, BoxError> {
        let now_unix_ms = unix_time_now_ms();
        let mut state = self.lock_state()?;
        state.inflight_requests = state.inflight_requests.saturating_add(1);
        self.prune_expired_clients_locked(&mut state, now_unix_ms);
        state.last_request_at = Instant::now();
        state.last_request_unix_ms = now_unix_ms;

        if let Some(client_id) = client_id
            && !client_id.trim().is_empty()
            && let Some(client) = state.clients.get_mut(client_id)
        {
            let lease_ttl_secs = client
                .registration
                .lease_ttl_secs
                .unwrap_or(self.server_config.runtime.default_lease_ttl_secs);
            client.last_seen_unix_ms = now_unix_ms;
            client.expires_at_unix_ms = now_unix_ms + seconds_to_ms(lease_ttl_secs);
        }

        Ok(ControllerRequestGuard {
            state: Arc::clone(&self.state),
        })
    }

    /// Register one host client lease used for controller ownership and diagnostics.
    /// 注册一个供控制器 ownership 与诊断使用的宿主客户端租约。
    pub fn register_client(
        &self,
        registration: ClientRegistration,
    ) -> Result<ClientLeaseSnapshot, BoxError> {
        validate_client_registration(&registration)?;
        let now_unix_ms = unix_time_now_ms();
        let mut state = self.lock_state()?;
        self.prune_expired_clients_locked(&mut state, now_unix_ms);

        let lease_ttl_secs = registration
            .lease_ttl_secs
            .unwrap_or(self.server_config.runtime.default_lease_ttl_secs);

        let snapshot = {
            let client = state
                .clients
                .entry(registration.client_id.clone())
                .and_modify(|client| {
                    client.registration = registration.clone();
                    client.last_seen_unix_ms = now_unix_ms;
                    client.expires_at_unix_ms = now_unix_ms + seconds_to_ms(lease_ttl_secs);
                })
                .or_insert_with(|| ClientState {
                    registration,
                    last_seen_unix_ms: now_unix_ms,
                    expires_at_unix_ms: now_unix_ms + seconds_to_ms(lease_ttl_secs),
                    attached_space_ids: BTreeSet::new(),
                });
            snapshot_client_state(client)
        };

        Ok(snapshot)
    }

    /// Renew one existing client lease.
    /// 续约一个现有客户端租约。
    pub fn renew_client(
        &self,
        client_id: &str,
        lease_ttl_secs: Option<u64>,
    ) -> Result<ClientLeaseSnapshot, BoxError> {
        let now_unix_ms = unix_time_now_ms();
        let mut state = self.lock_state()?;
        self.prune_expired_clients_locked(&mut state, now_unix_ms);
        let client = state
            .clients
            .get_mut(client_id)
            .ok_or_else(|| invalid_input(format!("client `{client_id}` is not registered")))?;
        let effective_ttl = lease_ttl_secs
            .or(client.registration.lease_ttl_secs)
            .unwrap_or(self.server_config.runtime.default_lease_ttl_secs);
        client.registration.lease_ttl_secs = Some(effective_ttl);
        client.last_seen_unix_ms = now_unix_ms;
        client.expires_at_unix_ms = now_unix_ms + seconds_to_ms(effective_ttl);
        Ok(snapshot_client_state(client))
    }

    /// Unregister one client and release all its space attachments.
    /// 注销一个客户端并释放它的全部空间附着。
    pub fn unregister_client(&self, client_id: &str) -> Result<bool, BoxError> {
        let now_unix_ms = unix_time_now_ms();
        let mut state = self.lock_state()?;
        self.prune_expired_clients_locked(&mut state, now_unix_ms);
        Ok(self.remove_client_locked(&mut state, client_id))
    }

    /// Return snapshots for all currently active clients.
    /// 返回所有当前活跃客户端的快照。
    pub fn list_clients(&self) -> Result<Vec<ClientLeaseSnapshot>, BoxError> {
        let now_unix_ms = unix_time_now_ms();
        let mut state = self.lock_state()?;
        self.prune_expired_clients_locked(&mut state, now_unix_ms);
        Ok(state.clients.values().map(snapshot_client_state).collect())
    }

    /// Attach one host-resolved runtime space into the controller registry.
    /// 将一个宿主已解析的运行时空间附着到控制器注册表。
    pub fn attach_space(
        &self,
        registration: SpaceRegistration,
        client_id: Option<&str>,
    ) -> Result<SpaceSnapshot, BoxError> {
        validate_space_registration(&registration)?;
        let now_unix_ms = unix_time_now_ms();
        let mut state = self.lock_state()?;
        self.prune_expired_clients_locked(&mut state, now_unix_ms);
        if let Some(client_id) = client_id.filter(|value| !value.trim().is_empty())
            && !state.clients.contains_key(client_id)
        {
            return Err(invalid_input(format!(
                "client `{client_id}` is not registered"
            )));
        }

        let space_id = registration.space_id.clone();
        if let Some(existing_space) = state.spaces.get(&space_id) {
            ensure_matching_space_registration(&existing_space.registration, &registration)?;
        } else {
            state.spaces.insert(
                space_id.clone(),
                SpaceState {
                    registration,
                    attached_clients: BTreeSet::new(),
                    sqlite_bindings: BTreeMap::new(),
                    lancedb_bindings: BTreeMap::new(),
                },
            );
        }

        if let Some(client_id) = client_id.filter(|value| !value.trim().is_empty()) {
            state
                .clients
                .get_mut(client_id)
                .expect("client existence validated before insertion")
                .attached_space_ids
                .insert(space_id.clone());
            state
                .spaces
                .get_mut(&space_id)
                .expect("space must exist after insertion")
                .attached_clients
                .insert(client_id.to_string());
        }

        Ok(snapshot_space_state(
            state
                .spaces
                .get(&space_id)
                .expect("space must exist after insertion"),
        ))
    }

    /// Detach one runtime space from one client or remove it entirely when unowned.
    /// 将一个运行时空间从某个客户端解除附着，或在无人持有时整体移除。
    pub fn detach_space(&self, space_id: &str, client_id: Option<&str>) -> Result<bool, BoxError> {
        let now_unix_ms = unix_time_now_ms();
        let mut state = self.lock_state()?;
        self.prune_expired_clients_locked(&mut state, now_unix_ms);

        if !state.spaces.contains_key(space_id) {
            return Ok(false);
        }

        if let Some(client_id) = client_id.filter(|value| !value.trim().is_empty()) {
            let client = state
                .clients
                .get_mut(client_id)
                .ok_or_else(|| invalid_input(format!("client `{client_id}` is not registered")))?;
            if !client.attached_space_ids.contains(space_id) {
                return Err(invalid_input(format!(
                    "client `{client_id}` is not attached to space `{space_id}`"
                )));
            }
            client.attached_space_ids.remove(space_id);
            if let Some(space) = state.spaces.get_mut(space_id) {
                space.attached_clients.remove(client_id);
            }
        }

        let should_remove = {
            let space = state
                .spaces
                .get(space_id)
                .ok_or_else(|| invalid_input(format!("space `{space_id}` is not attached")))?;
            let is_idle = space.attached_clients.is_empty()
                && space.sqlite_bindings.is_empty()
                && space.lancedb_bindings.is_empty();
            if client_id.is_none() && !is_idle {
                return Err(invalid_input(format!(
                    "space `{space_id}` is still attached or has enabled backends and cannot be detached without an explicit client_id"
                )));
            }
            is_idle
        };

        if should_remove {
            let Some(space) = state.spaces.remove(space_id) else {
                return Ok(false);
            };
            for attached_client_id in space.attached_clients {
                if let Some(client) = state.clients.get_mut(&attached_client_id) {
                    client.attached_space_ids.remove(space_id);
                }
            }
            return Ok(true);
        }

        Ok(true)
    }

    /// Enable one SQLite backend for the target runtime space.
    /// 为目标运行时空间启用一个 SQLite 后端。
    pub fn enable_sqlite(
        &self,
        request: ControllerSqliteEnableRequest,
        client_id: Option<&str>,
    ) -> Result<SpaceBackendStatus, BoxError> {
        let mut state = self.lock_state()?;
        self.ensure_space_access_locked(&state, &request.space_id, client_id)?;
        let space = state.spaces.get_mut(&request.space_id).ok_or_else(|| {
            invalid_input(format!("space `{}` is not attached", request.space_id))
        })?;
        let backend = ControllerSqliteBackend::new(&request)?;
        let status = SpaceBackendStatus {
            enabled: true,
            mode: "dynamic_library".to_string(),
            target: backend.db_path().to_string(),
        };
        space.sqlite_bindings.insert(request.binding_id, backend);
        Ok(status)
    }

    /// Disable the SQLite backend for the target runtime space.
    /// 为目标运行时空间关闭 SQLite 后端。
    pub fn disable_sqlite(
        &self,
        space_id: &str,
        binding_id: &str,
        client_id: Option<&str>,
    ) -> Result<bool, BoxError> {
        let mut state = self.lock_state()?;
        self.ensure_space_access_locked(&state, space_id, client_id)?;
        let space = state
            .spaces
            .get_mut(space_id)
            .ok_or_else(|| invalid_input(format!("space `{space_id}` is not attached")))?;
        Ok(space.sqlite_bindings.remove(binding_id).is_some())
    }

    /// Execute one SQLite script against the backend bound to the target runtime space using typed parameters.
    /// 使用类型化参数针对绑定到目标运行时空间的后端执行一条 SQLite 脚本。
    pub fn execute_sqlite_script_typed(
        &self,
        space_id: &str,
        binding_id: &str,
        client_id: Option<&str>,
        sql: &str,
        params: &[ControllerSqliteValue],
    ) -> Result<ControllerSqliteExecuteResult, BoxError> {
        let backend = self.sqlite_backend(space_id, binding_id, client_id)?;

        backend.execute_script_typed(sql, params)
    }

    /// Execute one SQLite JSON query against the backend bound to the target runtime space using typed parameters.
    /// 使用类型化参数针对绑定到目标运行时空间的后端执行一条 SQLite JSON 查询。
    pub fn query_sqlite_json_typed(
        &self,
        space_id: &str,
        binding_id: &str,
        client_id: Option<&str>,
        sql: &str,
        params: &[ControllerSqliteValue],
    ) -> Result<ControllerSqliteQueryResult, BoxError> {
        let backend = self.sqlite_backend(space_id, binding_id, client_id)?;

        backend.query_json_typed(sql, params)
    }

    /// Execute one SQLite batch against the backend bound to the target runtime space using typed parameter groups.
    /// 使用类型化参数组针对绑定到目标运行时空间的后端执行一组 SQLite 批量语句。
    pub fn execute_sqlite_batch_typed(
        &self,
        space_id: &str,
        binding_id: &str,
        client_id: Option<&str>,
        sql: &str,
        batch_params: &[Vec<ControllerSqliteValue>],
    ) -> Result<ControllerSqliteExecuteBatchResult, BoxError> {
        let backend = self.sqlite_backend(space_id, binding_id, client_id)?;

        backend.execute_batch_typed(sql, batch_params)
    }

    /// Open one SQLite streaming query against the backend bound to the target runtime space using typed parameters.
    /// 使用类型化参数针对绑定到目标运行时空间的后端打开一条 SQLite 流式查询。
    pub fn open_sqlite_query_stream_typed(
        &self,
        space_id: &str,
        binding_id: &str,
        client_id: Option<&str>,
        sql: &str,
        params: &[ControllerSqliteValue],
        target_chunk_size: Option<usize>,
    ) -> Result<ControllerSqliteQueryStreamOpenResult, BoxError> {
        let backend = self.sqlite_backend(space_id, binding_id, client_id)?;
        let stream = backend.start_query_stream_typed(sql, params, target_chunk_size)?;
        let mut state = self.lock_state()?;
        let stream_id = state.next_query_stream_id;
        state.next_query_stream_id = state.next_query_stream_id.saturating_add(1).max(1);
        state.sqlite_query_streams.insert(
            stream_id,
            SqliteQueryStreamEntry {
                handle: stream,
                owner_client_id: normalize_client_id(client_id).map(String::from),
                owner_space_id: space_id.to_string(),
                last_accessed_at_unix_ms: unix_time_now_ms(),
            },
        );
        Ok(ControllerSqliteQueryStreamOpenResult {
            stream_id,
            metrics_ready: false,
        })
    }

    /// Wait for terminal metrics of one controller-managed SQLite query stream.
    /// 等待一条由控制器管理的 SQLite 查询流终态指标。
    pub fn wait_sqlite_query_stream_metrics(
        &self,
        stream_id: u64,
        client_id: Option<&str>,
    ) -> Result<ControllerSqliteQueryStreamMetrics, BoxError> {
        let stream = self.sqlite_query_stream_handle(stream_id, client_id)?;
        stream.wait_for_metrics()
    }

    /// Read one chunk from one controller-managed SQLite query stream.
    /// 从一条由控制器管理的 SQLite 查询流读取一个分块。
    pub fn read_sqlite_query_stream_chunk(
        &self,
        stream_id: u64,
        index: usize,
        client_id: Option<&str>,
    ) -> Result<Vec<u8>, BoxError> {
        let stream = self.sqlite_query_stream_handle(stream_id, client_id)?;
        stream.read_chunk(index)
    }

    /// Close one controller-managed SQLite query stream and release its spool resources.
    /// 关闭一条由控制器管理的 SQLite 查询流并释放其暂存资源。
    pub fn close_sqlite_query_stream(
        &self,
        stream_id: u64,
        client_id: Option<&str>,
    ) -> Result<bool, BoxError> {
        // Validate ownership before closing
        // 关闭前校验归属
        self.sqlite_query_stream_handle(stream_id, client_id)?;
        let mut state = self.lock_state()?;
        Ok(state.sqlite_query_streams.remove(&stream_id).is_some())
    }

    /// Tokenize text using the SQLite backend bound to the target runtime space.
    /// 使用绑定到目标运行时空间的 SQLite 后端对文本执行分词。
    pub fn tokenize_sqlite_text(
        &self,
        space_id: &str,
        binding_id: &str,
        client_id: Option<&str>,
        tokenizer_mode: ControllerSqliteTokenizerMode,
        text: &str,
        search_mode: bool,
    ) -> Result<ControllerSqliteTokenizeResult, BoxError> {
        let backend = self.sqlite_backend(space_id, binding_id, client_id)?;

        backend.tokenize_text(tokenizer_mode, text, search_mode)
    }

    /// List custom dictionary words from the SQLite backend bound to the target runtime space.
    /// 从绑定到目标运行时空间的 SQLite 后端列出自定义词条目。
    pub fn list_sqlite_custom_words(
        &self,
        space_id: &str,
        binding_id: &str,
        client_id: Option<&str>,
    ) -> Result<ControllerSqliteListCustomWordsResult, BoxError> {
        let backend = self.sqlite_backend(space_id, binding_id, client_id)?;

        backend.list_custom_words()
    }

    /// Insert or update one SQLite custom dictionary word through the bound backend.
    /// 通过绑定后端插入或更新一条 SQLite 自定义词。
    pub fn upsert_sqlite_custom_word(
        &self,
        space_id: &str,
        binding_id: &str,
        client_id: Option<&str>,
        word: &str,
        weight: usize,
    ) -> Result<ControllerSqliteDictionaryMutationResult, BoxError> {
        let backend = self.sqlite_backend(space_id, binding_id, client_id)?;

        backend.upsert_custom_word(word, weight)
    }

    /// Remove one SQLite custom dictionary word through the bound backend.
    /// 通过绑定后端删除一条 SQLite 自定义词。
    pub fn remove_sqlite_custom_word(
        &self,
        space_id: &str,
        binding_id: &str,
        client_id: Option<&str>,
        word: &str,
    ) -> Result<ControllerSqliteDictionaryMutationResult, BoxError> {
        let backend = self.sqlite_backend(space_id, binding_id, client_id)?;

        backend.remove_custom_word(word)
    }

    /// Ensure one SQLite FTS index through the backend bound to the target runtime space.
    /// 通过绑定到目标运行时空间的后端确认一条 SQLite FTS 索引。
    pub fn ensure_sqlite_fts_index(
        &self,
        space_id: &str,
        binding_id: &str,
        client_id: Option<&str>,
        index_name: &str,
        tokenizer_mode: ControllerSqliteTokenizerMode,
    ) -> Result<ControllerSqliteEnsureFtsIndexResult, BoxError> {
        let backend = self.sqlite_backend(space_id, binding_id, client_id)?;

        backend.ensure_fts_index(index_name, tokenizer_mode)
    }

    /// Rebuild one SQLite FTS index through the backend bound to the target runtime space.
    /// 通过绑定到目标运行时空间的后端重建一条 SQLite FTS 索引。
    pub fn rebuild_sqlite_fts_index(
        &self,
        space_id: &str,
        binding_id: &str,
        client_id: Option<&str>,
        index_name: &str,
        tokenizer_mode: ControllerSqliteTokenizerMode,
    ) -> Result<ControllerSqliteRebuildFtsIndexResult, BoxError> {
        let backend = self.sqlite_backend(space_id, binding_id, client_id)?;

        backend.rebuild_fts_index(index_name, tokenizer_mode)
    }

    /// Upsert one SQLite FTS document through the backend bound to the target runtime space.
    /// 通过绑定到目标运行时空间的后端写入一条 SQLite FTS 文档。
    #[allow(clippy::too_many_arguments)]
    pub fn upsert_sqlite_fts_document(
        &self,
        space_id: &str,
        binding_id: &str,
        client_id: Option<&str>,
        index_name: &str,
        tokenizer_mode: ControllerSqliteTokenizerMode,
        id: &str,
        file_path: &str,
        title: &str,
        content: &str,
    ) -> Result<ControllerSqliteFtsMutationResult, BoxError> {
        let backend = self.sqlite_backend(space_id, binding_id, client_id)?;

        backend.upsert_fts_document(index_name, tokenizer_mode, id, file_path, title, content)
    }

    /// Delete one SQLite FTS document through the backend bound to the target runtime space.
    /// 通过绑定到目标运行时空间的后端删除一条 SQLite FTS 文档。
    pub fn delete_sqlite_fts_document(
        &self,
        space_id: &str,
        binding_id: &str,
        client_id: Option<&str>,
        index_name: &str,
        id: &str,
    ) -> Result<ControllerSqliteFtsMutationResult, BoxError> {
        let backend = self.sqlite_backend(space_id, binding_id, client_id)?;

        backend.delete_fts_document(index_name, id)
    }

    /// Search one SQLite FTS index through the backend bound to the target runtime space.
    /// 通过绑定到目标运行时空间的后端检索一条 SQLite FTS 索引。
    #[allow(clippy::too_many_arguments)]
    pub fn search_sqlite_fts(
        &self,
        space_id: &str,
        binding_id: &str,
        client_id: Option<&str>,
        index_name: &str,
        tokenizer_mode: ControllerSqliteTokenizerMode,
        query: &str,
        limit: u32,
        offset: u32,
    ) -> Result<ControllerSqliteSearchFtsResult, BoxError> {
        let backend = self.sqlite_backend(space_id, binding_id, client_id)?;

        backend.search_fts(index_name, tokenizer_mode, query, limit, offset)
    }

    /// Enable one LanceDB backend for the target runtime space.
    /// 为目标运行时空间启用一个 LanceDB 后端。
    pub fn enable_lancedb(
        &self,
        request: ControllerLanceDbEnableRequest,
        client_id: Option<&str>,
    ) -> Result<SpaceBackendStatus, BoxError> {
        let mut state = self.lock_state()?;
        self.ensure_space_access_locked(&state, &request.space_id, client_id)?;
        let space = state.spaces.get_mut(&request.space_id).ok_or_else(|| {
            invalid_input(format!("space `{}` is not attached", request.space_id))
        })?;
        let backend = ControllerLanceDbBackend::new(&request)?;
        let status = SpaceBackendStatus {
            enabled: true,
            mode: "dynamic_library".to_string(),
            target: backend.default_db_path().to_string(),
        };
        space.lancedb_bindings.insert(request.binding_id, backend);
        Ok(status)
    }

    /// Disable the LanceDB backend for the target runtime space.
    /// 为目标运行时空间关闭 LanceDB 后端。
    pub fn disable_lancedb(
        &self,
        space_id: &str,
        binding_id: &str,
        client_id: Option<&str>,
    ) -> Result<bool, BoxError> {
        let mut state = self.lock_state()?;
        self.ensure_space_access_locked(&state, space_id, client_id)?;
        let space = state
            .spaces
            .get_mut(space_id)
            .ok_or_else(|| invalid_input(format!("space `{space_id}` is not attached")))?;
        Ok(space.lancedb_bindings.remove(binding_id).is_some())
    }

    /// Create one LanceDB table against the backend bound to the target runtime space.
    /// 针对绑定到目标运行时空间的后端创建一张 LanceDB 表。
    pub async fn create_lancedb_table(
        &self,
        space_id: &str,
        binding_id: &str,
        client_id: Option<&str>,
        request_json: &str,
    ) -> Result<ControllerLanceDbCreateTableResult, BoxError> {
        let backend = self.lancedb_backend(space_id, binding_id, client_id)?;

        backend.create_table(request_json).await
    }

    /// Upsert LanceDB rows against the backend bound to the target runtime space.
    /// 针对绑定到目标运行时空间的后端写入 LanceDB 行数据。
    pub async fn upsert_lancedb(
        &self,
        space_id: &str,
        binding_id: &str,
        client_id: Option<&str>,
        request_json: &str,
        data: Vec<u8>,
    ) -> Result<ControllerLanceDbUpsertResult, BoxError> {
        let backend = self.lancedb_backend(space_id, binding_id, client_id)?;

        backend.upsert(request_json, data).await
    }

    /// Search LanceDB rows against the backend bound to the target runtime space.
    /// 针对绑定到目标运行时空间的后端检索 LanceDB 行数据。
    pub async fn search_lancedb(
        &self,
        space_id: &str,
        binding_id: &str,
        client_id: Option<&str>,
        request_json: &str,
    ) -> Result<ControllerLanceDbSearchResult, BoxError> {
        let backend = self.lancedb_backend(space_id, binding_id, client_id)?;

        backend.search(request_json).await
    }

    /// Delete LanceDB rows against the backend bound to the target runtime space.
    /// 针对绑定到目标运行时空间的后端删除 LanceDB 行数据。
    pub async fn delete_lancedb(
        &self,
        space_id: &str,
        binding_id: &str,
        client_id: Option<&str>,
        request_json: &str,
    ) -> Result<ControllerLanceDbDeleteResult, BoxError> {
        let backend = self.lancedb_backend(space_id, binding_id, client_id)?;

        backend.delete(request_json).await
    }

    /// Drop one LanceDB table against the backend bound to the target runtime space.
    /// 针对绑定到目标运行时空间的后端删除一张 LanceDB 表。
    pub async fn drop_lancedb_table(
        &self,
        space_id: &str,
        binding_id: &str,
        client_id: Option<&str>,
        table_name: &str,
    ) -> Result<ControllerLanceDbDropTableResult, BoxError> {
        let backend = self.lancedb_backend(space_id, binding_id, client_id)?;

        backend.drop_table(table_name).await
    }

    /// Return snapshots for all currently attached spaces.
    /// 返回所有当前已附着空间的快照。
    pub fn list_spaces(&self) -> Result<Vec<SpaceSnapshot>, BoxError> {
        let now_unix_ms = unix_time_now_ms();
        let mut state = self.lock_state()?;
        self.prune_expired_clients_locked(&mut state, now_unix_ms);
        Ok(state.spaces.values().map(snapshot_space_state).collect())
    }

    /// Return one high-level controller status snapshot.
    /// 返回一条高层控制器状态快照。
    pub fn status_snapshot(&self) -> Result<ControllerStatusSnapshot, BoxError> {
        let now_unix_ms = unix_time_now_ms();
        let mut state = self.lock_state()?;
        self.prune_expired_clients_locked(&mut state, now_unix_ms);
        Ok(ControllerStatusSnapshot {
            process_mode: self.server_config.runtime.process_mode,
            bind_addr: self.server_config.bind_addr.clone(),
            started_at_unix_ms: self.started_at_unix_ms,
            last_request_at_unix_ms: state.last_request_unix_ms,
            minimum_uptime_secs: self.server_config.runtime.minimum_uptime_secs,
            idle_timeout_secs: self.server_config.runtime.idle_timeout_secs,
            default_lease_ttl_secs: self.server_config.runtime.default_lease_ttl_secs,
            active_clients: state.clients.len(),
            attached_spaces: state.spaces.len(),
            inflight_requests: state.inflight_requests,
            shutdown_candidate: self.should_shutdown_locked(&state),
        })
    }

    /// Determine whether the managed controller currently qualifies for self-shutdown.
    /// 判断当前托管控制器是否已满足自停条件。
    pub fn should_shutdown(&self) -> Result<bool, BoxError> {
        let now_unix_ms = unix_time_now_ms();
        let mut state = self.lock_state()?;
        self.prune_expired_clients_locked(&mut state, now_unix_ms);
        Ok(self.should_shutdown_locked(&state))
    }

    /// Remove expired clients, release their space attachments, and return how many were reaped.
    /// 清理过期客户端、释放其空间附着，并返回被回收的数量。
    pub fn reap_expired_clients(&self) -> Result<usize, BoxError> {
        let now_unix_ms = unix_time_now_ms();
        let mut state = self.lock_state()?;
        Ok(self.prune_expired_clients_locked(&mut state, now_unix_ms))
    }

    /// Remove stale query streams that have been idle for too long.
    /// 清理空闲过久的查询流。
    pub fn reap_stale_query_streams(&self) -> Result<usize, BoxError> {
        let now_unix_ms = unix_time_now_ms();
        let stale_threshold_ms = 300_000; // 5 minutes

        let mut state = self.lock_state()?;
        let before = state.sqlite_query_streams.len();
        state.sqlite_query_streams.retain(|_id, entry| {
            // Reap only completed streams so slow readers or still-running producers are preserved.
            // 仅回收已经完成的流，避免误删慢消费者或仍在运行的生产者。
            let is_complete = entry.handle.is_complete();
            if !is_complete {
                return true;
            }
            let idle_anchor_unix_ms = entry
                .handle
                .completed_at_unix_ms()
                .unwrap_or(entry.last_accessed_at_unix_ms)
                .max(entry.last_accessed_at_unix_ms);
            now_unix_ms.saturating_sub(idle_anchor_unix_ms) < stale_threshold_ms
        });
        let removed = before - state.sqlite_query_streams.len();

        if removed > 0 {
            tracing::warn!(removed, "Reaped stale query streams");
        }

        Ok(removed)
    }

    /// Check whether the configured process mode keeps the controller permanently alive.
    /// 检查当前配置的进程模式是否会让控制器永久存活。
    fn should_shutdown_locked(&self, state: &ControllerState) -> bool {
        if self.server_config.runtime.process_mode
            != vldb_controller_client::types::ControllerProcessMode::Managed
        {
            return false;
        }

        let minimum_uptime = Duration::from_secs(self.server_config.runtime.minimum_uptime_secs);
        if self.started_at.elapsed() < minimum_uptime {
            return false;
        }

        if !state.clients.is_empty() {
            return false;
        }

        if state.inflight_requests > 0 {
            return false;
        }

        // Active query streams keep the controller alive
        // 活跃的查询流让控制器保持存活
        if !state.sqlite_query_streams.is_empty() {
            return false;
        }

        if state.spaces.values().any(|space| {
            !space.attached_clients.is_empty()
                || !space.sqlite_bindings.is_empty()
                || !space.lancedb_bindings.is_empty()
        }) {
            return false;
        }

        let idle_timeout = Duration::from_secs(self.server_config.runtime.idle_timeout_secs);
        state.last_request_at.elapsed() >= idle_timeout
    }

    /// Return the SQLite backend bound to one runtime space and binding identifier.
    /// 返回绑定到某个运行时空间与绑定标识符的 SQLite 后端。
    fn sqlite_backend(
        &self,
        space_id: &str,
        binding_id: &str,
        client_id: Option<&str>,
    ) -> Result<ControllerSqliteBackend, BoxError> {
        let state = self.lock_state()?;
        self.ensure_space_access_locked(&state, space_id, client_id)?;
        let space = state
            .spaces
            .get(space_id)
            .ok_or_else(|| invalid_input(format!("space `{space_id}` is not attached")))?;
        space
            .sqlite_bindings
            .get(binding_id)
            .cloned()
            .ok_or_else(|| {
                invalid_input(format!(
                    "sqlite backend binding `{binding_id}` is not enabled for space `{space_id}`"
                ))
            })
    }

    /// Return the LanceDB backend bound to one runtime space and binding identifier.
    /// 返回绑定到某个运行时空间与绑定标识符的 LanceDB 后端。
    fn lancedb_backend(
        &self,
        space_id: &str,
        binding_id: &str,
        client_id: Option<&str>,
    ) -> Result<ControllerLanceDbBackend, BoxError> {
        let state = self.lock_state()?;
        self.ensure_space_access_locked(&state, space_id, client_id)?;
        let space = state
            .spaces
            .get(space_id)
            .ok_or_else(|| invalid_input(format!("space `{space_id}` is not attached")))?;
        space
            .lancedb_bindings
            .get(binding_id)
            .cloned()
            .ok_or_else(|| {
                invalid_input(format!(
                    "lancedb backend binding `{binding_id}` is not enabled for space `{space_id}`"
                ))
            })
    }

    /// Return one controller-owned SQLite query-stream handle by id.
    /// Return one SQLite query stream handle with ownership validation.
    /// 返回带有所有权校验的 SQLite 查询流句柄。
    fn sqlite_query_stream_handle(
        &self,
        stream_id: u64,
        requesting_client_id: Option<&str>,
    ) -> Result<Arc<ControllerSqliteQueryStreamHandle>, BoxError> {
        let mut state = self.lock_state()?;
        let (handle, owner_client_id, owner_space_id) = {
            let entry = state.sqlite_query_streams.get(&stream_id).ok_or_else(|| {
                invalid_input(format!("sqlite query stream `{stream_id}` not found"))
            })?;
            (
                entry.handle.clone(),
                entry.owner_client_id.clone(),
                entry.owner_space_id.clone(),
            )
        };
        self.ensure_space_access_locked(&state, &owner_space_id, requesting_client_id)?;

        match (
            normalize_client_id(requesting_client_id),
            owner_client_id.as_deref(),
        ) {
            (Some(requested), Some(owner)) if requested != owner => {
                return Err(invalid_input(format!(
                    "sqlite query stream `{stream_id}` belongs to client `{owner}`, not `{requested}`"
                )));
            }
            (None, Some(owner)) => {
                return Err(invalid_input(format!(
                    "sqlite query stream `{stream_id}` requires owner client `{owner}`"
                )));
            }
            _ => {}
        }

        if let Some(entry) = state.sqlite_query_streams.get_mut(&stream_id) {
            entry.last_accessed_at_unix_ms = unix_time_now_ms();
        }

        Ok(handle)
    }

    /// Ensure the caller may access the target space under the current ownership model.
    /// 确保调用方在当前 ownership 模型下可以访问目标空间。
    fn ensure_space_access_locked(
        &self,
        state: &ControllerState,
        space_id: &str,
        client_id: Option<&str>,
    ) -> Result<(), BoxError> {
        let space = state
            .spaces
            .get(space_id)
            .ok_or_else(|| invalid_input(format!("space `{space_id}` is not attached")))?;
        if space.attached_clients.is_empty() {
            return Ok(());
        }

        let Some(client_id) = normalize_client_id(client_id) else {
            return Err(invalid_input(format!(
                "space `{space_id}` requires one attached client_id"
            )));
        };

        if !state.clients.contains_key(client_id) {
            return Err(invalid_input(format!(
                "client `{client_id}` is not registered"
            )));
        }
        if !space.attached_clients.contains(client_id) {
            return Err(invalid_input(format!(
                "client `{client_id}` is not attached to space `{space_id}`"
            )));
        }
        Ok(())
    }

    /// Remove expired client leases and release the corresponding space attachments.
    /// 清理过期客户端租约并释放对应的空间附着。
    fn prune_expired_clients_locked(&self, state: &mut ControllerState, now_unix_ms: u64) -> usize {
        let expired_client_ids = state
            .clients
            .iter()
            .filter_map(|(client_id, client)| {
                (client.expires_at_unix_ms <= now_unix_ms).then_some(client_id.clone())
            })
            .collect::<Vec<_>>();

        let reaped = expired_client_ids.len();
        for client_id in expired_client_ids {
            self.remove_client_locked(state, &client_id);
        }
        reaped
    }

    /// Remove one client from the registry and clear all related space references.
    /// 从注册表移除一个客户端并清理所有相关空间引用。
    fn remove_client_locked(&self, state: &mut ControllerState, client_id: &str) -> bool {
        let Some(client) = state.clients.remove(client_id) else {
            return false;
        };

        for space_id in client.attached_space_ids {
            if let Some(space) = state.spaces.get_mut(&space_id) {
                space.attached_clients.remove(client_id);
            }
        }

        // Clean up query streams owned by this client
        // 清理该客户端拥有的查询流
        state
            .sqlite_query_streams
            .retain(|_id, entry| entry.owner_client_id.as_deref() != Some(client_id));

        let empty_space_ids = state
            .spaces
            .iter()
            .filter_map(|(space_id, space)| {
                (space.attached_clients.is_empty()
                    && space.sqlite_bindings.is_empty()
                    && space.lancedb_bindings.is_empty())
                .then_some(space_id.clone())
            })
            .collect::<Vec<_>>();

        for space_id in empty_space_ids {
            state.spaces.remove(&space_id);
        }

        true
    }
}

/// Convert one internal client state into an external snapshot.
/// 将一个内部客户端状态转换成外部快照。
fn snapshot_client_state(client: &ClientState) -> ClientLeaseSnapshot {
    ClientLeaseSnapshot {
        client_id: client.registration.client_id.clone(),
        host_kind: client.registration.host_kind.clone(),
        process_id: client.registration.process_id,
        process_name: client.registration.process_name.clone(),
        last_seen_unix_ms: client.last_seen_unix_ms,
        expires_at_unix_ms: client.expires_at_unix_ms,
        attached_space_ids: client.attached_space_ids.iter().cloned().collect(),
    }
}

/// Convert one internal space state into an external snapshot.
/// 将一个内部空间状态转换成外部快照。
fn snapshot_space_state(space: &SpaceState) -> SpaceSnapshot {
    SpaceSnapshot {
        space_id: space.registration.space_id.clone(),
        space_label: space.registration.space_label.clone(),
        space_kind: space.registration.space_kind,
        space_root: space.registration.space_root.clone(),
        attached_clients: space.attached_clients.len(),
        sqlite: snapshot_sqlite_backend_status(space),
        lancedb: snapshot_lancedb_backend_status(space),
    }
}

/// Build one aggregated SQLite backend status snapshot for diagnostics.
/// 为诊断构造一条聚合 SQLite 后端状态快照。
fn snapshot_sqlite_backend_status(space: &SpaceState) -> Option<SpaceBackendStatus> {
    if space.sqlite_bindings.is_empty() {
        return None;
    }
    let target = if space.sqlite_bindings.len() == 1 {
        space
            .sqlite_bindings
            .values()
            .next()
            .map(|backend| backend.db_path().to_string())
            .unwrap_or_default()
    } else {
        format!(
            "<multiple sqlite bindings: {}>",
            space.sqlite_bindings.len()
        )
    };
    Some(SpaceBackendStatus {
        enabled: true,
        mode: "dynamic_library".to_string(),
        target,
    })
}

/// Build one aggregated LanceDB backend status snapshot for diagnostics.
/// 为诊断构造一条聚合 LanceDB 后端状态快照。
fn snapshot_lancedb_backend_status(space: &SpaceState) -> Option<SpaceBackendStatus> {
    if space.lancedb_bindings.is_empty() {
        return None;
    }
    let target = if space.lancedb_bindings.len() == 1 {
        space
            .lancedb_bindings
            .values()
            .next()
            .map(|backend| backend.default_db_path().to_string())
            .unwrap_or_default()
    } else {
        format!(
            "<multiple lancedb bindings: {}>",
            space.lancedb_bindings.len()
        )
    };
    Some(SpaceBackendStatus {
        enabled: true,
        mode: "dynamic_library".to_string(),
        target,
    })
}

/// Enforce consistent identity when the same space identifier is attached repeatedly.
/// 当同一空间标识被重复附着时强制保持一致身份。
fn ensure_matching_space_registration(
    existing: &SpaceRegistration,
    incoming: &SpaceRegistration,
) -> Result<(), BoxError> {
    if existing.space_id != incoming.space_id
        || existing.space_label != incoming.space_label
        || existing.space_kind != incoming.space_kind
        || existing.space_root != incoming.space_root
    {
        return Err(invalid_input(format!(
            "space `{}` registration conflicts with existing identity",
            existing.space_id
        )));
    }
    Ok(())
}

/// Validate one host-provided client registration before it enters the registry.
/// 在宿主提供的客户端注册信息进入注册表前进行校验。
fn validate_client_registration(registration: &ClientRegistration) -> Result<(), BoxError> {
    if registration.client_id.trim().is_empty() {
        return Err(invalid_input("client_id must not be empty"));
    }
    if registration.host_kind.trim().is_empty() {
        return Err(invalid_input("host_kind must not be empty"));
    }
    if registration.process_name.trim().is_empty() {
        return Err(invalid_input("process_name must not be empty"));
    }
    Ok(())
}

/// Validate one host-provided space registration before it enters the registry.
/// 在宿主提供的空间注册信息进入注册表前进行校验。
fn validate_space_registration(registration: &SpaceRegistration) -> Result<(), BoxError> {
    if registration.space_id.trim().is_empty() {
        return Err(invalid_input("space_id must not be empty"));
    }
    if registration.space_label.trim().is_empty() {
        return Err(invalid_input("space_label must not be empty"));
    }
    if registration.space_root.trim().is_empty() {
        return Err(invalid_input("space_root must not be empty"));
    }
    Ok(())
}

/// Build one invalid-input error shared by the controller runtime.
/// 构造一个供控制器运行时复用的无效输入错误。
fn invalid_input(message: impl Into<String>) -> BoxError {
    VldbControllerError::invalid_input(message)
}

/// Normalize one optional client identifier by trimming blank content away.
/// 通过裁剪空白内容来标准化一个可选客户端标识符。
fn normalize_client_id(client_id: Option<&str>) -> Option<&str> {
    client_id.and_then(|client_id| {
        let trimmed = client_id.trim();
        (!trimmed.is_empty()).then_some(trimmed)
    })
}

/// Return the current Unix time in milliseconds.
/// 返回当前 Unix 毫秒时间。
fn unix_time_now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

/// Convert whole seconds into milliseconds using saturating arithmetic.
/// 使用饱和算术将秒转换成毫秒。
fn seconds_to_ms(seconds: u64) -> u64 {
    seconds.saturating_mul(1_000)
}

#[cfg(test)]
mod tests {
    use super::VldbControllerRuntime;
    use std::time::{SystemTime, UNIX_EPOCH};
    use vldb_controller_client::types::{
        ClientRegistration, ControllerLanceDbEnableRequest, ControllerRuntimeConfig,
        ControllerServerConfig, ControllerSqliteEnableRequest, ControllerSqliteTokenizerMode,
        ControllerSqliteValue, SpaceKind, SpaceRegistration,
    };

    fn unique_temp_path(label: &str) -> String {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time should be after epoch")
            .as_nanos();
        std::env::temp_dir()
            .join(format!("vldb-controller-{label}-{stamp}"))
            .to_string_lossy()
            .to_string()
    }

    fn runtime() -> VldbControllerRuntime {
        VldbControllerRuntime::new(ControllerServerConfig::default())
    }

    #[test]
    fn runtime_registers_clients_and_tracks_spaces() {
        let runtime = runtime();
        runtime
            .register_client(ClientRegistration {
                client_id: "client-a".to_string(),
                host_kind: "mcp".to_string(),
                process_id: 1234,
                process_name: "mcp.exe".to_string(),
                lease_ttl_secs: Some(60),
            })
            .expect("client should register");

        let snapshot = runtime
            .attach_space(
                SpaceRegistration {
                    space_id: "root".to_string(),
                    space_label: "ROOT".to_string(),
                    space_kind: SpaceKind::Root,
                    space_root: "D:/runtime/root".to_string(),
                },
                Some("client-a"),
            )
            .expect("space should attach");
        assert_eq!(snapshot.attached_clients, 1);

        let clients = runtime.list_clients().expect("client list should succeed");
        assert_eq!(clients.len(), 1);
        assert_eq!(clients[0].attached_space_ids, vec!["root".to_string()]);
    }

    #[test]
    fn runtime_rejects_conflicting_space_identity_for_same_space_id() {
        let runtime = runtime();
        runtime
            .attach_space(
                SpaceRegistration {
                    space_id: "project-a".to_string(),
                    space_label: "PROJECT_A".to_string(),
                    space_kind: SpaceKind::Project,
                    space_root: "D:/project-a/.vulcan".to_string(),
                },
                None,
            )
            .expect("initial space should attach");

        let error = runtime
            .attach_space(
                SpaceRegistration {
                    space_id: "project-a".to_string(),
                    space_label: "PROJECT_B".to_string(),
                    space_kind: SpaceKind::Project,
                    space_root: "D:/project-b/.vulcan".to_string(),
                },
                None,
            )
            .expect_err("conflicting space identity should be rejected");
        assert!(
            error
                .to_string()
                .contains("conflicts with existing identity")
        );
    }

    #[test]
    fn runtime_rejects_anonymous_detach_for_non_idle_shared_space() {
        let runtime = runtime();
        runtime
            .register_client(ClientRegistration {
                client_id: "client-a".to_string(),
                host_kind: "mcp".to_string(),
                process_id: 1234,
                process_name: "mcp.exe".to_string(),
                lease_ttl_secs: Some(60),
            })
            .expect("client should register");
        runtime
            .attach_space(
                SpaceRegistration {
                    space_id: "root".to_string(),
                    space_label: "ROOT".to_string(),
                    space_kind: SpaceKind::Root,
                    space_root: "D:/runtime/root".to_string(),
                },
                Some("client-a"),
            )
            .expect("space should attach");

        let error = runtime
            .detach_space("root", None)
            .expect_err("anonymous detach should reject active shared space");
        assert!(
            error
                .to_string()
                .contains("cannot be detached without an explicit client_id")
        );
    }

    #[test]
    fn runtime_rejects_detach_for_unregistered_client() {
        let runtime = runtime();
        runtime
            .attach_space(
                SpaceRegistration {
                    space_id: "root".to_string(),
                    space_label: "ROOT".to_string(),
                    space_kind: SpaceKind::Root,
                    space_root: "D:/runtime/root".to_string(),
                },
                None,
            )
            .expect("space should attach");

        let error = runtime
            .detach_space("root", Some("missing-client"))
            .expect_err("unregistered client should be rejected");
        assert!(error.to_string().contains("is not registered"));
    }

    #[test]
    fn runtime_rejects_detach_for_client_not_attached_to_space() {
        let runtime = runtime();
        runtime
            .register_client(ClientRegistration {
                client_id: "client-a".to_string(),
                host_kind: "mcp".to_string(),
                process_id: 1234,
                process_name: "mcp.exe".to_string(),
                lease_ttl_secs: Some(60),
            })
            .expect("client should register");
        runtime
            .attach_space(
                SpaceRegistration {
                    space_id: "root".to_string(),
                    space_label: "ROOT".to_string(),
                    space_kind: SpaceKind::Root,
                    space_root: "D:/runtime/root".to_string(),
                },
                None,
            )
            .expect("space should attach");

        let error = runtime
            .detach_space("root", Some("client-a"))
            .expect_err("non-attached client should be rejected");
        assert!(error.to_string().contains("is not attached to space"));
    }

    #[test]
    fn runtime_enables_and_disables_sqlite_backend() {
        let runtime = runtime();
        runtime
            .attach_space(
                SpaceRegistration {
                    space_id: "root".to_string(),
                    space_label: "ROOT".to_string(),
                    space_kind: SpaceKind::Root,
                    space_root: "D:/runtime/root".to_string(),
                },
                None,
            )
            .expect("space should attach");

        let request = ControllerSqliteEnableRequest {
            space_id: "root".to_string(),
            binding_id: "default".to_string(),
            db_path: unique_temp_path("sqlite"),
            ..Default::default()
        };
        let status = runtime
            .enable_sqlite(request.clone(), None)
            .expect("sqlite backend should enable");
        assert_eq!(status.target, request.db_path);
        assert!(
            runtime
                .disable_sqlite("root", "default", None)
                .expect("sqlite disable should succeed")
        );
    }

    #[test]
    fn runtime_executes_sqlite_script_and_query_json() {
        let runtime = runtime();
        runtime
            .attach_space(
                SpaceRegistration {
                    space_id: "root".to_string(),
                    space_label: "ROOT".to_string(),
                    space_kind: SpaceKind::Root,
                    space_root: "D:/runtime/root".to_string(),
                },
                None,
            )
            .expect("space should attach");

        let request = ControllerSqliteEnableRequest {
            space_id: "root".to_string(),
            binding_id: "default".to_string(),
            db_path: unique_temp_path("sqlite-data-plane"),
            ..Default::default()
        };
        runtime
            .enable_sqlite(request, None)
            .expect("sqlite backend should enable");

        runtime
            .execute_sqlite_script_typed(
                "root",
                "default",
                None,
                "CREATE TABLE IF NOT EXISTS items(id INTEGER PRIMARY KEY, name TEXT NOT NULL);",
                &[],
            )
            .expect("table creation should succeed");

        let insert_result = runtime
            .execute_sqlite_script_typed(
                "root",
                "default",
                None,
                "INSERT INTO items(name) VALUES (?1);",
                &[ControllerSqliteValue::String("alpha".to_string())],
            )
            .expect("insert should succeed");
        assert!(insert_result.success);
        assert_eq!(insert_result.rows_changed, 1);

        let query_result = runtime
            .query_sqlite_json_typed(
                "root",
                "default",
                None,
                "SELECT name FROM items ORDER BY id ASC;",
                &[],
            )
            .expect("query_json should succeed");
        assert_eq!(query_result.row_count, 1);
        assert_eq!(query_result.json_data, "[{\"name\":\"alpha\"}]");
    }

    #[test]
    fn runtime_executes_sqlite_batch_and_query_stream() {
        let runtime = runtime();
        runtime
            .attach_space(
                SpaceRegistration {
                    space_id: "root".to_string(),
                    space_label: "ROOT".to_string(),
                    space_kind: SpaceKind::Root,
                    space_root: "D:/runtime/root".to_string(),
                },
                None,
            )
            .expect("space should attach");

        let request = ControllerSqliteEnableRequest {
            space_id: "root".to_string(),
            binding_id: "default".to_string(),
            db_path: unique_temp_path("sqlite-batch-stream"),
            ..Default::default()
        };
        runtime
            .enable_sqlite(request, None)
            .expect("sqlite backend should enable");

        runtime
            .execute_sqlite_script_typed(
                "root",
                "default",
                None,
                "CREATE TABLE IF NOT EXISTS items(id INTEGER PRIMARY KEY, name TEXT NOT NULL);",
                &[],
            )
            .expect("table creation should succeed");

        let batch_result = runtime
            .execute_sqlite_batch_typed(
                "root",
                "default",
                None,
                "INSERT INTO items(name) VALUES (?1);",
                &[
                    vec![ControllerSqliteValue::String("alpha".to_string())],
                    vec![ControllerSqliteValue::String("beta".to_string())],
                ],
            )
            .expect("batch insert should succeed");
        assert!(batch_result.success);
        assert_eq!(batch_result.rows_changed, 2);
        assert_eq!(batch_result.statements_executed, 2);

        let stream_open = runtime
            .open_sqlite_query_stream_typed(
                "root",
                "default",
                None,
                "SELECT id, name FROM items ORDER BY id ASC;",
                &[],
                Some(64 * 1024),
            )
            .expect("query_stream should succeed");
        let metrics = runtime
            .wait_sqlite_query_stream_metrics(stream_open.stream_id, None)
            .expect("query_stream metrics should succeed");
        assert_eq!(metrics.row_count, 2);
        assert!(metrics.chunk_count >= 1);
        let chunk = runtime
            .read_sqlite_query_stream_chunk(stream_open.stream_id, 0, None)
            .expect("query_stream first chunk should succeed");
        assert!(!chunk.is_empty());
        assert!(
            runtime
                .close_sqlite_query_stream(stream_open.stream_id, None)
                .expect("query_stream close should succeed")
        );
    }

    #[test]
    fn runtime_manages_sqlite_custom_words_and_tokenization() {
        let runtime = runtime();
        runtime
            .attach_space(
                SpaceRegistration {
                    space_id: "root".to_string(),
                    space_label: "ROOT".to_string(),
                    space_kind: SpaceKind::Root,
                    space_root: "D:/runtime/root".to_string(),
                },
                None,
            )
            .expect("space should attach");

        let request = ControllerSqliteEnableRequest {
            space_id: "root".to_string(),
            binding_id: "default".to_string(),
            db_path: unique_temp_path("sqlite-tokenizer"),
            ..Default::default()
        };
        runtime
            .enable_sqlite(request, None)
            .expect("sqlite backend should enable");

        let before = runtime
            .tokenize_sqlite_text(
                "root",
                "default",
                None,
                ControllerSqliteTokenizerMode::Jieba,
                "市民田-女士急匆匆",
                false,
            )
            .expect("tokenize before custom word should succeed");
        assert!(!before.tokens.iter().any(|value| value == "田-女士"));

        let upsert = runtime
            .upsert_sqlite_custom_word("root", "default", None, "田-女士", 42)
            .expect("upsert custom word should succeed");
        assert!(upsert.success);

        let listed = runtime
            .list_sqlite_custom_words("root", "default", None)
            .expect("list custom words should succeed");
        assert_eq!(listed.words.len(), 1);
        assert_eq!(listed.words[0].word, "田-女士");

        let after = runtime
            .tokenize_sqlite_text(
                "root",
                "default",
                None,
                ControllerSqliteTokenizerMode::Jieba,
                "市民田-女士急匆匆",
                false,
            )
            .expect("tokenize after custom word should succeed");
        assert!(after.tokens.iter().any(|value| value == "田-女士"));

        let removed = runtime
            .remove_sqlite_custom_word("root", "default", None, "田-女士")
            .expect("remove custom word should succeed");
        assert!(removed.success);
    }

    #[test]
    fn runtime_manages_sqlite_fts_lifecycle() {
        let runtime = runtime();
        runtime
            .attach_space(
                SpaceRegistration {
                    space_id: "root".to_string(),
                    space_label: "ROOT".to_string(),
                    space_kind: SpaceKind::Root,
                    space_root: "D:/runtime/root".to_string(),
                },
                None,
            )
            .expect("space should attach");

        let request = ControllerSqliteEnableRequest {
            space_id: "root".to_string(),
            binding_id: "default".to_string(),
            db_path: unique_temp_path("sqlite-fts"),
            ..Default::default()
        };
        runtime
            .enable_sqlite(request, None)
            .expect("sqlite backend should enable");

        runtime
            .ensure_sqlite_fts_index(
                "root",
                "default",
                None,
                "memory_docs",
                ControllerSqliteTokenizerMode::None,
            )
            .expect("ensure fts index should succeed");
        let upsert = runtime
            .upsert_sqlite_fts_document(
                "root",
                "default",
                None,
                "memory_docs",
                ControllerSqliteTokenizerMode::None,
                "doc-1",
                "/demo/file.md",
                "Alpha Title",
                "alpha beta gamma",
            )
            .expect("upsert fts document should succeed");
        assert!(upsert.success);

        let search = runtime
            .search_sqlite_fts(
                "root",
                "default",
                None,
                "memory_docs",
                ControllerSqliteTokenizerMode::None,
                "alpha",
                10,
                0,
            )
            .expect("search fts should succeed");
        assert_eq!(search.total, 1);
        assert_eq!(search.hits.len(), 1);
        assert_eq!(search.hits[0].id, "doc-1");

        let rebuild = runtime
            .rebuild_sqlite_fts_index(
                "root",
                "default",
                None,
                "memory_docs",
                ControllerSqliteTokenizerMode::None,
            )
            .expect("rebuild fts index should succeed");
        assert!(rebuild.success);
        assert_eq!(rebuild.reindexed_rows, 1);

        let deleted = runtime
            .delete_sqlite_fts_document("root", "default", None, "memory_docs", "doc-1")
            .expect("delete fts document should succeed");
        assert!(deleted.success);
    }

    #[test]
    fn runtime_enables_and_disables_lancedb_backend() {
        let runtime = runtime();
        runtime
            .attach_space(
                SpaceRegistration {
                    space_id: "project-a".to_string(),
                    space_label: "PROJECT_A".to_string(),
                    space_kind: SpaceKind::Project,
                    space_root: "D:/project-a/.vulcan".to_string(),
                },
                None,
            )
            .expect("space should attach");

        let request = ControllerLanceDbEnableRequest {
            space_id: "project-a".to_string(),
            binding_id: "default".to_string(),
            default_db_path: unique_temp_path("lancedb-default"),
            db_root: Some(unique_temp_path("lancedb-root")),
            read_consistency_interval_ms: Some(0),
            ..Default::default()
        };
        let status = runtime
            .enable_lancedb(request.clone(), None)
            .expect("lancedb backend should enable");
        assert_eq!(status.target, request.default_db_path);
        assert!(
            runtime
                .disable_lancedb("project-a", "default", None)
                .expect("lancedb disable should succeed")
        );
    }

    #[tokio::test]
    async fn runtime_lancedb_create_upsert_search_delete_round_trip() {
        let runtime = runtime();
        runtime
            .attach_space(
                SpaceRegistration {
                    space_id: "project-a".to_string(),
                    space_label: "PROJECT_A".to_string(),
                    space_kind: SpaceKind::Project,
                    space_root: "D:/project-a/.vulcan".to_string(),
                },
                None,
            )
            .expect("space should attach");

        let request = ControllerLanceDbEnableRequest {
            space_id: "project-a".to_string(),
            binding_id: "default".to_string(),
            default_db_path: unique_temp_path("lancedb-data-plane-default"),
            db_root: Some(unique_temp_path("lancedb-data-plane-root")),
            ..Default::default()
        };
        runtime
            .enable_lancedb(request, None)
            .expect("lancedb backend should enable");

        runtime
            .create_lancedb_table(
                "project-a",
                "default",
                None,
                r#"{
                  "table_name":"memory",
                  "columns":[
                    {"name":"id","column_type":"string","nullable":false},
                    {"name":"embedding","column_type":"vector_float32","vector_dim":2,"nullable":false},
                    {"name":"text","column_type":"string","nullable":true}
                  ],
                  "overwrite_if_exists":true
                }"#,
            )
            .await
            .expect("create_table should succeed");

        runtime
            .upsert_lancedb(
                "project-a",
                "default",
                None,
                r#"{
                  "table_name":"memory",
                  "input_format":"json_rows",
                  "key_columns":["id"]
                }"#,
                br#"[{"id":"row-1","embedding":[0.1,0.2],"text":"alpha"}]"#.to_vec(),
            )
            .await
            .expect("upsert should succeed");

        let search = runtime
            .search_lancedb(
                "project-a",
                "default",
                None,
                r#"{
                  "table_name":"memory",
                  "vector":[0.1,0.2],
                  "limit":3,
                  "filter":"",
                  "vector_column":"embedding",
                  "output_format":"json"
                }"#,
            )
            .await
            .expect("search should succeed");
        assert_eq!(search.rows, 1);
        assert!(!search.data.is_empty());

        let delete = runtime
            .delete_lancedb(
                "project-a",
                "default",
                None,
                r#"{
                  "table_name":"memory",
                  "condition":"id = 'row-1'"
                }"#,
            )
            .await
            .expect("delete should succeed");
        assert_eq!(delete.deleted_rows, 1);

        let drop_result = runtime
            .drop_lancedb_table("project-a", "default", None, "memory")
            .await
            .expect("drop_table should succeed");
        assert!(drop_result.message.contains("dropped"));
    }

    #[test]
    fn runtime_stream_ownership_validates_client_id() {
        let runtime = runtime();
        let db_path = unique_temp_path("stream-ownership-sqlite");
        let space_root = unique_temp_path("stream-ownership-root");

        // Register two clients
        runtime
            .register_client(ClientRegistration {
                client_id: "client-a".to_string(),
                host_kind: "test".to_string(),
                process_id: 1,
                process_name: "test".to_string(),
                lease_ttl_secs: Some(60),
            })
            .expect("client-a should register");
        runtime
            .register_client(ClientRegistration {
                client_id: "client-b".to_string(),
                host_kind: "test".to_string(),
                process_id: 2,
                process_name: "test".to_string(),
                lease_ttl_secs: Some(60),
            })
            .expect("client-b should register");

        // Attach space and enable SQLite
        runtime
            .attach_space(
                SpaceRegistration {
                    space_id: "root".to_string(),
                    space_label: "ROOT".to_string(),
                    space_kind: SpaceKind::Root,
                    space_root: space_root.clone(),
                },
                Some("client-a"),
            )
            .expect("space should attach");
        runtime
            .attach_space(
                SpaceRegistration {
                    space_id: "root".to_string(),
                    space_label: "ROOT".to_string(),
                    space_kind: SpaceKind::Root,
                    space_root: space_root.clone(),
                },
                Some("client-b"),
            )
            .expect("client-b should attach to the same space");
        runtime
            .enable_sqlite(
                ControllerSqliteEnableRequest {
                    space_id: "root".to_string(),
                    binding_id: "default".to_string(),
                    db_path,
                    ..ControllerSqliteEnableRequest::default()
                },
                Some("client-a"),
            )
            .expect("sqlite should enable");

        // Create a stream owned by client-a (with client_id)
        let stream_open = runtime
            .open_sqlite_query_stream_typed(
                "root",
                "default",
                Some("client-a"),
                "SELECT 1;",
                &[],
                None,
            )
            .expect("stream should open");

        // client-a can access the stream
        assert!(
            runtime
                .read_sqlite_query_stream_chunk(stream_open.stream_id, 0, Some("client-a"))
                .is_ok()
        );

        // client-b CANNOT access client-a's stream
        let error = runtime
            .read_sqlite_query_stream_chunk(stream_open.stream_id, 0, Some("client-b"))
            .expect_err("client-b should not access client-a's stream");
        assert!(
            error.to_string().contains("belongs to client"),
            "error should mention ownership, got: {}",
            error
        );

        let anonymous_error = runtime
            .read_sqlite_query_stream_chunk(stream_open.stream_id, 0, None)
            .expect_err("anonymous callers should not access owned streams");
        assert!(
            anonymous_error
                .to_string()
                .contains("requires one attached client_id")
                || anonymous_error
                    .to_string()
                    .contains("requires owner client"),
            "anonymous error should mention attached client or owner, got: {}",
            anonymous_error
        );

        // Clean up
        runtime
            .close_sqlite_query_stream(stream_open.stream_id, Some("client-a"))
            .expect("close should succeed");
    }

    #[test]
    fn runtime_rejects_data_plane_access_for_unattached_client() {
        let runtime = runtime();
        let db_path = unique_temp_path("sqlite-owned-space");

        runtime
            .register_client(ClientRegistration {
                client_id: "client-a".to_string(),
                host_kind: "test".to_string(),
                process_id: 11,
                process_name: "client-a".to_string(),
                lease_ttl_secs: Some(60),
            })
            .expect("client-a should register");
        runtime
            .register_client(ClientRegistration {
                client_id: "client-b".to_string(),
                host_kind: "test".to_string(),
                process_id: 12,
                process_name: "client-b".to_string(),
                lease_ttl_secs: Some(60),
            })
            .expect("client-b should register");
        runtime
            .attach_space(
                SpaceRegistration {
                    space_id: "root".to_string(),
                    space_label: "ROOT".to_string(),
                    space_kind: SpaceKind::Root,
                    space_root: "D:/runtime/root".to_string(),
                },
                Some("client-a"),
            )
            .expect("space should attach to client-a");
        runtime
            .enable_sqlite(
                ControllerSqliteEnableRequest {
                    space_id: "root".to_string(),
                    binding_id: "default".to_string(),
                    db_path,
                    ..ControllerSqliteEnableRequest::default()
                },
                Some("client-a"),
            )
            .expect("sqlite backend should enable");

        let anonymous_error = runtime
            .execute_sqlite_script_typed("root", "default", None, "SELECT 1;", &[])
            .expect_err("anonymous caller should be rejected");
        assert!(
            anonymous_error
                .to_string()
                .contains("requires one attached client_id"),
            "anonymous error should mention attached client, got: {}",
            anonymous_error
        );

        let foreign_error = runtime
            .execute_sqlite_script_typed("root", "default", Some("client-b"), "SELECT 1;", &[])
            .expect_err("unattached client should be rejected");
        assert!(
            foreign_error
                .to_string()
                .contains("is not attached to space"),
            "foreign client error should mention space attachment, got: {}",
            foreign_error
        );
    }

    #[test]
    fn runtime_reaps_only_completed_idle_streams() {
        let runtime = runtime();
        let db_path = unique_temp_path("sqlite-stale-stream");
        runtime
            .attach_space(
                SpaceRegistration {
                    space_id: "root".to_string(),
                    space_label: "ROOT".to_string(),
                    space_kind: SpaceKind::Root,
                    space_root: "D:/runtime/root".to_string(),
                },
                None,
            )
            .expect("space should attach");
        runtime
            .enable_sqlite(
                ControllerSqliteEnableRequest {
                    space_id: "root".to_string(),
                    binding_id: "default".to_string(),
                    db_path,
                    ..ControllerSqliteEnableRequest::default()
                },
                None,
            )
            .expect("sqlite backend should enable");

        let stale_stream = runtime
            .open_sqlite_query_stream_typed("root", "default", None, "SELECT 1;", &[], None)
            .expect("stale stream should open");
        runtime
            .wait_sqlite_query_stream_metrics(stale_stream.stream_id, None)
            .expect("stale stream metrics should succeed");
        let immediate_removed = runtime
            .reap_stale_query_streams()
            .expect("freshly completed stream should not be reaped immediately");
        assert_eq!(immediate_removed, 0);

        let fresh_stream = runtime
            .open_sqlite_query_stream_typed("root", "default", None, "SELECT 1;", &[], None)
            .expect("fresh stream should open");

        {
            let mut state = runtime.lock_state().expect("runtime state should lock");
            state
                .sqlite_query_streams
                .get_mut(&stale_stream.stream_id)
                .expect("stale stream entry should exist")
                .last_accessed_at_unix_ms = 0;
            state
                .sqlite_query_streams
                .get(&stale_stream.stream_id)
                .expect("stale stream entry should exist")
                .handle
                .set_completed_at_unix_ms_for_test(Some(0));
        }

        let removed = runtime
            .reap_stale_query_streams()
            .expect("stale stream reap should succeed");
        assert_eq!(removed, 1);
        assert!(
            runtime
                .read_sqlite_query_stream_chunk(stale_stream.stream_id, 0, None)
                .is_err(),
            "stale completed stream should be removed"
        );
        assert!(
            runtime
                .read_sqlite_query_stream_chunk(fresh_stream.stream_id, 0, None)
                .is_ok(),
            "recently created stream should remain available"
        );
    }

    #[test]
    fn managed_runtime_requires_idle_window_before_shutdown() {
        let runtime = VldbControllerRuntime::new(ControllerServerConfig {
            bind_addr: "127.0.0.1:19801".to_string(),
            runtime: ControllerRuntimeConfig {
                minimum_uptime_secs: 0,
                idle_timeout_secs: 0,
                default_lease_ttl_secs: 60,
                ..ControllerRuntimeConfig::default()
            },
        });

        runtime
            .begin_request(None)
            .expect("request guard should begin successfully");
        assert!(
            runtime
                .should_shutdown()
                .expect("shutdown check should succeed")
        );
    }

    #[test]
    fn managed_runtime_keeps_running_when_anonymous_space_has_enabled_backend() {
        let runtime = VldbControllerRuntime::new(ControllerServerConfig {
            bind_addr: "127.0.0.1:19801".to_string(),
            runtime: ControllerRuntimeConfig {
                minimum_uptime_secs: 0,
                idle_timeout_secs: 0,
                default_lease_ttl_secs: 60,
                ..ControllerRuntimeConfig::default()
            },
        });

        runtime
            .attach_space(
                SpaceRegistration {
                    space_id: "root".to_string(),
                    space_label: "ROOT".to_string(),
                    space_kind: SpaceKind::Root,
                    space_root: "D:/runtime/root".to_string(),
                },
                None,
            )
            .expect("space should attach");

        let request = ControllerSqliteEnableRequest {
            space_id: "root".to_string(),
            db_path: unique_temp_path("sqlite-managed-anonymous"),
            ..Default::default()
        };
        runtime
            .enable_sqlite(request, None)
            .expect("sqlite backend should enable");

        assert!(
            !runtime
                .should_shutdown()
                .expect("shutdown check should succeed")
        );
    }
}

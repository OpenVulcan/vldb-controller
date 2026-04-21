use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use crate::core::lancedb::ControllerLanceDbBackend;
use crate::core::sqlite::ControllerSqliteBackend;
use vldb_controller_client::types::{
    ClientLeaseSnapshot, ClientRegistration, ControllerLanceDbCreateTableResult,
    ControllerLanceDbDeleteResult, ControllerLanceDbDropTableResult,
    ControllerLanceDbEnableRequest, ControllerLanceDbSearchResult, ControllerLanceDbUpsertResult,
    ControllerServerConfig, ControllerSqliteDictionaryMutationResult,
    ControllerSqliteEnableRequest, ControllerSqliteEnsureFtsIndexResult,
    ControllerSqliteExecuteBatchResult, ControllerSqliteExecuteResult,
    ControllerSqliteFtsMutationResult, ControllerSqliteListCustomWordsResult,
    ControllerSqliteQueryResult, ControllerSqliteQueryStreamResult,
    ControllerSqliteRebuildFtsIndexResult, ControllerSqliteSearchFtsResult,
    ControllerSqliteTokenizeResult, ControllerSqliteTokenizerMode, ControllerSqliteValue,
    ControllerStatusSnapshot, SpaceBackendStatus, SpaceRegistration, SpaceSnapshot,
};

/// Shared error type used by the controller core.
/// 控制器核心复用的共享错误类型。
pub type BoxError = Box<dyn Error + Send + Sync + 'static>;

/// Space-aware runtime registry, lease tracker, and shutdown policy manager.
/// 面向空间的运行时注册表、租约跟踪器与关闭策略管理器。
pub struct VldbControllerRuntime {
    server_config: ControllerServerConfig,
    started_at: Instant,
    started_at_unix_ms: u64,
    inflight_requests: Arc<AtomicUsize>,
    state: Mutex<ControllerState>,
}

/// RAII request guard that tracks in-flight request counts for shutdown decisions.
/// 用于关闭判定的执行中请求计数 RAII 守卫。
pub struct ControllerRequestGuard {
    inflight_requests: Arc<AtomicUsize>,
}

/// In-memory controller state shared across requests.
/// 在请求之间共享的控制器内存状态。
struct ControllerState {
    last_request_at: Instant,
    last_request_unix_ms: u64,
    clients: BTreeMap<String, ClientState>,
    spaces: BTreeMap<String, SpaceState>,
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
    sqlite: Option<ControllerSqliteBackend>,
    lancedb: Option<ControllerLanceDbBackend>,
}

impl Drop for ControllerRequestGuard {
    /// Release one in-flight request slot when the request scope ends.
    /// 在请求作用域结束时释放一个执行中请求槽位。
    fn drop(&mut self) {
        self.inflight_requests.fetch_sub(1, Ordering::SeqCst);
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
            inflight_requests: Arc::new(AtomicUsize::new(0)),
            state: Mutex::new(ControllerState {
                last_request_at: Instant::now(),
                last_request_unix_ms: now_unix_ms,
                clients: BTreeMap::new(),
                spaces: BTreeMap::new(),
            }),
        }
    }

    /// Mark the beginning of one incoming request and optionally refresh the caller lease.
    /// 标记一条传入请求的开始，并可选刷新调用方租约。
    pub fn begin_request(
        &self,
        client_id: Option<&str>,
    ) -> Result<ControllerRequestGuard, BoxError> {
        self.inflight_requests.fetch_add(1, Ordering::SeqCst);
        let guard = ControllerRequestGuard {
            inflight_requests: Arc::clone(&self.inflight_requests),
        };

        let now_unix_ms = unix_time_now_ms();
        let mut state = self
            .state
            .lock()
            .map_err(|_| invalid_input("controller runtime state lock poisoned"))?;
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

        Ok(guard)
    }

    /// Register one host client lease used for controller ownership and diagnostics.
    /// 注册一个供控制器 ownership 与诊断使用的宿主客户端租约。
    pub fn register_client(
        &self,
        registration: ClientRegistration,
    ) -> Result<ClientLeaseSnapshot, BoxError> {
        validate_client_registration(&registration)?;
        let now_unix_ms = unix_time_now_ms();
        let mut state = self
            .state
            .lock()
            .map_err(|_| invalid_input("controller runtime state lock poisoned"))?;
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
        let mut state = self
            .state
            .lock()
            .map_err(|_| invalid_input("controller runtime state lock poisoned"))?;
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
        let mut state = self
            .state
            .lock()
            .map_err(|_| invalid_input("controller runtime state lock poisoned"))?;
        self.prune_expired_clients_locked(&mut state, now_unix_ms);
        Ok(self.remove_client_locked(&mut state, client_id))
    }

    /// Return snapshots for all currently active clients.
    /// 返回所有当前活跃客户端的快照。
    pub fn list_clients(&self) -> Result<Vec<ClientLeaseSnapshot>, BoxError> {
        let now_unix_ms = unix_time_now_ms();
        let mut state = self
            .state
            .lock()
            .map_err(|_| invalid_input("controller runtime state lock poisoned"))?;
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
        let mut state = self
            .state
            .lock()
            .map_err(|_| invalid_input("controller runtime state lock poisoned"))?;
        self.prune_expired_clients_locked(&mut state, now_unix_ms);
        if let Some(client_id) = client_id.filter(|value| !value.trim().is_empty()) {
            if !state.clients.contains_key(client_id) {
                return Err(invalid_input(format!(
                    "client `{client_id}` is not registered"
                )));
            }
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
                    sqlite: None,
                    lancedb: None,
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
        let mut state = self
            .state
            .lock()
            .map_err(|_| invalid_input("controller runtime state lock poisoned"))?;
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
                && space.sqlite.is_none()
                && space.lancedb.is_none();
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
    ) -> Result<SpaceBackendStatus, BoxError> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| invalid_input("controller runtime state lock poisoned"))?;
        let space = state.spaces.get_mut(&request.space_id).ok_or_else(|| {
            invalid_input(format!("space `{}` is not attached", request.space_id))
        })?;
        let backend = ControllerSqliteBackend::new(&request)?;
        let status = SpaceBackendStatus {
            enabled: true,
            mode: "dynamic_library".to_string(),
            target: backend.db_path().to_string(),
        };
        space.sqlite = Some(backend);
        Ok(status)
    }

    /// Disable the SQLite backend for the target runtime space.
    /// 为目标运行时空间关闭 SQLite 后端。
    pub fn disable_sqlite(&self, space_id: &str) -> Result<bool, BoxError> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| invalid_input("controller runtime state lock poisoned"))?;
        let space = state
            .spaces
            .get_mut(space_id)
            .ok_or_else(|| invalid_input(format!("space `{space_id}` is not attached")))?;
        Ok(space.sqlite.take().is_some())
    }

    /// Execute one SQLite script against the backend bound to the target runtime space using typed parameters.
    /// 使用类型化参数针对绑定到目标运行时空间的后端执行一条 SQLite 脚本。
    pub fn execute_sqlite_script_typed(
        &self,
        space_id: &str,
        sql: &str,
        params: &[ControllerSqliteValue],
    ) -> Result<ControllerSqliteExecuteResult, BoxError> {
        let backend = {
            let state = self
                .state
                .lock()
                .map_err(|_| invalid_input("controller runtime state lock poisoned"))?;
            let space = state
                .spaces
                .get(space_id)
                .ok_or_else(|| invalid_input(format!("space `{space_id}` is not attached")))?;
            space.sqlite.clone().ok_or_else(|| {
                invalid_input(format!(
                    "sqlite backend is not enabled for space `{space_id}`"
                ))
            })?
        };

        backend.execute_script_typed(sql, params)
    }

    /// Execute one SQLite JSON query against the backend bound to the target runtime space using typed parameters.
    /// 使用类型化参数针对绑定到目标运行时空间的后端执行一条 SQLite JSON 查询。
    pub fn query_sqlite_json_typed(
        &self,
        space_id: &str,
        sql: &str,
        params: &[ControllerSqliteValue],
    ) -> Result<ControllerSqliteQueryResult, BoxError> {
        let backend = {
            let state = self
                .state
                .lock()
                .map_err(|_| invalid_input("controller runtime state lock poisoned"))?;
            let space = state
                .spaces
                .get(space_id)
                .ok_or_else(|| invalid_input(format!("space `{space_id}` is not attached")))?;
            space.sqlite.clone().ok_or_else(|| {
                invalid_input(format!(
                    "sqlite backend is not enabled for space `{space_id}`"
                ))
            })?
        };

        backend.query_json_typed(sql, params)
    }

    /// Execute one SQLite batch against the backend bound to the target runtime space using typed parameter groups.
    /// 使用类型化参数组针对绑定到目标运行时空间的后端执行一组 SQLite 批量语句。
    pub fn execute_sqlite_batch_typed(
        &self,
        space_id: &str,
        sql: &str,
        batch_params: &[Vec<ControllerSqliteValue>],
    ) -> Result<ControllerSqliteExecuteBatchResult, BoxError> {
        let backend = {
            let state = self
                .state
                .lock()
                .map_err(|_| invalid_input("controller runtime state lock poisoned"))?;
            let space = state
                .spaces
                .get(space_id)
                .ok_or_else(|| invalid_input(format!("space `{space_id}` is not attached")))?;
            space.sqlite.clone().ok_or_else(|| {
                invalid_input(format!(
                    "sqlite backend is not enabled for space `{space_id}`"
                ))
            })?
        };

        backend.execute_batch_typed(sql, batch_params)
    }

    /// Execute one SQLite streaming query against the backend bound to the target runtime space using typed parameters.
    /// 使用类型化参数针对绑定到目标运行时空间的后端执行一条 SQLite 流式查询。
    pub fn query_sqlite_stream_typed(
        &self,
        space_id: &str,
        sql: &str,
        params: &[ControllerSqliteValue],
        target_chunk_size: Option<usize>,
    ) -> Result<ControllerSqliteQueryStreamResult, BoxError> {
        let backend = {
            let state = self
                .state
                .lock()
                .map_err(|_| invalid_input("controller runtime state lock poisoned"))?;
            let space = state
                .spaces
                .get(space_id)
                .ok_or_else(|| invalid_input(format!("space `{space_id}` is not attached")))?;
            space.sqlite.clone().ok_or_else(|| {
                invalid_input(format!(
                    "sqlite backend is not enabled for space `{space_id}`"
                ))
            })?
        };

        backend.query_stream_typed(sql, params, target_chunk_size)
    }

    /// Tokenize text using the SQLite backend bound to the target runtime space.
    /// 使用绑定到目标运行时空间的 SQLite 后端对文本执行分词。
    pub fn tokenize_sqlite_text(
        &self,
        space_id: &str,
        tokenizer_mode: ControllerSqliteTokenizerMode,
        text: &str,
        search_mode: bool,
    ) -> Result<ControllerSqliteTokenizeResult, BoxError> {
        let backend = {
            let state = self
                .state
                .lock()
                .map_err(|_| invalid_input("controller runtime state lock poisoned"))?;
            let space = state
                .spaces
                .get(space_id)
                .ok_or_else(|| invalid_input(format!("space `{space_id}` is not attached")))?;
            space.sqlite.clone().ok_or_else(|| {
                invalid_input(format!(
                    "sqlite backend is not enabled for space `{space_id}`"
                ))
            })?
        };

        backend.tokenize_text(tokenizer_mode, text, search_mode)
    }

    /// List custom dictionary words from the SQLite backend bound to the target runtime space.
    /// 从绑定到目标运行时空间的 SQLite 后端列出自定义词条目。
    pub fn list_sqlite_custom_words(
        &self,
        space_id: &str,
    ) -> Result<ControllerSqliteListCustomWordsResult, BoxError> {
        let backend = {
            let state = self
                .state
                .lock()
                .map_err(|_| invalid_input("controller runtime state lock poisoned"))?;
            let space = state
                .spaces
                .get(space_id)
                .ok_or_else(|| invalid_input(format!("space `{space_id}` is not attached")))?;
            space.sqlite.clone().ok_or_else(|| {
                invalid_input(format!(
                    "sqlite backend is not enabled for space `{space_id}`"
                ))
            })?
        };

        backend.list_custom_words()
    }

    /// Insert or update one SQLite custom dictionary word through the bound backend.
    /// 通过绑定后端插入或更新一条 SQLite 自定义词。
    pub fn upsert_sqlite_custom_word(
        &self,
        space_id: &str,
        word: &str,
        weight: usize,
    ) -> Result<ControllerSqliteDictionaryMutationResult, BoxError> {
        let backend = {
            let state = self
                .state
                .lock()
                .map_err(|_| invalid_input("controller runtime state lock poisoned"))?;
            let space = state
                .spaces
                .get(space_id)
                .ok_or_else(|| invalid_input(format!("space `{space_id}` is not attached")))?;
            space.sqlite.clone().ok_or_else(|| {
                invalid_input(format!(
                    "sqlite backend is not enabled for space `{space_id}`"
                ))
            })?
        };

        backend.upsert_custom_word(word, weight)
    }

    /// Remove one SQLite custom dictionary word through the bound backend.
    /// 通过绑定后端删除一条 SQLite 自定义词。
    pub fn remove_sqlite_custom_word(
        &self,
        space_id: &str,
        word: &str,
    ) -> Result<ControllerSqliteDictionaryMutationResult, BoxError> {
        let backend = {
            let state = self
                .state
                .lock()
                .map_err(|_| invalid_input("controller runtime state lock poisoned"))?;
            let space = state
                .spaces
                .get(space_id)
                .ok_or_else(|| invalid_input(format!("space `{space_id}` is not attached")))?;
            space.sqlite.clone().ok_or_else(|| {
                invalid_input(format!(
                    "sqlite backend is not enabled for space `{space_id}`"
                ))
            })?
        };

        backend.remove_custom_word(word)
    }

    /// Ensure one SQLite FTS index through the backend bound to the target runtime space.
    /// 通过绑定到目标运行时空间的后端确认一条 SQLite FTS 索引。
    pub fn ensure_sqlite_fts_index(
        &self,
        space_id: &str,
        index_name: &str,
        tokenizer_mode: ControllerSqliteTokenizerMode,
    ) -> Result<ControllerSqliteEnsureFtsIndexResult, BoxError> {
        let backend = {
            let state = self
                .state
                .lock()
                .map_err(|_| invalid_input("controller runtime state lock poisoned"))?;
            let space = state
                .spaces
                .get(space_id)
                .ok_or_else(|| invalid_input(format!("space `{space_id}` is not attached")))?;
            space.sqlite.clone().ok_or_else(|| {
                invalid_input(format!(
                    "sqlite backend is not enabled for space `{space_id}`"
                ))
            })?
        };

        backend.ensure_fts_index(index_name, tokenizer_mode)
    }

    /// Rebuild one SQLite FTS index through the backend bound to the target runtime space.
    /// 通过绑定到目标运行时空间的后端重建一条 SQLite FTS 索引。
    pub fn rebuild_sqlite_fts_index(
        &self,
        space_id: &str,
        index_name: &str,
        tokenizer_mode: ControllerSqliteTokenizerMode,
    ) -> Result<ControllerSqliteRebuildFtsIndexResult, BoxError> {
        let backend = {
            let state = self
                .state
                .lock()
                .map_err(|_| invalid_input("controller runtime state lock poisoned"))?;
            let space = state
                .spaces
                .get(space_id)
                .ok_or_else(|| invalid_input(format!("space `{space_id}` is not attached")))?;
            space.sqlite.clone().ok_or_else(|| {
                invalid_input(format!(
                    "sqlite backend is not enabled for space `{space_id}`"
                ))
            })?
        };

        backend.rebuild_fts_index(index_name, tokenizer_mode)
    }

    /// Upsert one SQLite FTS document through the backend bound to the target runtime space.
    /// 通过绑定到目标运行时空间的后端写入一条 SQLite FTS 文档。
    pub fn upsert_sqlite_fts_document(
        &self,
        space_id: &str,
        index_name: &str,
        tokenizer_mode: ControllerSqliteTokenizerMode,
        id: &str,
        file_path: &str,
        title: &str,
        content: &str,
    ) -> Result<ControllerSqliteFtsMutationResult, BoxError> {
        let backend = {
            let state = self
                .state
                .lock()
                .map_err(|_| invalid_input("controller runtime state lock poisoned"))?;
            let space = state
                .spaces
                .get(space_id)
                .ok_or_else(|| invalid_input(format!("space `{space_id}` is not attached")))?;
            space.sqlite.clone().ok_or_else(|| {
                invalid_input(format!(
                    "sqlite backend is not enabled for space `{space_id}`"
                ))
            })?
        };

        backend.upsert_fts_document(index_name, tokenizer_mode, id, file_path, title, content)
    }

    /// Delete one SQLite FTS document through the backend bound to the target runtime space.
    /// 通过绑定到目标运行时空间的后端删除一条 SQLite FTS 文档。
    pub fn delete_sqlite_fts_document(
        &self,
        space_id: &str,
        index_name: &str,
        id: &str,
    ) -> Result<ControllerSqliteFtsMutationResult, BoxError> {
        let backend = {
            let state = self
                .state
                .lock()
                .map_err(|_| invalid_input("controller runtime state lock poisoned"))?;
            let space = state
                .spaces
                .get(space_id)
                .ok_or_else(|| invalid_input(format!("space `{space_id}` is not attached")))?;
            space.sqlite.clone().ok_or_else(|| {
                invalid_input(format!(
                    "sqlite backend is not enabled for space `{space_id}`"
                ))
            })?
        };

        backend.delete_fts_document(index_name, id)
    }

    /// Search one SQLite FTS index through the backend bound to the target runtime space.
    /// 通过绑定到目标运行时空间的后端检索一条 SQLite FTS 索引。
    pub fn search_sqlite_fts(
        &self,
        space_id: &str,
        index_name: &str,
        tokenizer_mode: ControllerSqliteTokenizerMode,
        query: &str,
        limit: u32,
        offset: u32,
    ) -> Result<ControllerSqliteSearchFtsResult, BoxError> {
        let backend = {
            let state = self
                .state
                .lock()
                .map_err(|_| invalid_input("controller runtime state lock poisoned"))?;
            let space = state
                .spaces
                .get(space_id)
                .ok_or_else(|| invalid_input(format!("space `{space_id}` is not attached")))?;
            space.sqlite.clone().ok_or_else(|| {
                invalid_input(format!(
                    "sqlite backend is not enabled for space `{space_id}`"
                ))
            })?
        };

        backend.search_fts(index_name, tokenizer_mode, query, limit, offset)
    }

    /// Enable one LanceDB backend for the target runtime space.
    /// 为目标运行时空间启用一个 LanceDB 后端。
    pub fn enable_lancedb(
        &self,
        request: ControllerLanceDbEnableRequest,
    ) -> Result<SpaceBackendStatus, BoxError> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| invalid_input("controller runtime state lock poisoned"))?;
        let space = state.spaces.get_mut(&request.space_id).ok_or_else(|| {
            invalid_input(format!("space `{}` is not attached", request.space_id))
        })?;
        let backend = ControllerLanceDbBackend::new(&request)?;
        let status = SpaceBackendStatus {
            enabled: true,
            mode: "dynamic_library".to_string(),
            target: backend.default_db_path().to_string(),
        };
        space.lancedb = Some(backend);
        Ok(status)
    }

    /// Disable the LanceDB backend for the target runtime space.
    /// 为目标运行时空间关闭 LanceDB 后端。
    pub fn disable_lancedb(&self, space_id: &str) -> Result<bool, BoxError> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| invalid_input("controller runtime state lock poisoned"))?;
        let space = state
            .spaces
            .get_mut(space_id)
            .ok_or_else(|| invalid_input(format!("space `{space_id}` is not attached")))?;
        Ok(space.lancedb.take().is_some())
    }

    /// Create one LanceDB table against the backend bound to the target runtime space.
    /// 针对绑定到目标运行时空间的后端创建一张 LanceDB 表。
    pub async fn create_lancedb_table(
        &self,
        space_id: &str,
        request_json: &str,
    ) -> Result<ControllerLanceDbCreateTableResult, BoxError> {
        let backend = {
            let state = self
                .state
                .lock()
                .map_err(|_| invalid_input("controller runtime state lock poisoned"))?;
            let space = state
                .spaces
                .get(space_id)
                .ok_or_else(|| invalid_input(format!("space `{space_id}` is not attached")))?;
            space.lancedb.clone().ok_or_else(|| {
                invalid_input(format!(
                    "lancedb backend is not enabled for space `{space_id}`"
                ))
            })?
        };

        backend.create_table(request_json).await
    }

    /// Upsert LanceDB rows against the backend bound to the target runtime space.
    /// 针对绑定到目标运行时空间的后端写入 LanceDB 行数据。
    pub async fn upsert_lancedb(
        &self,
        space_id: &str,
        request_json: &str,
        data: Vec<u8>,
    ) -> Result<ControllerLanceDbUpsertResult, BoxError> {
        let backend = {
            let state = self
                .state
                .lock()
                .map_err(|_| invalid_input("controller runtime state lock poisoned"))?;
            let space = state
                .spaces
                .get(space_id)
                .ok_or_else(|| invalid_input(format!("space `{space_id}` is not attached")))?;
            space.lancedb.clone().ok_or_else(|| {
                invalid_input(format!(
                    "lancedb backend is not enabled for space `{space_id}`"
                ))
            })?
        };

        backend.upsert(request_json, data).await
    }

    /// Search LanceDB rows against the backend bound to the target runtime space.
    /// 针对绑定到目标运行时空间的后端检索 LanceDB 行数据。
    pub async fn search_lancedb(
        &self,
        space_id: &str,
        request_json: &str,
    ) -> Result<ControllerLanceDbSearchResult, BoxError> {
        let backend = {
            let state = self
                .state
                .lock()
                .map_err(|_| invalid_input("controller runtime state lock poisoned"))?;
            let space = state
                .spaces
                .get(space_id)
                .ok_or_else(|| invalid_input(format!("space `{space_id}` is not attached")))?;
            space.lancedb.clone().ok_or_else(|| {
                invalid_input(format!(
                    "lancedb backend is not enabled for space `{space_id}`"
                ))
            })?
        };

        backend.search(request_json).await
    }

    /// Delete LanceDB rows against the backend bound to the target runtime space.
    /// 针对绑定到目标运行时空间的后端删除 LanceDB 行数据。
    pub async fn delete_lancedb(
        &self,
        space_id: &str,
        request_json: &str,
    ) -> Result<ControllerLanceDbDeleteResult, BoxError> {
        let backend = {
            let state = self
                .state
                .lock()
                .map_err(|_| invalid_input("controller runtime state lock poisoned"))?;
            let space = state
                .spaces
                .get(space_id)
                .ok_or_else(|| invalid_input(format!("space `{space_id}` is not attached")))?;
            space.lancedb.clone().ok_or_else(|| {
                invalid_input(format!(
                    "lancedb backend is not enabled for space `{space_id}`"
                ))
            })?
        };

        backend.delete(request_json).await
    }

    /// Drop one LanceDB table against the backend bound to the target runtime space.
    /// 针对绑定到目标运行时空间的后端删除一张 LanceDB 表。
    pub async fn drop_lancedb_table(
        &self,
        space_id: &str,
        table_name: &str,
    ) -> Result<ControllerLanceDbDropTableResult, BoxError> {
        let backend = {
            let state = self
                .state
                .lock()
                .map_err(|_| invalid_input("controller runtime state lock poisoned"))?;
            let space = state
                .spaces
                .get(space_id)
                .ok_or_else(|| invalid_input(format!("space `{space_id}` is not attached")))?;
            space.lancedb.clone().ok_or_else(|| {
                invalid_input(format!(
                    "lancedb backend is not enabled for space `{space_id}`"
                ))
            })?
        };

        backend.drop_table(table_name).await
    }

    /// Return snapshots for all currently attached spaces.
    /// 返回所有当前已附着空间的快照。
    pub fn list_spaces(&self) -> Result<Vec<SpaceSnapshot>, BoxError> {
        let now_unix_ms = unix_time_now_ms();
        let mut state = self
            .state
            .lock()
            .map_err(|_| invalid_input("controller runtime state lock poisoned"))?;
        self.prune_expired_clients_locked(&mut state, now_unix_ms);
        Ok(state.spaces.values().map(snapshot_space_state).collect())
    }

    /// Return one high-level controller status snapshot.
    /// 返回一条高层控制器状态快照。
    pub fn status_snapshot(&self) -> Result<ControllerStatusSnapshot, BoxError> {
        let now_unix_ms = unix_time_now_ms();
        let mut state = self
            .state
            .lock()
            .map_err(|_| invalid_input("controller runtime state lock poisoned"))?;
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
            inflight_requests: self.inflight_requests.load(Ordering::SeqCst),
            shutdown_candidate: self.should_shutdown_locked(&state),
        })
    }

    /// Determine whether the managed controller currently qualifies for self-shutdown.
    /// 判断当前托管控制器是否已满足自停条件。
    pub fn should_shutdown(&self) -> Result<bool, BoxError> {
        let now_unix_ms = unix_time_now_ms();
        let mut state = self
            .state
            .lock()
            .map_err(|_| invalid_input("controller runtime state lock poisoned"))?;
        self.prune_expired_clients_locked(&mut state, now_unix_ms);
        Ok(self.should_shutdown_locked(&state))
    }

    /// Remove expired clients, release their space attachments, and return how many were reaped.
    /// 清理过期客户端、释放其空间附着，并返回被回收的数量。
    pub fn reap_expired_clients(&self) -> Result<usize, BoxError> {
        let now_unix_ms = unix_time_now_ms();
        let mut state = self
            .state
            .lock()
            .map_err(|_| invalid_input("controller runtime state lock poisoned"))?;
        Ok(self.prune_expired_clients_locked(&mut state, now_unix_ms))
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

        if self.inflight_requests.load(Ordering::SeqCst) > 0 {
            return false;
        }

        if state.spaces.values().any(|space| {
            !space.attached_clients.is_empty() || space.sqlite.is_some() || space.lancedb.is_some()
        }) {
            return false;
        }

        let idle_timeout = Duration::from_secs(self.server_config.runtime.idle_timeout_secs);
        state.last_request_at.elapsed() >= idle_timeout
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

        let empty_space_ids = state
            .spaces
            .iter()
            .filter_map(|(space_id, space)| {
                (space.attached_clients.is_empty()
                    && space.sqlite.is_none()
                    && space.lancedb.is_none())
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
        sqlite: space.sqlite.as_ref().map(|backend| SpaceBackendStatus {
            enabled: true,
            mode: "dynamic_library".to_string(),
            target: backend.db_path().to_string(),
        }),
        lancedb: space.lancedb.as_ref().map(|backend| SpaceBackendStatus {
            enabled: true,
            mode: "dynamic_library".to_string(),
            target: backend.default_db_path().to_string(),
        }),
    }
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
    Box::new(std::io::Error::new(
        std::io::ErrorKind::InvalidInput,
        message.into(),
    ))
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

        let mut request = ControllerSqliteEnableRequest::default();
        request.space_id = "root".to_string();
        request.db_path = unique_temp_path("sqlite");
        let status = runtime
            .enable_sqlite(request.clone())
            .expect("sqlite backend should enable");
        assert_eq!(status.target, request.db_path);
        assert!(
            runtime
                .disable_sqlite("root")
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

        let mut request = ControllerSqliteEnableRequest::default();
        request.space_id = "root".to_string();
        request.db_path = unique_temp_path("sqlite-data-plane");
        runtime
            .enable_sqlite(request)
            .expect("sqlite backend should enable");

        runtime
            .execute_sqlite_script_typed(
                "root",
                "CREATE TABLE IF NOT EXISTS items(id INTEGER PRIMARY KEY, name TEXT NOT NULL);",
                &[],
            )
            .expect("table creation should succeed");

        let insert_result = runtime
            .execute_sqlite_script_typed(
                "root",
                "INSERT INTO items(name) VALUES (?1);",
                &[ControllerSqliteValue::String("alpha".to_string())],
            )
            .expect("insert should succeed");
        assert!(insert_result.success);
        assert_eq!(insert_result.rows_changed, 1);

        let query_result = runtime
            .query_sqlite_json_typed("root", "SELECT name FROM items ORDER BY id ASC;", &[])
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

        let mut request = ControllerSqliteEnableRequest::default();
        request.space_id = "root".to_string();
        request.db_path = unique_temp_path("sqlite-batch-stream");
        runtime
            .enable_sqlite(request)
            .expect("sqlite backend should enable");

        runtime
            .execute_sqlite_script_typed(
                "root",
                "CREATE TABLE IF NOT EXISTS items(id INTEGER PRIMARY KEY, name TEXT NOT NULL);",
                &[],
            )
            .expect("table creation should succeed");

        let batch_result = runtime
            .execute_sqlite_batch_typed(
                "root",
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

        let stream_result = runtime
            .query_sqlite_stream_typed(
                "root",
                "SELECT id, name FROM items ORDER BY id ASC;",
                &[],
                Some(64 * 1024),
            )
            .expect("query_stream should succeed");
        assert_eq!(stream_result.row_count, 2);
        assert!(stream_result.chunk_count >= 1);
        assert!(!stream_result.chunks.is_empty());
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

        let mut request = ControllerSqliteEnableRequest::default();
        request.space_id = "root".to_string();
        request.db_path = unique_temp_path("sqlite-tokenizer");
        runtime
            .enable_sqlite(request)
            .expect("sqlite backend should enable");

        let before = runtime
            .tokenize_sqlite_text(
                "root",
                ControllerSqliteTokenizerMode::Jieba,
                "市民田-女士急匆匆",
                false,
            )
            .expect("tokenize before custom word should succeed");
        assert!(!before.tokens.iter().any(|value| value == "田-女士"));

        let upsert = runtime
            .upsert_sqlite_custom_word("root", "田-女士", 42)
            .expect("upsert custom word should succeed");
        assert!(upsert.success);

        let listed = runtime
            .list_sqlite_custom_words("root")
            .expect("list custom words should succeed");
        assert_eq!(listed.words.len(), 1);
        assert_eq!(listed.words[0].word, "田-女士");

        let after = runtime
            .tokenize_sqlite_text(
                "root",
                ControllerSqliteTokenizerMode::Jieba,
                "市民田-女士急匆匆",
                false,
            )
            .expect("tokenize after custom word should succeed");
        assert!(after.tokens.iter().any(|value| value == "田-女士"));

        let removed = runtime
            .remove_sqlite_custom_word("root", "田-女士")
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

        let mut request = ControllerSqliteEnableRequest::default();
        request.space_id = "root".to_string();
        request.db_path = unique_temp_path("sqlite-fts");
        runtime
            .enable_sqlite(request)
            .expect("sqlite backend should enable");

        runtime
            .ensure_sqlite_fts_index("root", "memory_docs", ControllerSqliteTokenizerMode::None)
            .expect("ensure fts index should succeed");
        let upsert = runtime
            .upsert_sqlite_fts_document(
                "root",
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
            .rebuild_sqlite_fts_index("root", "memory_docs", ControllerSqliteTokenizerMode::None)
            .expect("rebuild fts index should succeed");
        assert!(rebuild.success);
        assert_eq!(rebuild.reindexed_rows, 1);

        let deleted = runtime
            .delete_sqlite_fts_document("root", "memory_docs", "doc-1")
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

        let mut request = ControllerLanceDbEnableRequest::default();
        request.space_id = "project-a".to_string();
        request.default_db_path = unique_temp_path("lancedb-default");
        request.db_root = Some(unique_temp_path("lancedb-root"));
        request.read_consistency_interval_ms = Some(0);
        let status = runtime
            .enable_lancedb(request.clone())
            .expect("lancedb backend should enable");
        assert_eq!(status.target, request.default_db_path);
        assert!(
            runtime
                .disable_lancedb("project-a")
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

        let mut request = ControllerLanceDbEnableRequest::default();
        request.space_id = "project-a".to_string();
        request.default_db_path = unique_temp_path("lancedb-data-plane-default");
        request.db_root = Some(unique_temp_path("lancedb-data-plane-root"));
        runtime
            .enable_lancedb(request)
            .expect("lancedb backend should enable");

        runtime
            .create_lancedb_table(
                "project-a",
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
                r#"{
                  "table_name":"memory",
                  "condition":"id = 'row-1'"
                }"#,
            )
            .await
            .expect("delete should succeed");
        assert_eq!(delete.deleted_rows, 1);

        let drop_result = runtime
            .drop_lancedb_table("project-a", "memory")
            .await
            .expect("drop_table should succeed");
        assert!(drop_result.message.contains("dropped"));
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

        let mut request = ControllerSqliteEnableRequest::default();
        request.space_id = "root".to_string();
        request.db_path = unique_temp_path("sqlite-managed-anonymous");
        runtime
            .enable_sqlite(request)
            .expect("sqlite backend should enable");

        assert!(
            !runtime
                .should_shutdown()
                .expect("shutdown check should succeed")
        );
    }
}

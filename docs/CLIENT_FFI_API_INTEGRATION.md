# vldb-controller Client / FFI 全量 API 对接说明

## 1. 文档范围

本文覆盖当前工作树里两套正式接入面：

- Rust SDK：`vldb-controller-client`
- FFI：`ffi/src/lib.rs` 导出的 `extern "C"` 接口

本文重点说明：

- 对接方式
- 参数结构
- 每个接口的作用
- 推荐执行顺序
- 内存释放与生命周期约束

## 2. 总体接入模型

### 2.1 Rust SDK 适用场景

适用于：

- Rust 宿主
- 需要显式控制 `binding_id`
- 需要 typed API 与 JSON 兼容包装两套能力
- 需要自动拉起 controller、自动续租、主动注销

### 2.2 FFI 适用场景

适用于：

- C / C++ / Go / Python / Node-API / Bun / Deno 等非 Rust 宿主
- 需要稳定 C ABI
- 更偏向单 binding、单 space 直连方式

### 2.3 两套接入面的核心差异

| 维度 | Rust SDK | FFI |
|------|----------|-----|
| 语言 | Rust | 任意支持 C ABI 的语言 |
| 句柄模型 | `ControllerClient` Rust 对象 | `FfiControllerClientHandle*` 不透明句柄 |
| `binding_id` | 调用方可显式传入 | 当前 FFI 统一使用 `binding_id = space_id` |
| 参数风格 | typed + JSON 兼容包装 | native struct + JSON 两套导出 |
| 内存释放 | Rust 自动管理 | 调用方必须显式调用 `*_free` |
| 自动续租 | `connect()` 后自动启动 | `client_connect` 后自动启动 |

## 3. 推荐执行顺序

### 3.1 通用生命周期顺序

无论是 Rust SDK 还是 FFI，推荐顺序都一致：

1. 构造客户端配置
2. 准备客户端注册信息
3. 创建 client / handle
4. `connect`
5. `attach_space`
6. `enable_sqlite` 或 `enable_lancedb`
7. 执行数据面接口
8. `disable_sqlite` / `disable_lancedb`（可选）
9. `detach_space`（可选）
10. `shutdown` 或 `client_free`

### 3.2 SQLite 推荐顺序

1. `attach_space`
2. `enable_sqlite`
3. 执行脚本建表
4. 执行写入 / 查询 / stream / FTS / 自定义词接口
5. `disable_sqlite`

### 3.3 LanceDB 推荐顺序

1. `attach_space`
2. `enable_lancedb`
3. `create_lancedb_table`
4. `upsert_lancedb`
5. `search_lancedb`
6. `delete_lancedb` 或 `drop_lancedb_table`
7. `disable_lancedb`

## 4. Rust SDK 对接说明

### 4.1 导出入口

`client/src/lib.rs` 当前对外导出：

- `ControllerClient`
- `ControllerClientConfig`
- 全部 `types.rs` 中的请求/响应/枚举/错误类型

建议接入方式：

```rust
use vldb_controller_client::{
    ClientRegistration,
    ControllerClient,
    ControllerClientConfig,
    ControllerLanceDbEnableRequest,
    ControllerSqliteEnableRequest,
    ControllerSqliteValue,
    SpaceKind,
    SpaceRegistration,
};
```

### 4.2 关键结构

#### 4.2.1 `ControllerClientConfig`

| 字段 | 类型 | 作用 |
|------|------|------|
| `endpoint` | `String` | controller 地址；支持 `http://host:port`、`https://host:port`、`host:port`、`port`、`:port` |
| `auto_spawn` | `bool` | controller 不可达时是否允许自动拉起 |
| `spawn_executable` | `Option<String>` | controller 可执行文件路径；空则回退 `PATH` 中的 `vldb-controller` |
| `spawn_process_mode` | `ControllerProcessMode` | 自动拉起时使用 `service` 或 `managed` |
| `minimum_uptime_secs` | `u64` | 自动拉起 controller 的最小存活时间 |
| `idle_timeout_secs` | `u64` | 自动拉起 controller 的空闲超时时间 |
| `default_lease_ttl_secs` | `u64` | 默认客户端租约 TTL |
| `connect_timeout_secs` | `u64` | 单次传输连接超时 |
| `startup_timeout_secs` | `u64` | 自动拉起后的总等待时长 |
| `startup_retry_interval_ms` | `u64` | 自动拉起等待期间的重试间隔 |
| `lease_renew_interval_secs` | `u64` | 后台续租间隔 |

辅助方法：

| 方法 | 返回 | 作用 |
|------|------|------|
| `endpoint_url()` | `Result<String, BoxError>` | 归一化给 tonic 使用的 endpoint URL |
| `bind_addr()` | `Result<String, BoxError>` | 归一化给 auto-spawn controller 使用的 bind 地址 |
| `spawn_executable()` | `&str` | 返回实际使用的可执行文件名/路径 |

说明：

- 这是当前最明确的编译期变更点：`endpoint_url()` / `bind_addr()` 都返回 `Result`
- `localhost:PORT` 会被转换成 `127.0.0.1:PORT` 用于 auto-spawn bind
- 非本地主机名可用于连接，但不能被自动转换成 bind 地址

#### 4.2.2 `ClientRegistration`

| 字段 | 类型 | 作用 |
|------|------|------|
| `client_id` | `String` | 宿主侧稳定 client 标识 |
| `host_kind` | `String` | 宿主类别，例如 `mcp` / `ide` / `service` |
| `process_id` | `u32` | 宿主进程号 |
| `process_name` | `String` | 宿主进程名 |
| `lease_ttl_secs` | `Option<u64>` | 自定义租约 TTL；空则使用 controller 默认值 |

#### 4.2.3 `SpaceRegistration`

| 字段 | 类型 | 作用 |
|------|------|------|
| `space_id` | `String` | 运行时空间标识，例如 `root`、`user`、项目 ID |
| `space_label` | `String` | 人类可读标签 |
| `space_kind` | `SpaceKind` | `Root` / `User` / `Project` |
| `space_root` | `String` | 空间根路径 |

#### 4.2.4 `ControllerSqliteEnableRequest`

| 字段 | 类型 | 作用 |
|------|------|------|
| `space_id` | `String` | 目标空间 |
| `binding_id` | `String` | SQLite binding 标识 |
| `db_path` | `String` | SQLite 文件路径 |
| `connection_pool_size` | `usize` | 连接池大小 |
| `busy_timeout_ms` | `u64` | busy timeout |
| `journal_mode` | `String` | journal 模式 |
| `synchronous` | `String` | synchronous 模式 |
| `foreign_keys` | `bool` | 是否启用外键 |
| `temp_store` | `String` | temp store 模式 |
| `wal_autocheckpoint_pages` | `u32` | WAL checkpoint 页数 |
| `cache_size_kib` | `i64` | 缓存大小 |
| `mmap_size_bytes` | `u64` | mmap 大小 |
| `enforce_db_file_lock` | `bool` | 是否强制 DB 文件锁 |
| `read_only` | `bool` | 是否只读 |
| `allow_uri_filenames` | `bool` | 是否允许 URI filename |
| `trusted_schema` | `bool` | 是否启用 trusted_schema |
| `defensive` | `bool` | 是否启用 SQLite defensive 模式 |

#### 4.2.5 `ControllerLanceDbEnableRequest`

| 字段 | 类型 | 作用 |
|------|------|------|
| `space_id` | `String` | 目标空间 |
| `binding_id` | `String` | LanceDB binding 标识 |
| `default_db_path` | `String` | 默认库路径 |
| `db_root` | `Option<String>` | 子库根目录 |
| `read_consistency_interval_ms` | `Option<u64>` | 读一致性间隔 |
| `max_upsert_payload` | `usize` | 单次 upsert 最大载荷 |
| `max_search_limit` | `usize` | 最大搜索条数 |
| `max_concurrent_requests` | `usize` | 最大并发请求数 |

#### 4.2.6 常用值与结果结构

| 类型 | 作用 |
|------|------|
| `ControllerSqliteValue` | typed SQLite 参数值：`Int64` / `Float64` / `String` / `Bytes` / `Bool` / `Null` |
| `ControllerSqliteQueryStreamOpenResult` | stream 打开结果，包含 `stream_id` |
| `ControllerSqliteQueryStreamMetrics` | stream 最终行数/块数/字节数 |
| `ControllerLanceDbColumnDef` | LanceDB 列定义 |
| `ControllerLanceDbInputFormat` | `Unspecified` / `JsonRows` / `ArrowIpc` |
| `ControllerLanceDbOutputFormat` | `Unspecified` / `ArrowIpc` / `JsonRows` |
| `VldbControllerError` | 当前统一错误类型 |

### 4.3 `ControllerClient` 接口总览

说明：

- 所有业务方法都会自动带上当前实例持有的 `client_id`
- `connect()` 后会自动注册 client 并启动后台续租
- `shutdown()` 会停止续租并显式注销

#### 4.3.1 构造与生命周期

| 接口 | 作用 | 关键参数 | 返回 | 调用时机 |
|------|------|----------|------|----------|
| `new(config, registration)` | 创建 client 对象，不发网络请求 | `ControllerClientConfig`、`ClientRegistration` | `ControllerClient` | 第一步 |
| `connect()` | 确保 controller 可达，必要时自动拉起，然后注册 client 并启动续租 | 无 | `Result<(), BoxError>` | 业务调用前 |
| `shutdown()` | 停止后台续租并注销 client | 无 | `Result<(), BoxError>` | 退出前 |

#### 4.3.2 控制面接口

| 接口 | 作用 | 关键参数 | 返回 | 前置条件 |
|------|------|----------|------|----------|
| `get_status()` | 读取 controller 当前状态 | 无 | `ControllerStatusSnapshot` | `connect()` 后推荐调用 |
| `list_clients()` | 列出 controller 当前客户端租约 | 无 | `Vec<ClientLeaseSnapshot>` | 已连接 |
| `attach_space(registration)` | 将当前 client 附着到某个 space | `SpaceRegistration` | `SpaceSnapshot` | 已连接 |
| `detach_space(space_id)` | 将当前 client 从某个 space 解绑 | `space_id` | `bool` | 已 attach |
| `list_spaces()` | 查看当前 controller 已知 space 快照 | 无 | `Vec<SpaceSnapshot>` | 已连接 |

#### 4.3.3 SQLite 后端管理

| 接口 | 作用 | 关键参数 | 返回 | 推荐顺序 |
|------|------|----------|------|----------|
| `enable_sqlite(request)` | 启用 SQLite backend | `ControllerSqliteEnableRequest` | `()` | `attach_space` 后 |
| `disable_sqlite(space_id, binding_id)` | 关闭 SQLite backend | `space_id`、`binding_id` | `bool` | SQLite 数据面结束后 |

#### 4.3.4 SQLite typed 数据面

| 接口 | 作用 | 关键参数 | 返回 | 说明 |
|------|------|----------|------|------|
| `execute_sqlite_script_typed(space_id, binding_id, sql, params)` | 执行脚本/DDL/单次写入 | `sql`、`Vec<ControllerSqliteValue>` | `ControllerSqliteExecuteResult` | 适合建表、插入、更新、删除 |
| `execute_sqlite_batch_typed(space_id, binding_id, sql, items)` | 批量执行同一 SQL 模板 | `Vec<Vec<ControllerSqliteValue>>` | `ControllerSqliteExecuteBatchResult` | 适合批量写入 |
| `query_sqlite_json_typed(space_id, binding_id, sql, params)` | 查询并返回 JSON 行集 | `sql`、typed 参数 | `ControllerSqliteQueryResult` | 结果在 `json_data` |
| `open_sqlite_query_stream_typed(space_id, binding_id, sql, params, target_chunk_size)` | 打开流式查询 | `target_chunk_size` 可选 | `ControllerSqliteQueryStreamOpenResult` | 适合大结果集 |
| `wait_sqlite_query_stream_metrics(stream_id)` | 等待 stream 完成并返回终态指标 | `stream_id` | `ControllerSqliteQueryStreamMetrics` | stream 打开后 |
| `read_sqlite_query_stream_chunk(stream_id, index)` | 读取指定块 | `stream_id`、`index` | `Vec<u8>` | 配合 metrics/chunk_count 使用 |
| `close_sqlite_query_stream(stream_id)` | 释放 stream 暂存资源 | `stream_id` | `bool` | 用完必须调用 |
| `tokenize_sqlite_text(space_id, binding_id, tokenizer_mode, text, search_mode)` | 文本分词 | 分词模式、文本、search_mode | `ControllerSqliteTokenizeResult` | FTS 前置 |
| `list_sqlite_custom_words(space_id, binding_id)` | 列出自定义词 | space/binding | `ControllerSqliteListCustomWordsResult` | 词典管理 |
| `upsert_sqlite_custom_word(space_id, binding_id, word, weight)` | 新增/更新自定义词 | `word`、`weight` | `ControllerSqliteDictionaryMutationResult` | 词典管理 |
| `remove_sqlite_custom_word(space_id, binding_id, word)` | 删除自定义词 | `word` | `ControllerSqliteDictionaryMutationResult` | 词典管理 |
| `ensure_sqlite_fts_index(space_id, binding_id, index_name, tokenizer_mode)` | 确保 FTS 索引存在 | `index_name`、分词模式 | `ControllerSqliteEnsureFtsIndexResult` | FTS 初始化 |
| `rebuild_sqlite_fts_index(space_id, binding_id, index_name, tokenizer_mode)` | 全量重建 FTS 索引 | 同上 | `ControllerSqliteRebuildFtsIndexResult` | 需要重建时 |
| `upsert_sqlite_fts_document(space_id, binding_id, index_name, tokenizer_mode, id, file_path, title, content)` | 写入 FTS 文档 | 文档主键、路径、标题、正文 | `ControllerSqliteFtsMutationResult` | FTS 建索引后 |
| `delete_sqlite_fts_document(space_id, binding_id, index_name, id)` | 删除 FTS 文档 | `id` | `ControllerSqliteFtsMutationResult` | FTS 删除 |
| `search_sqlite_fts(space_id, binding_id, index_name, tokenizer_mode, query, limit, offset)` | 检索 FTS | 查询词、分页 | `ControllerSqliteSearchFtsResult` | FTS 查询 |

#### 4.3.5 SQLite JSON 兼容包装接口

这组接口本质上还是走 typed API，只是把 JSON 参数数组先解析成 `ControllerSqliteValue`。

| 接口 | 作用 | 参数格式 | 返回 |
|------|------|----------|------|
| `execute_sqlite_script(space_id, binding_id, sql, params_json)` | JSON 参数执行脚本 | `params_json` 为 JSON 数组字符串 | `ControllerSqliteExecuteResult` |
| `execute_sqlite_batch(space_id, binding_id, sql, batch_params_json)` | JSON 批量执行 | `Vec<String>`，每个元素都是一组 JSON 参数数组 | `ControllerSqliteExecuteBatchResult` |
| `query_sqlite_json(space_id, binding_id, sql, params_json)` | JSON 参数查询 | JSON 数组字符串 | `ControllerSqliteQueryResult` |
| `open_sqlite_query_stream(space_id, binding_id, sql, params_json, target_chunk_size)` | JSON 参数流式查询 | JSON 数组字符串 | `ControllerSqliteQueryStreamOpenResult` |

`params_json` 支持的值：

- JSON 数字
- JSON 字符串
- JSON 布尔
- JSON `null`
- 字节包装对象：`{"type":"bytes","base64":"..."}` 或 `{"__type":"bytes","base64":"..."}`

#### 4.3.6 LanceDB 后端管理

| 接口 | 作用 | 关键参数 | 返回 | 推荐顺序 |
|------|------|----------|------|----------|
| `enable_lancedb(request)` | 启用 LanceDB backend | `ControllerLanceDbEnableRequest` | `()` | `attach_space` 后 |
| `disable_lancedb(space_id, binding_id)` | 关闭 LanceDB backend | `space_id`、`binding_id` | `bool` | LanceDB 数据面结束后 |

#### 4.3.7 LanceDB typed 数据面

| 接口 | 作用 | 关键参数 | 返回 | 说明 |
|------|------|----------|------|------|
| `create_lancedb_table_typed(space_id, binding_id, table_name, columns, overwrite_if_exists)` | 建表 | `Vec<ControllerLanceDbColumnDef>` | `ControllerLanceDbCreateTableResult` | 首次建表 |
| `upsert_lancedb_typed(space_id, binding_id, table_name, input_format, data, key_columns)` | 写入/更新数据 | 输入格式、二进制载荷、主键列 | `ControllerLanceDbUpsertResult` | JSON Rows 或 Arrow IPC |
| `search_lancedb_typed(space_id, binding_id, table_name, vector, limit, filter, vector_column, output_format)` | 向量检索 | 查询向量、过滤条件、输出格式 | `ControllerLanceDbSearchResult` | `data` 字段按 `format` 解码 |
| `delete_lancedb_typed(space_id, binding_id, table_name, condition)` | 条件删除 | SQL 风格条件 | `ControllerLanceDbDeleteResult` | 删除匹配行 |
| `drop_lancedb_table(space_id, binding_id, table_name)` | 删表 | 表名 | `ControllerLanceDbDropTableResult` | 最终清理 |

#### 4.3.8 LanceDB JSON 兼容包装接口

| 接口 | 作用 | `request_json` 结构 | 说明 |
|------|------|---------------------|------|
| `create_lancedb_table(space_id, binding_id, request_json)` | 建表 | `table_name`、`columns`、`overwrite_if_exists` | 内部会解析 JSON 再转 typed |
| `upsert_lancedb(space_id, binding_id, request_json, data)` | 写入 | `table_name`、`input_format`、`key_columns` | 二进制数据单独传 `data` |
| `search_lancedb(space_id, binding_id, request_json)` | 检索 | `table_name`、`vector`、`limit`、`filter`、`vector_column`、`output_format` | `limit` 默认 10 |
| `delete_lancedb(space_id, binding_id, request_json)` | 删除 | `table_name`、`condition` | 条件删除 |

### 4.4 Rust SDK 最小对接顺序

#### 4.4.1 基础生命周期

```rust
let config = ControllerClientConfig::default();
let registration = ClientRegistration {
    client_id: "demo-client".into(),
    host_kind: "demo".into(),
    process_id: std::process::id(),
    process_name: "demo-host".into(),
    lease_ttl_secs: Some(120),
};

let client = ControllerClient::new(config, registration);
client.connect().await?;
client.attach_space(SpaceRegistration {
    space_id: "root".into(),
    space_label: "ROOT".into(),
    space_kind: SpaceKind::Root,
    space_root: "D:/runtime/root".into(),
}).await?;
```

#### 4.4.2 SQLite 典型流程

```rust
client.enable_sqlite(ControllerSqliteEnableRequest {
    space_id: "root".into(),
    binding_id: "default".into(),
    db_path: "D:/data/demo.db".into(),
    ..Default::default()
}).await?;

client.execute_sqlite_script_typed(
    "root",
    "default",
    "CREATE TABLE IF NOT EXISTS items(id INTEGER PRIMARY KEY, name TEXT NOT NULL);",
    vec![],
).await?;
```

#### 4.4.3 LanceDB 典型流程

```rust
client.enable_lancedb(ControllerLanceDbEnableRequest {
    space_id: "root".into(),
    binding_id: "default".into(),
    default_db_path: "D:/data/lancedb".into(),
    ..Default::default()
}).await?;
```

## 5. FFI 对接说明

### 5.1 FFI 基本规则

#### 5.1.1 返回码

| 常量 | 值 | 含义 |
|------|----|------|
| `FFI_STATUS_OK` | `0` | 调用成功 |
| `FFI_STATUS_ERR` | `1` | 调用失败 |

#### 5.1.2 句柄与线程安全

- `vldb_controller_ffi_client_create*` 返回的是不透明句柄
- 同一 handle 的内部调用会被串行化
- `client_free` 后句柄失效
- 伪造句柄不会命中真实 client

#### 5.1.3 指针约束

| 项目 | 规则 |
|------|------|
| `client_out` | `client_create` / `client_create_json` 中必须非空 |
| `error_out` | 可空 |
| `response_out` / `result_out` | 可空；为空时不会分配输出对象 |
| `client` | 所有 `client_*` 接口都要求有效句柄 |

#### 5.1.4 内存释放规则

- 所有由 FFI 返回的字符串必须用 `vldb_controller_ffi_string_free`
- 所有返回的字节缓冲必须用 `vldb_controller_ffi_bytes_free`
- 所有返回的结果对象都必须调用对应 `*_free`
- 不允许宿主自己 `free()` Rust 返回的指针

#### 5.1.5 binding 规则

这是当前 FFI 接口非常重要的限制：

> **FFI 当前不对外暴露独立 `binding_id`。所有 SQLite / LanceDB native 与 JSON 接口内部都会使用 `binding_id = space_id`。**

这意味着：

- 每个 `space_id` 在 FFI 侧只等价暴露一个 backend binding
- 如果集成方需要一个 space 下管理多个 binding，请优先使用 Rust SDK 或扩展 FFI

### 5.2 FFI Native 输入结构

#### 5.2.1 `FfiControllerClientConfig`

| 字段 | C 类型 | 含义 |
|------|--------|------|
| `endpoint` | `const char*` | controller endpoint |
| `auto_spawn` | `unsigned char` | 0/1 |
| `spawn_executable` | `const char*` | 可执行文件路径，可空 |
| `spawn_process_mode` | `int` | `0=Service`，`1=Managed` |
| `minimum_uptime_secs` | `unsigned long long` | 最小存活时间 |
| `idle_timeout_secs` | `unsigned long long` | 空闲超时 |
| `default_lease_ttl_secs` | `unsigned long long` | 默认租约 TTL |
| `connect_timeout_secs` | `unsigned long long` | 单次连接超时 |
| `startup_timeout_secs` | `unsigned long long` | 拉起总等待时间 |
| `startup_retry_interval_ms` | `unsigned long long` | 拉起重试间隔 |
| `lease_renew_interval_secs` | `unsigned long long` | 续租间隔 |

#### 5.2.2 `FfiClientRegistration`

| 字段 | C 类型 | 含义 |
|------|--------|------|
| `client_id` | `const char*` | client 标识 |
| `host_kind` | `const char*` | 宿主类型 |
| `process_id` | `u32` | 进程号 |
| `process_name` | `const char*` | 进程名 |
| `lease_ttl_secs` | `unsigned long long` | 0 表示使用默认值 |

#### 5.2.3 `FfiSpaceRegistration`

| 字段 | C 类型 | 含义 |
|------|--------|------|
| `space_id` | `const char*` | 空间标识 |
| `space_label` | `const char*` | 空间标签 |
| `space_kind` | `int` | `0=Root`，`1=User`，`2=Project` |
| `space_root` | `const char*` | 空间根路径 |

#### 5.2.4 `FfiControllerSqliteEnableRequest`

| 字段 | C 类型 | 含义 |
|------|--------|------|
| `space_id` | `const char*` | 目标空间，同时会被用作 `binding_id` |
| `db_path` | `const char*` | SQLite 文件路径 |
| 其他配置字段 | 同 Rust SDK 语义 | 与 `ControllerSqliteEnableRequest` 一致 |

#### 5.2.5 `FfiControllerLanceDbEnableRequest`

| 字段 | C 类型 | 含义 |
|------|--------|------|
| `space_id` | `const char*` | 目标空间，同时会被用作 `binding_id` |
| `default_db_path` | `const char*` | 默认库路径 |
| `db_root` | `const char*` | 子库根目录，可空 |
| `read_consistency_interval_ms` | `unsigned long long` | 0 表示 `None` |
| `max_upsert_payload` | `unsigned long long` | 最大 upsert 载荷 |
| `max_search_limit` | `unsigned long long` | 最大搜索条数 |
| `max_concurrent_requests` | `unsigned long long` | 最大并发数 |

#### 5.2.6 `FfiSqliteValue`

`kind` 的取值：

| `kind` | 语义 | 使用字段 |
|--------|------|----------|
| `0` | `Int64` | `int64_value` |
| `1` | `Float64` | `float64_value` |
| `2` | `String` | `string_value` |
| `3` | `Bytes` | `bytes_value` + `bytes_len` |
| `4` | `Bool` | `bool_value` |
| `5` | `Null` | 无 |

#### 5.2.7 `FfiSqliteBatchItem`

| 字段 | 含义 |
|------|------|
| `params` | 指向一组 `FfiSqliteValue` |
| `params_len` | 参数数量 |

#### 5.2.8 `FfiControllerLanceDbColumnDef`

| 字段 | 含义 |
|------|------|
| `name` | 列名 |
| `column_type` | 列类型，取值同 Rust 枚举映射 |
| `vector_dim` | 向量维度 |
| `nullable` | 0/1 |

列类型取值：

| 值 | 类型 |
|----|------|
| `0` | `Unspecified` |
| `1` | `String` |
| `2` | `Int64` |
| `3` | `Float64` |
| `4` | `Bool` |
| `5` | `VectorFloat32` |
| `6` | `Float32` |
| `7` | `Uint64` |
| `8` | `Int32` |
| `9` | `Uint32` |

### 5.3 FFI JSON 请求结构

#### 5.3.1 client / control plane

| JSON 接口 | 请求体结构 |
|-----------|------------|
| `client_create_json` | `{ "config": ControllerClientConfig, "registration": ClientRegistration }` |
| `client_connect_json` | `{}` |
| `client_shutdown_json` | `{}` |
| `client_attach_space_json` | `{ "registration": SpaceRegistration }` |
| `client_detach_space_json` | `{ "space_id": "root" }` |
| `client_list_spaces_json` | `{}` |

#### 5.3.2 SQLite JSON 请求

| JSON 接口 | 请求体结构 |
|-----------|------------|
| `client_enable_sqlite_json` | `{ "request": ControllerSqliteEnableRequest }` |
| `client_disable_sqlite_json` | `{ "space_id": "root" }` |
| `client_execute_sqlite_script_json` | `{ "space_id": "...", "sql": "...", "params": [...] }` |
| `client_query_sqlite_json_json` | `{ "space_id": "...", "sql": "...", "params": [...] }` |
| `client_execute_sqlite_batch_json` | `{ "space_id": "...", "sql": "...", "batch_params": [[...], [...]] }` |
| `client_query_sqlite_stream_json` | `{ "space_id": "...", "sql": "...", "params": [...], "target_chunk_size": 65536 }` |
| `client_tokenize_sqlite_text_json` | `{ "space_id": "...", "tokenizer_mode": "none|jieba", "text": "...", "search_mode": true }` |
| `client_list_sqlite_custom_words_json` | `{ "space_id": "..." }` |
| `client_upsert_sqlite_custom_word_json` | `{ "space_id": "...", "word": "...", "weight": 42 }` |
| `client_remove_sqlite_custom_word_json` | `{ "space_id": "...", "word": "..." }` |
| `client_ensure_sqlite_fts_index_json` | `{ "space_id": "...", "index_name": "...", "tokenizer_mode": "none|jieba" }` |
| `client_rebuild_sqlite_fts_index_json` | 同上 |
| `client_upsert_sqlite_fts_document_json` | `{ "space_id": "...", "index_name": "...", "tokenizer_mode": "...", "id": "...", "file_path": "...", "title": "...", "content": "..." }` |
| `client_delete_sqlite_fts_document_json` | `{ "space_id": "...", "index_name": "...", "id": "..." }` |
| `client_search_sqlite_fts_json` | `{ "space_id": "...", "index_name": "...", "tokenizer_mode": "...", "query": "...", "limit": 10, "offset": 0 }` |

SQLite JSON 参数数组支持：

- 数字
- 字符串
- 布尔
- `null`
- 字节对象：`{"type":"bytes","base64":"..."}` 或 `{"__type":"bytes","base64":"..."}`

#### 5.3.3 LanceDB JSON 请求

| JSON 接口 | 请求体结构 |
|-----------|------------|
| `client_enable_lancedb_json` | `{ "request": ControllerLanceDbEnableRequest }` |
| `client_disable_lancedb_json` | `{ "space_id": "..." }` |
| `client_create_lancedb_table_json` | `{ "space_id": "...", "table_name": "...", "columns": [...], "overwrite_if_exists": false }` |
| `client_upsert_lancedb_json` | `{ "space_id": "...", "table_name": "...", "input_format": "json_rows|arrow_ipc", "key_columns": [...], "data_base64": "..." }` |
| `client_search_lancedb_json` | `{ "space_id": "...", "table_name": "...", "vector": [0.1, 0.2], "limit": 10, "filter": "", "vector_column": "", "output_format": "arrow_ipc|json_rows" }` |
| `client_delete_lancedb_json` | `{ "space_id": "...", "table_name": "...", "condition": "id = 'a'" }` |
| `client_drop_lancedb_table_json` | `{ "space_id": "...", "table_name": "..." }` |

### 5.4 FFI 导出函数说明

#### 5.4.1 通用与内存函数

| 接口 | 作用 | 关键参数 | 返回 / 输出 | 调用顺序 |
|------|------|----------|-------------|----------|
| `vldb_controller_ffi_version()` | 返回库版本字符串 | 无 | `char*` | 可随时调用 |
| `vldb_controller_ffi_string_free(value)` | 释放字符串 | `char*` | 无 | 所有字符串输出用后调用 |
| `vldb_controller_ffi_bytes_free(data, len)` | 释放字节数组 | 指针 + 长度 | 无 | 所有裸字节输出用后调用 |

#### 5.4.2 client 生命周期

| 接口 | 作用 | 参数 | 输出 | 推荐顺序 |
|------|------|------|------|----------|
| `vldb_controller_ffi_client_create` | native 方式创建 handle | `FfiControllerClientConfig*`、`FfiClientRegistration*` | `client_out` | 第一步 |
| `vldb_controller_ffi_client_create_json` | JSON 方式创建 handle | `request_json` | `client_out`、`response_out` | 第一步 |
| `vldb_controller_ffi_client_free` | 释放 handle 并停止内部资源 | `client` | 无 | 最后一步 |
| `vldb_controller_ffi_client_connect` | 连接、注册、启动续租 | `client` | 无 | create 后 |
| `vldb_controller_ffi_client_connect_json` | JSON 方式 connect | `client`、`request_json={}` | `response_out` | create 后 |
| `vldb_controller_ffi_client_shutdown` | 停止续租并注销 | `client` | 无 | 退出前 |
| `vldb_controller_ffi_client_shutdown_json` | JSON 方式 shutdown | `client`、`request_json={}` | `response_out` | 退出前 |

#### 5.4.3 状态与空间控制

| 接口 | 作用 | 参数 | 输出 | 前置条件 |
|------|------|------|------|----------|
| `vldb_controller_ffi_client_get_status` | 获取状态快照 | `client` | `FfiControllerStatusSnapshot*` | 已 connect 推荐 |
| `vldb_controller_ffi_client_get_status_json` | JSON 方式获取状态 | `client` | `response_out` | 同上 |
| `vldb_controller_ffi_controller_status_free` | 释放状态快照 | `status` | 无 | `get_status` 后 |
| `vldb_controller_ffi_client_attach_space` | native 方式 attach space | `FfiSpaceRegistration*` | `FfiSpaceSnapshot*` | 已 connect |
| `vldb_controller_ffi_client_attach_space_json` | JSON 方式 attach | `request_json` | `response_out` | 已 connect |
| `vldb_controller_ffi_client_detach_space` | native 方式 detach | `space_id` | `detached_out` | 已 attach |
| `vldb_controller_ffi_client_detach_space_json` | JSON 方式 detach | `request_json` | `response_out` | 已 attach |
| `vldb_controller_ffi_client_list_spaces` | 列空间快照 | `client` | `FfiSpaceSnapshotArray*` | 已 connect |
| `vldb_controller_ffi_client_list_spaces_json` | JSON 方式列空间 | `client` | `response_out` | 已 connect |
| `vldb_controller_ffi_space_snapshot_array_free` | 释放空间数组 | `spaces` | 无 | `list_spaces` 后 |
| `vldb_controller_ffi_space_snapshot_free` | 释放单个空间快照 | `space` | 无 | `attach_space` 后 |

#### 5.4.4 SQLite native 接口

| 接口 | 作用 | 关键参数 | 输出 | 推荐顺序 |
|------|------|----------|------|----------|
| `vldb_controller_ffi_client_enable_sqlite` | 启用 SQLite backend | `FfiControllerSqliteEnableRequest*` | 无 | `attach_space` 后 |
| `vldb_controller_ffi_client_disable_sqlite` | 关闭 SQLite backend | `space_id` | `disabled_out` | 数据面结束后 |
| `vldb_controller_ffi_client_execute_sqlite_script` | 执行脚本/DDL/写入 | `space_id`、`sql`、`FfiSqliteValue[]` | `FfiControllerSqliteExecuteResult*` | backend 已启用 |
| `vldb_controller_ffi_client_query_sqlite_json` | 查询 JSON 行集 | `space_id`、`sql`、`FfiSqliteValue[]` | `FfiControllerSqliteQueryResult*` | backend 已启用 |
| `vldb_controller_ffi_client_execute_sqlite_batch` | 批量执行 | `space_id`、`sql`、`FfiSqliteBatchItem[]` | `FfiControllerSqliteExecuteBatchResult*` | backend 已启用 |
| `vldb_controller_ffi_client_query_sqlite_stream` | 执行流式查询并一次性收集全部 chunks | `space_id`、`sql`、参数、目标块大小 | `FfiControllerSqliteQueryStreamResult*` | 不是增量 API |
| `vldb_controller_ffi_client_tokenize_sqlite_text` | 文本分词 | `space_id`、`tokenizer_mode`、`text`、`search_mode` | `FfiControllerSqliteTokenizeResult*` | FTS 前置 |
| `vldb_controller_ffi_client_list_sqlite_custom_words` | 列出自定义词 | `space_id` | `FfiControllerSqliteListCustomWordsResult*` | 词典管理 |
| `vldb_controller_ffi_client_upsert_sqlite_custom_word` | 新增/更新自定义词 | `space_id`、`word`、`weight` | `FfiControllerSqliteDictionaryMutationResult*` | 词典管理 |
| `vldb_controller_ffi_client_remove_sqlite_custom_word` | 删除自定义词 | `space_id`、`word` | `FfiControllerSqliteDictionaryMutationResult*` | 词典管理 |
| `vldb_controller_ffi_client_ensure_sqlite_fts_index` | 确保 FTS 索引存在 | `space_id`、`index_name`、`tokenizer_mode` | `FfiControllerSqliteEnsureFtsIndexResult*` | FTS 初始化 |
| `vldb_controller_ffi_client_rebuild_sqlite_fts_index` | 重建 FTS 索引 | 同上 | `FfiControllerSqliteRebuildFtsIndexResult*` | 重建时 |
| `vldb_controller_ffi_client_upsert_sqlite_fts_document` | 写入 FTS 文档 | `space_id`、`index_name`、`tokenizer_mode`、文档字段 | `FfiControllerSqliteFtsMutationResult*` | ensure 后 |
| `vldb_controller_ffi_client_delete_sqlite_fts_document` | 删除 FTS 文档 | `space_id`、`index_name`、`id` | `FfiControllerSqliteFtsMutationResult*` | 文档删除 |
| `vldb_controller_ffi_client_search_sqlite_fts` | 检索 FTS | `space_id`、`index_name`、`tokenizer_mode`、`query`、`limit`、`offset` | `FfiControllerSqliteSearchFtsResult*` | 检索 |

#### 5.4.5 SQLite JSON 接口

| 接口 | 作用 | `request_json` | 输出 |
|------|------|----------------|------|
| `vldb_controller_ffi_client_enable_sqlite_json` | JSON 方式启用 SQLite | `{ "request": ControllerSqliteEnableRequest }` | `response_out` |
| `vldb_controller_ffi_client_disable_sqlite_json` | JSON 方式关闭 SQLite | `{ "space_id": "..." }` | `response_out` |
| `vldb_controller_ffi_client_execute_sqlite_script_json` | JSON 参数执行脚本 | `{ "space_id": "...", "sql": "...", "params": [...] }` | `response_out` |
| `vldb_controller_ffi_client_query_sqlite_json_json` | JSON 参数查询 | 同上 | `response_out` |
| `vldb_controller_ffi_client_execute_sqlite_batch_json` | JSON 批量执行 | `{ "space_id": "...", "sql": "...", "batch_params": [[...]] }` | `response_out` |
| `vldb_controller_ffi_client_query_sqlite_stream_json` | JSON 流式查询 | `{ "space_id": "...", "sql": "...", "params": [...], "target_chunk_size": ... }` | `response_out` |
| `vldb_controller_ffi_client_tokenize_sqlite_text_json` | JSON 分词 | `{ "space_id": "...", "tokenizer_mode": "...", "text": "...", "search_mode": true }` | `response_out` |
| `vldb_controller_ffi_client_list_sqlite_custom_words_json` | JSON 列自定义词 | `{ "space_id": "..." }` | `response_out` |
| `vldb_controller_ffi_client_upsert_sqlite_custom_word_json` | JSON 新增/更新自定义词 | `{ "space_id": "...", "word": "...", "weight": 42 }` | `response_out` |
| `vldb_controller_ffi_client_remove_sqlite_custom_word_json` | JSON 删除自定义词 | `{ "space_id": "...", "word": "..." }` | `response_out` |
| `vldb_controller_ffi_client_ensure_sqlite_fts_index_json` | JSON ensure FTS | `{ "space_id": "...", "index_name": "...", "tokenizer_mode": "..." }` | `response_out` |
| `vldb_controller_ffi_client_rebuild_sqlite_fts_index_json` | JSON rebuild FTS | 同上 | `response_out` |
| `vldb_controller_ffi_client_upsert_sqlite_fts_document_json` | JSON 写入 FTS 文档 | 文档字段 JSON | `response_out` |
| `vldb_controller_ffi_client_delete_sqlite_fts_document_json` | JSON 删除 FTS 文档 | `{ "space_id": "...", "index_name": "...", "id": "..." }` | `response_out` |
| `vldb_controller_ffi_client_search_sqlite_fts_json` | JSON 检索 FTS | `{ "space_id": "...", "index_name": "...", "tokenizer_mode": "...", "query": "...", "limit": 10, "offset": 0 }` | `response_out` |

#### 5.4.6 SQLite 结果释放函数

| 释放函数 | 对应生产函数 |
|----------|--------------|
| `vldb_controller_ffi_sqlite_execute_result_free` | `client_execute_sqlite_script` |
| `vldb_controller_ffi_sqlite_query_result_free` | `client_query_sqlite_json` |
| `vldb_controller_ffi_sqlite_execute_batch_result_free` | `client_execute_sqlite_batch` |
| `vldb_controller_ffi_sqlite_query_stream_result_free` | `client_query_sqlite_stream` |
| `vldb_controller_ffi_byte_buffer_array_free` | `FfiControllerSqliteQueryStreamResult.chunks` |
| `vldb_controller_ffi_sqlite_tokenize_result_free` | `client_tokenize_sqlite_text` |
| `vldb_controller_ffi_sqlite_custom_word_array_free` | `FfiControllerSqliteListCustomWordsResult.words` |
| `vldb_controller_ffi_sqlite_list_custom_words_result_free` | `client_list_sqlite_custom_words` |
| `vldb_controller_ffi_sqlite_dictionary_mutation_result_free` | `upsert/remove_sqlite_custom_word` |
| `vldb_controller_ffi_sqlite_ensure_fts_index_result_free` | `client_ensure_sqlite_fts_index` |
| `vldb_controller_ffi_sqlite_rebuild_fts_index_result_free` | `client_rebuild_sqlite_fts_index` |
| `vldb_controller_ffi_sqlite_fts_mutation_result_free` | `upsert/delete_sqlite_fts_document` |
| `vldb_controller_ffi_sqlite_search_fts_hit_array_free` | `FfiControllerSqliteSearchFtsResult.hits` |
| `vldb_controller_ffi_sqlite_search_fts_result_free` | `client_search_sqlite_fts` |

#### 5.4.7 LanceDB native 接口

| 接口 | 作用 | 关键参数 | 输出 | 推荐顺序 |
|------|------|----------|------|----------|
| `vldb_controller_ffi_client_enable_lancedb` | 启用 LanceDB backend | `FfiControllerLanceDbEnableRequest*` | 无 | `attach_space` 后 |
| `vldb_controller_ffi_client_disable_lancedb` | 关闭 LanceDB backend | `space_id` | `disabled_out` | 数据面结束后 |
| `vldb_controller_ffi_client_create_lancedb_table` | 建表 | `space_id`、`table_name`、`FfiControllerLanceDbColumnDef[]` | `FfiControllerLanceDbCreateTableResult*` | 首次建表 |
| `vldb_controller_ffi_client_upsert_lancedb` | 写入/更新 | `space_id`、`table_name`、`input_format`、`data`、`key_columns` | `FfiControllerLanceDbUpsertResult*` | 建表后 |
| `vldb_controller_ffi_client_search_lancedb` | 向量检索 | `space_id`、`table_name`、`vector`、`limit`、`filter`、`vector_column`、`output_format` | `FfiControllerLanceDbSearchResult*` | 写入后 |
| `vldb_controller_ffi_client_delete_lancedb` | 条件删除 | `space_id`、`table_name`、`condition` | `FfiControllerLanceDbDeleteResult*` | 删除时 |
| `vldb_controller_ffi_client_drop_lancedb_table` | 删表 | `space_id`、`table_name` | `FfiControllerLanceDbDropTableResult*` | 最终清理 |

#### 5.4.8 LanceDB JSON 接口

| 接口 | 作用 | `request_json` | 输出 |
|------|------|----------------|------|
| `vldb_controller_ffi_client_enable_lancedb_json` | JSON 方式启用 | `{ "request": ControllerLanceDbEnableRequest }` | `response_out` |
| `vldb_controller_ffi_client_disable_lancedb_json` | JSON 方式关闭 | `{ "space_id": "..." }` | `response_out` |
| `vldb_controller_ffi_client_create_lancedb_table_json` | JSON 建表 | `{ "space_id": "...", "table_name": "...", "columns": [...], "overwrite_if_exists": false }` | `response_out` |
| `vldb_controller_ffi_client_upsert_lancedb_json` | JSON 写入 | `{ "space_id": "...", "table_name": "...", "input_format": "...", "key_columns": [...], "data_base64": "..." }` | `response_out` |
| `vldb_controller_ffi_client_search_lancedb_json` | JSON 检索 | `{ "space_id": "...", "table_name": "...", "vector": [...], "limit": 10, "filter": "", "vector_column": "", "output_format": "" }` | `response_out` |
| `vldb_controller_ffi_client_delete_lancedb_json` | JSON 删除 | `{ "space_id": "...", "table_name": "...", "condition": "..." }` | `response_out` |
| `vldb_controller_ffi_client_drop_lancedb_table_json` | JSON 删表 | `{ "space_id": "...", "table_name": "..." }` | `response_out` |

#### 5.4.9 LanceDB 结果释放函数

| 释放函数 | 对应生产函数 |
|----------|--------------|
| `vldb_controller_ffi_lancedb_create_table_result_free` | `client_create_lancedb_table` |
| `vldb_controller_ffi_lancedb_upsert_result_free` | `client_upsert_lancedb` |
| `vldb_controller_ffi_lancedb_search_result_free` | `client_search_lancedb` |
| `vldb_controller_ffi_lancedb_delete_result_free` | `client_delete_lancedb` |
| `vldb_controller_ffi_lancedb_drop_table_result_free` | `client_drop_lancedb_table` |

### 5.5 FFI 典型执行顺序

#### 5.5.1 Native 模式最小顺序

1. 组装 `FfiControllerClientConfig`
2. 组装 `FfiClientRegistration`
3. `vldb_controller_ffi_client_create`
4. `vldb_controller_ffi_client_connect`
5. `vldb_controller_ffi_client_attach_space`
6. `vldb_controller_ffi_client_enable_sqlite` / `vldb_controller_ffi_client_enable_lancedb`
7. 执行数据面函数
8. 调用对应 `*_free`
9. `vldb_controller_ffi_client_shutdown`
10. `vldb_controller_ffi_client_free`

#### 5.5.2 FFI SQLite 典型顺序

1. `client_attach_space`
2. `client_enable_sqlite`
3. `client_execute_sqlite_script`
4. `client_query_sqlite_json` 或 `client_query_sqlite_stream`
5. `client_disable_sqlite`

#### 5.5.3 FFI LanceDB 典型顺序

1. `client_attach_space`
2. `client_enable_lancedb`
3. `client_create_lancedb_table`
4. `client_upsert_lancedb`
5. `client_search_lancedb`
6. `client_delete_lancedb` 或 `client_drop_lancedb_table`
7. `client_disable_lancedb`

## 6. 接入注意事项

### 6.1 关于 owned space / stream

当前 controller 已启用真实权限校验：

- 一个 space 若已归属于附着 client，后续数据面访问必须来自该 attached client
- SQLite stream 的 `wait/chunk/close` 也会校验 owner client

对接层不应假设“只知道 `space_id` 就能直接访问”。

### 6.2 关于 FFI 的 binding 能力

当前 FFI 为了保持接口简单，所有 backend 操作都默认：

```text
binding_id = space_id
```

如果集成方需要：

- 同一 space 下多个 SQLite binding
- 同一 space 下多个 LanceDB binding

建议优先使用 Rust SDK，或者后续扩展 FFI 请求结构。

### 6.3 关于查询流

Rust SDK 的 SQLite stream 是真正的增量式接口：

- `open`
- `wait_metrics`
- `read_chunk`
- `close`

FFI 当前暴露的是“一次性收集型 stream 兼容接口”：

- native 模式返回 `FfiControllerSqliteQueryStreamResult`
- JSON 模式返回 `chunks_base64`

如果集成方需要真正增量消费流，请优先使用 Rust SDK 或扩展 FFI。

### 6.4 关于错误处理

Rust SDK：

- 统一返回 `BoxError`
- 推荐 `downcast_ref::<VldbControllerError>()` 做细分

FFI：

- 先判断返回码
- 再读 `error_out`
- 若 `error_out == NULL`，仍然可能收到失败返回码

## 7. 对接建议结论

如果集成方是 Rust：

- 优先使用 `ControllerClient`
- 因为它能完整控制 `binding_id`
- 也更适合 stream 和多 binding 场景

如果集成方是非 Rust 语言：

- 优先使用当前 FFI
- 但要明确它当前是“每个 space 暴露一个 binding”的模型
- 并严格配对 `*_free`

如果集成方需要一份最小判断：

> **需要完整能力、显式 binding、多段 stream 控制，用 Rust SDK；需要稳定 C ABI、单 binding 简化接入，用 FFI。**

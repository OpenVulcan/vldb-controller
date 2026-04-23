# vldb-controller Client / FFI 全量 API 对接说明

## 1. 文档范围

本文覆盖当前工作树下两套正式接入面：

- Rust SDK：`vldb-controller-client`
- FFI：`ffi/src/lib.rs` 与 `ffi/include/vldb_controller_ffi.h`

本文重点说明：

- 当前会话模型
- 接口字段与结构
- 每类接口的作用与执行顺序
- 自动续租、自动重连、自动重初始化行为
- 上游接入方需要注意的兼容点

---

## 2. 当前核心模型

## 2.1 方案 A：控制器分配会话 ID

当前实现已经切换到 **方案 A：由 controller 分配 `client_session_id`**。

新的身份语义如下：

| 概念 | 来源 | 作用 |
|------|------|------|
| `client_name` | 宿主提供 | 仅用于诊断与展示，可重复 |
| `client_session_id` | controller 分配 | 不可预测的唯一会话主键，用于 attach / enable / 数据面 / 清理 |

这意味着：

- 同名宿主不会再被合并
- 同名宿主不会再继承旧状态
- 每次注册都是一条新的独立会话
- 一条会话结束后，它拥有的 attachment / backend / stream 都会被回收

一句话理解：

> **一个宿主实例对应一个独立 client session，结束后不继承，不复用，不按名字合并。**

## 2.2 为什么要这样改

改动原因主要有 4 个：

1. 旧模型把宿主传入的 `client_id` 当成主键，同名注册会发生合并，无法支持多个同名宿主并发存在。
2. 旧模型下，宿主重启后容易继承上一次残留的 attachment / backend，资源生命周期不干净。
3. 托管模式希望做到“所有客户端都结束后，controller 才能自动退出”，必须保证客户端身份是一次会话，而不是一个长期名字。
4. SDK 需要在 controller 丢失、重启或网络波动后自动恢复，恢复的前提是能明确区分“旧会话失效”和“新会话重建”。

## 2.3 客户端结束与资源回收

一条 client session 在以下两种情况下结束：

1. 显式结束：调用 `shutdown()` / `unregister_client`
2. 被动结束：租约超时，controller 判定该会话已经丢失

结束后 controller 会自动回收该会话拥有的：

- `space attachment`
- `SQLite binding`
- `LanceDB binding`
- `SQLite query stream`

当且仅当 **所有 client session 都已结束**，并且：

- 没有 inflight request
- 没有活跃 stream
- 没有残留 backend
- 满足 `managed` 模式的最小运行时间与空闲时间

controller 才会进入自停流程。

## 2.4 掉线后不会立刻关闭

controller 不会因为一次瞬时断连就立刻销毁会话，而是依赖租约超时来抗网络波动。

当前默认值：

- SDK 后台续租间隔：`30s`
- controller 默认租约 TTL：`120s`

所以宿主断开后，controller 通常会先等待一段租约宽限时间；如果宿主在此期间恢复续租，会话会继续存活；如果一直没有恢复，才会进入自动回收。

---

## 3. 推荐执行顺序

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
10. `shutdown`
11. `client_free`（仅 FFI）

补充说明：

- `connect` 内部会执行 `register_client`
- `register_client` 返回新的 `client_session_id`
- Rust SDK / FFI 会自动缓存这个 `client_session_id`
- 后续所有需要归属校验的接口都会自动携带该 `client_session_id`

---

## 4. Rust SDK 对接说明

## 4.1 导出入口

`client/src/lib.rs` 当前对外导出：

- `ControllerClient`
- `ControllerClientConfig`
- `types.rs` 中全部请求 / 响应 / 枚举 / 错误类型

建议引用方式：

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

## 4.2 关键结构

### 4.2.1 `ControllerClientConfig`

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

- `localhost:PORT` 会在 auto-spawn 绑定场景下转成 `127.0.0.1:PORT`
- 非本地主机名可用于连接，但不能自动转换成 bind 地址

### 4.2.2 `ClientRegistration`

| 字段 | 类型 | 作用 |
|------|------|------|
| `client_name` | `String` | 宿主名称，仅用于诊断，可重复 |
| `host_kind` | `String` | 宿主类别，例如 `mcp` / `ide` / `service` |
| `process_id` | `u32` | 宿主进程号 |
| `process_name` | `String` | 宿主进程名 |
| `lease_ttl_secs` | `Option<u64>` | 自定义租约 TTL；空则使用 controller 默认值 |

重要说明：

- `ClientRegistration` 不再承担主键职责
- 控制器真正使用的主键是 `client_session_id`
- 即使两个宿主传入完全相同的 `client_name`，也会拿到两条不同 session

### 4.2.3 `ClientLeaseSnapshot`

| 字段 | 类型 | 作用 |
|------|------|------|
| `client_session_id` | `String` | controller 分配的唯一会话 ID |
| `client_name` | `String` | 注册时传入的宿主名称 |
| `host_kind` | `String` | 宿主类别 |
| `process_id` | `u32` | 宿主进程号 |
| `process_name` | `String` | 宿主进程名 |
| `last_seen_unix_ms` | `u64` | 最近一次活动/续租时间 |
| `expires_at_unix_ms` | `u64` | 当前会话过期时间 |
| `attached_space_ids` | `Vec<String>` | 当前会话已附着空间 |

### 4.2.4 `SpaceRegistration`

| 字段 | 类型 | 作用 |
|------|------|------|
| `space_id` | `String` | 运行时空间标识，例如 `root`、`user`、项目 ID |
| `space_label` | `String` | 人类可读标签 |
| `space_kind` | `SpaceKind` | `Root` / `User` / `Project` |
| `space_root` | `String` | 空间根路径 |

### 4.2.5 `ControllerSqliteEnableRequest`

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

### 4.2.6 `ControllerLanceDbEnableRequest`

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

### 4.2.7 常用值与结果结构

| 类型 | 作用 |
|------|------|
| `ControllerSqliteValue` | typed SQLite 参数值：`Int64` / `Float64` / `String` / `Bytes` / `Bool` / `Null` |
| `ControllerSqliteQueryStreamOpenResult` | stream 打开结果，包含 `stream_id` |
| `ControllerSqliteQueryStreamMetrics` | stream 最终行数/块数/字节数 |
| `ControllerLanceDbColumnDef` | LanceDB 列定义 |
| `ControllerLanceDbInputFormat` | `Unspecified` / `JsonRows` / `ArrowIpc` |
| `ControllerLanceDbOutputFormat` | `Unspecified` / `ArrowIpc` / `JsonRows` |
| `VldbControllerError` | 当前统一错误类型 |

## 4.3 `ControllerClient` 生命周期

### 4.3.1 基本语义

- 所有业务方法都会自动携带当前实例持有的 `client_session_id`
- `connect()` 会确保 controller 可达，然后创建或恢复一个有效 session
- `shutdown()` 会停止续租、注销当前 session，并清空本地期望状态缓存
- 同名宿主重连不会继承旧会话，而是重新初始化一条新会话

### 4.3.2 自动重连与自动重初始化

当前 Rust SDK 已内建以下恢复行为：

1. 如果 controller 不可连接，且 `auto_spawn = true`，SDK 会自动尝试拉起 controller
2. 如果 controller 重启，导致旧 `client_session_id` 已不存在，SDK 会自动：
   1. 重新 `register_client`
   2. 获取新的 `client_session_id`
   3. 重放已缓存的 `attach_space`
   4. 重放已缓存的 `enable_sqlite`
   5. 重放已缓存的 `enable_lancedb`

也就是说：

- 网络抖动不会立刻导致 controller 回收会话
- controller 真正丢失或重启后，SDK 会在下一次连接/续租时自动恢复
- 显式 `shutdown()` 会清空期望状态，因此下次启动不会继承上一次已结束会话的资源

## 4.4 `ControllerClient` 接口总览

### 4.4.1 构造与生命周期

| 接口 | 作用 | 关键参数 | 返回 | 调用时机 |
|------|------|----------|------|----------|
| `new(config, registration)` | 创建 client 对象，不发网络请求 | `ControllerClientConfig`、`ClientRegistration` | `ControllerClient` | 第一步 |
| `connect()` | 确保 controller 可达，必要时自动拉起，然后注册或重建当前 session，并启动续租 | 无 | `Result<(), BoxError>` | 业务调用前 |
| `shutdown()` | 停止后台续租、注销当前 session，并清空本地期望状态 | 无 | `Result<(), BoxError>` | 退出前 |

### 4.4.2 控制面接口

| 接口 | 作用 | 关键参数 | 返回 | 前置条件 |
|------|------|----------|------|----------|
| `get_status()` | 读取 controller 当前状态 | 无 | `ControllerStatusSnapshot` | 可在 connect 前后调用 |
| `list_clients()` | 读取当前 session 的客户端租约快照 | 无 | `Vec<ClientLeaseSnapshot>` | 已 connect |
| `attach_space(registration)` | 将当前 session 附着到某个 space | `SpaceRegistration` | `SpaceSnapshot` | 已 connect |
| `detach_space(space_id)` | 将当前 session 从某个 space 解绑 | `space_id` | `bool` | 已 attach |
| `list_spaces()` | 查看当前 session 可见的 space 快照 | 无 | `Vec<SpaceSnapshot>` | 已 connect |

补充说明：

- `list_clients()` 现在要求绑定当前 `client_session_id`，只返回当前 session 自己的租约快照
- 普通客户端不应依赖 `list_clients()` 作为 session 获取方式
- `list_spaces()` 现在按当前 `client_session_id` 做可见范围过滤，只返回当前 session 已附着空间的逻辑快照
- `list_spaces()` 仅保留逻辑状态，不再暴露 `space_root` 与 SQLite/LanceDB backend 的物理目标路径
- `get_status()`、`list_clients()`、`list_spaces()` 不会因为 `auto_spawn=true` 就在 controller 已自停后把它重新拉起
- `get_status()`、`list_clients()`、`list_spaces()` 这类纯诊断请求不会刷新托管自停的空闲计时
- 直连 gRPC 调用时，`ListClientsRequest` / `ListSpacesRequest` 都必须显式携带当前 `client_session_id`

### 4.4.3 SQLite 后端管理

| 接口 | 作用 | 关键参数 | 返回 | 推荐顺序 |
|------|------|----------|------|----------|
| `enable_sqlite(request)` | 启用 SQLite backend | `ControllerSqliteEnableRequest` | `()` | `attach_space` 后 |
| `disable_sqlite(space_id, binding_id)` | 关闭 SQLite backend | `space_id`、`binding_id` | `bool` | 数据面结束后 |

### 4.4.4 SQLite typed 数据面

| 接口 | 作用 |
|------|------|
| `execute_sqlite_script_typed(space_id, binding_id, sql, params)` | 执行脚本 / DDL / 单次写入 |
| `execute_sqlite_batch_typed(space_id, binding_id, sql, items)` | 批量执行同一 SQL 模板 |
| `query_sqlite_json_typed(space_id, binding_id, sql, params)` | 查询并返回 JSON 行集 |
| `open_sqlite_query_stream_typed(space_id, binding_id, sql, params, target_chunk_size)` | 打开流式查询 |
| `wait_sqlite_query_stream_metrics(stream_id)` | 等待 stream 完成并返回最终指标 |
| `read_sqlite_query_stream_chunk(stream_id, index)` | 读取指定 chunk |
| `close_sqlite_query_stream(stream_id)` | 释放 stream 临时资源 |
| `tokenize_sqlite_text(space_id, binding_id, tokenizer_mode, text, search_mode)` | 文本分词 |
| `list_sqlite_custom_words(space_id, binding_id)` | 列出自定义词 |
| `upsert_sqlite_custom_word(space_id, binding_id, word, weight)` | 新增 / 更新自定义词 |
| `remove_sqlite_custom_word(space_id, binding_id, word)` | 删除自定义词 |
| `ensure_sqlite_fts_index(space_id, binding_id, index_name, tokenizer_mode)` | 确保 FTS 索引存在 |
| `rebuild_sqlite_fts_index(space_id, binding_id, index_name, tokenizer_mode)` | 重建 FTS 索引 |
| `upsert_sqlite_fts_document(...)` | 写入 FTS 文档 |
| `delete_sqlite_fts_document(...)` | 删除 FTS 文档 |
| `search_sqlite_fts(...)` | 执行 FTS 检索 |

### 4.4.5 SQLite JSON 包装接口

这组接口本质上还是走 typed API，只是先把 JSON 参数解析成 `ControllerSqliteValue`。

| 接口 | 作用 |
|------|------|
| `execute_sqlite_script(space_id, binding_id, sql, params_json)` | JSON 参数执行脚本 |
| `execute_sqlite_batch(space_id, binding_id, sql, batch_params_json)` | JSON 批量执行 |
| `query_sqlite_json(space_id, binding_id, sql, params_json)` | JSON 参数查询 |
| `open_sqlite_query_stream(space_id, binding_id, sql, params_json, target_chunk_size)` | JSON 参数流式查询 |

### 4.4.6 LanceDB 后端管理

| 接口 | 作用 | 关键参数 | 返回 | 推荐顺序 |
|------|------|----------|------|----------|
| `enable_lancedb(request)` | 启用 LanceDB backend | `ControllerLanceDbEnableRequest` | `()` | `attach_space` 后 |
| `disable_lancedb(space_id, binding_id)` | 关闭 LanceDB backend | `space_id`、`binding_id` | `bool` | 数据面结束后 |

### 4.4.7 LanceDB typed 数据面

| 接口 | 作用 |
|------|------|
| `create_lancedb_table_typed(space_id, binding_id, table_name, columns, overwrite_if_exists)` | 建表 |
| `upsert_lancedb_typed(space_id, binding_id, table_name, input_format, data, key_columns)` | 写入 / 更新 |
| `search_lancedb_typed(space_id, binding_id, table_name, vector, limit, filter, vector_column, output_format)` | 向量检索 |
| `delete_lancedb_typed(space_id, binding_id, table_name, condition)` | 条件删除 |
| `drop_lancedb_table(space_id, binding_id, table_name)` | 删表 |

### 4.4.8 LanceDB JSON 包装接口

| 接口 | 作用 |
|------|------|
| `create_lancedb_table(space_id, binding_id, request_json)` | JSON 建表 |
| `upsert_lancedb(space_id, binding_id, request_json, data)` | JSON 写入 |
| `search_lancedb(space_id, binding_id, request_json)` | JSON 检索 |
| `delete_lancedb(space_id, binding_id, request_json)` | JSON 删除 |

## 4.5 Rust SDK 最小示例

### 4.5.1 生命周期示例

```rust
let config = ControllerClientConfig::default();
let registration = ClientRegistration {
    client_name: "demo-client".into(),
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

### 4.5.2 SQLite 示例

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

### 4.5.3 LanceDB 示例

```rust
client.enable_lancedb(ControllerLanceDbEnableRequest {
    space_id: "root".into(),
    binding_id: "default".into(),
    default_db_path: "D:/data/lancedb".into(),
    ..Default::default()
}).await?;
```

---

## 5. FFI 对接说明

## 5.1 FFI 基本规则

### 5.1.1 返回码

| 常量 | 值 | 含义 |
|------|----|------|
| `FFI_STATUS_OK` | `0` | 成功 |
| `FFI_STATUS_ERR` | `1` | 失败 |

### 5.1.2 句柄与线程安全

- `vldb_controller_ffi_client_create*` 返回不透明句柄
- 同一 handle 的内部调用会被串行化
- `client_free` 后句柄失效
- 伪造句柄不会命中真实 client

### 5.1.3 指针约束

| 项目 | 规则 |
|------|------|
| `client_out` | `client_create` / `client_create_json` 中必须非空 |
| `error_out` | 可空 |
| `response_out` / `result_out` | 可空；为空时不会分配输出对象 |
| `client` | 所有 `client_*` 接口都要求有效句柄 |

### 5.1.4 内存释放规则

- 所有由 FFI 返回的字符串必须用 `vldb_controller_ffi_string_free`
- 所有返回的字节缓冲必须用 `vldb_controller_ffi_bytes_free`
- 所有返回的结果对象都必须调用对应 `*_free`
- 不允许宿主自己 `free()` Rust 返回的指针

### 5.1.5 FFI 侧会话语义

- FFI 注册结构现在传的是 `client_name`
- FFI 在 `client_connect` 后会自动拿到 controller 分配的 `client_session_id`
- 后续 native / JSON 调用都由句柄内部自动携带该 `client_session_id`
- 同名宿主不会在 controller 侧被合并

### 5.1.6 binding 规则

当前 FFI 仍保留一个重要限制：

> **FFI 不对外暴露独立 `binding_id`。所有 SQLite / LanceDB 调用内部统一使用 `binding_id = space_id`。**

如果上游需要一个 `space` 下管理多个 binding，请优先使用 Rust SDK。

## 5.2 FFI 关键结构

### 5.2.1 `FfiControllerClientConfig`

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

### 5.2.2 `FfiClientRegistration`

| 字段 | C 类型 | 含义 |
|------|--------|------|
| `client_name` | `const char*` | 宿主名称，仅用于诊断，可重复 |
| `host_kind` | `const char*` | 宿主类型 |
| `process_id` | `u32` | 进程号 |
| `process_name` | `const char*` | 进程名 |
| `lease_ttl_secs` | `unsigned long long` | 0 表示使用默认值 |

### 5.2.3 `FfiSpaceRegistration`

| 字段 | C 类型 | 含义 |
|------|--------|------|
| `space_id` | `const char*` | 空间标识 |
| `space_label` | `const char*` | 空间标签 |
| `space_kind` | `int` | `0=Root`，`1=User`，`2=Project` |
| `space_root` | `const char*` | 空间根路径 |

### 5.2.4 `FfiControllerSqliteEnableRequest`

| 字段 | C 类型 | 含义 |
|------|--------|------|
| `space_id` | `const char*` | 目标空间，同时也会被用作 `binding_id` |
| `db_path` | `const char*` | SQLite 文件路径 |
| 其他配置字段 | 同 Rust SDK 语义 | 与 `ControllerSqliteEnableRequest` 一致 |

### 5.2.5 `FfiControllerLanceDbEnableRequest`

| 字段 | C 类型 | 含义 |
|------|--------|------|
| `space_id` | `const char*` | 目标空间，同时也会被用作 `binding_id` |
| `default_db_path` | `const char*` | 默认库路径 |
| `db_root` | `const char*` | 子库根目录，可空 |
| `read_consistency_interval_ms` | `unsigned long long` | 0 表示 `None` |
| `max_upsert_payload` | `unsigned long long` | 最大 upsert 载荷 |
| `max_search_limit` | `unsigned long long` | 最大搜索条数 |
| `max_concurrent_requests` | `unsigned long long` | 最大并发数 |

## 5.3 FFI 导出函数分组

### 5.3.1 生命周期接口

| 接口 | 作用 |
|------|------|
| `vldb_controller_ffi_client_create` | native 方式创建 handle |
| `vldb_controller_ffi_client_create_json` | JSON 方式创建 handle |
| `vldb_controller_ffi_client_connect` | 连接 controller，注册或恢复会话，并启动续租 |
| `vldb_controller_ffi_client_connect_json` | JSON 方式 connect |
| `vldb_controller_ffi_client_shutdown` | 停止续租并注销当前 session |
| `vldb_controller_ffi_client_shutdown_json` | JSON 方式 shutdown |
| `vldb_controller_ffi_client_free` | 释放 handle |

### 5.3.2 状态与空间控制

| 接口 | 作用 |
|------|------|
| `vldb_controller_ffi_client_get_status` | 读取 controller 状态 |
| `vldb_controller_ffi_client_get_status_json` | JSON 方式读取状态 |
| `vldb_controller_ffi_client_attach_space` | native attach space |
| `vldb_controller_ffi_client_attach_space_json` | JSON attach space |
| `vldb_controller_ffi_client_detach_space` | native detach space |
| `vldb_controller_ffi_client_detach_space_json` | JSON detach space |
| `vldb_controller_ffi_client_list_spaces` | 列 space 快照 |
| `vldb_controller_ffi_client_list_spaces_json` | JSON 方式列 space |

### 5.3.3 SQLite native 接口

| 接口 | 作用 |
|------|------|
| `vldb_controller_ffi_client_enable_sqlite` | 启用 SQLite backend |
| `vldb_controller_ffi_client_disable_sqlite` | 关闭 SQLite backend |
| `vldb_controller_ffi_client_execute_sqlite_script` | 执行脚本 / DDL / 写入 |
| `vldb_controller_ffi_client_query_sqlite_json` | 查询 JSON 行集 |
| `vldb_controller_ffi_client_execute_sqlite_batch` | 批量执行 |
| `vldb_controller_ffi_client_query_sqlite_stream` | 一次性收集型 stream 查询 |
| `vldb_controller_ffi_client_tokenize_sqlite_text` | 文本分词 |
| `vldb_controller_ffi_client_list_sqlite_custom_words` | 列出自定义词 |
| `vldb_controller_ffi_client_upsert_sqlite_custom_word` | 新增 / 更新自定义词 |
| `vldb_controller_ffi_client_remove_sqlite_custom_word` | 删除自定义词 |
| `vldb_controller_ffi_client_ensure_sqlite_fts_index` | 确保 FTS 索引存在 |
| `vldb_controller_ffi_client_rebuild_sqlite_fts_index` | 重建 FTS 索引 |
| `vldb_controller_ffi_client_upsert_sqlite_fts_document` | 写入 FTS 文档 |
| `vldb_controller_ffi_client_delete_sqlite_fts_document` | 删除 FTS 文档 |
| `vldb_controller_ffi_client_search_sqlite_fts` | 检索 FTS |

### 5.3.4 SQLite JSON 接口

这组 JSON 接口与 native 功能一一对应，只是把请求参数封装进 `request_json`。

| 接口 | 典型请求体 |
|------|------------|
| `vldb_controller_ffi_client_enable_sqlite_json` | `{ "request": ControllerSqliteEnableRequest }` |
| `vldb_controller_ffi_client_disable_sqlite_json` | `{ "space_id": "root" }` |
| `vldb_controller_ffi_client_execute_sqlite_script_json` | `{ "space_id": "...", "sql": "...", "params": [...] }` |
| `vldb_controller_ffi_client_query_sqlite_json_json` | `{ "space_id": "...", "sql": "...", "params": [...] }` |
| `vldb_controller_ffi_client_execute_sqlite_batch_json` | `{ "space_id": "...", "sql": "...", "batch_params": [[...], [...]] }` |
| `vldb_controller_ffi_client_query_sqlite_stream_json` | `{ "space_id": "...", "sql": "...", "params": [...], "target_chunk_size": 65536 }` |

### 5.3.5 LanceDB native 接口

| 接口 | 作用 |
|------|------|
| `vldb_controller_ffi_client_enable_lancedb` | 启用 LanceDB backend |
| `vldb_controller_ffi_client_disable_lancedb` | 关闭 LanceDB backend |
| `vldb_controller_ffi_client_create_lancedb_table` | 建表 |
| `vldb_controller_ffi_client_upsert_lancedb` | 写入 / 更新 |
| `vldb_controller_ffi_client_search_lancedb` | 向量检索 |
| `vldb_controller_ffi_client_delete_lancedb` | 条件删除 |
| `vldb_controller_ffi_client_drop_lancedb_table` | 删表 |

### 5.3.6 LanceDB JSON 接口

| 接口 | 典型请求体 |
|------|------------|
| `vldb_controller_ffi_client_enable_lancedb_json` | `{ "request": ControllerLanceDbEnableRequest }` |
| `vldb_controller_ffi_client_disable_lancedb_json` | `{ "space_id": "..." }` |
| `vldb_controller_ffi_client_create_lancedb_table_json` | `{ "space_id": "...", "table_name": "...", "columns": [...], "overwrite_if_exists": false }` |
| `vldb_controller_ffi_client_upsert_lancedb_json` | `{ "space_id": "...", "table_name": "...", "input_format": "json_rows|arrow_ipc", "key_columns": [...], "data_base64": "..." }` |
| `vldb_controller_ffi_client_search_lancedb_json` | `{ "space_id": "...", "table_name": "...", "vector": [0.1, 0.2], "limit": 10, "filter": "", "vector_column": "", "output_format": "arrow_ipc|json_rows" }` |

### 5.3.7 常见释放函数

| 释放函数 | 作用 |
|----------|------|
| `vldb_controller_ffi_string_free` | 释放字符串 |
| `vldb_controller_ffi_bytes_free` | 释放字节缓冲 |
| `vldb_controller_ffi_controller_status_free` | 释放状态快照 |
| `vldb_controller_ffi_space_snapshot_free` | 释放单个 space 快照 |
| `vldb_controller_ffi_space_snapshot_array_free` | 释放 space 快照数组 |
| `vldb_controller_ffi_sqlite_*_free` | 释放 SQLite 结果对象 |
| `vldb_controller_ffi_lancedb_*_free` | 释放 LanceDB 结果对象 |

## 5.4 FFI 推荐顺序

1. 组装 `FfiControllerClientConfig`
2. 组装 `FfiClientRegistration`
3. `vldb_controller_ffi_client_create`
4. `vldb_controller_ffi_client_connect`
5. `vldb_controller_ffi_client_attach_space`
6. `vldb_controller_ffi_client_enable_sqlite` / `vldb_controller_ffi_client_enable_lancedb`
7. 执行数据面函数
8. 释放结果对象
9. `vldb_controller_ffi_client_shutdown`
10. `vldb_controller_ffi_client_free`

---

## 6. 上游对接需要明确的接口变化

## 6.1 Rust SDK 变化

| 旧语义 | 新语义 |
|--------|--------|
| `ClientRegistration.client_id` | `ClientRegistration.client_name` |
| 宿主自带 `client_id` 作为主键 | controller 分配 `client_session_id` 作为主键 |
| 同名注册会合并 | 同名注册不会合并 |
| 可能继承旧 attachment / binding | 不会继承旧会话状态 |

## 6.2 FFI 变化

| 旧字段 | 新字段 |
|--------|--------|
| `FfiClientRegistration.client_id` | `FfiClientRegistration.client_name` |

FFI 句柄内部会自动管理 `client_session_id`，上游不需要自己传 session id。

## 6.3 直连 gRPC / proto 变化

如果上游直接基于 `controller.proto` 重新生成 stub，需要按下面改：

| 消息 | 变化 |
|------|------|
| `RegisterClientRequest.client_id` | 改为 `RegisterClientRequest.client_name` |
| `ClientLeaseSnapshot.client_id` | 改为 `ClientLeaseSnapshot.client_session_id`，并新增 `client_name` |
| 其他所有原先使用 `client_id` 的请求 | 改为 `client_session_id` |

正确顺序应改成：

1. `RegisterClient(client_name, ...)`
2. 读取返回里的 `client.client_session_id`
3. 后续 `AttachSpace / EnableSqlite / EnableLanceDb / 数据面 / Stream` 全部使用这个 `client_session_id`

---

## 7. 接入注意事项

## 7.1 匿名 backend binding 已被禁止

当前 controller 已禁止匿名 `enable_sqlite` / `enable_lancedb`。

标准流程必须是：

1. 注册会话
2. `attach_space`
3. `enable_sqlite` / `enable_lancedb`

补充：

- 匿名 `attach_space` 现在也会被拒绝
- 也就是说，space 级生命周期同样必须绑定到一个真实 `client_session_id`

## 7.2 owned 资源现在会做真实校验

当前 controller 会基于 `client_session_id` 做空间与资源归属检查：

- 已归属 space 的数据面访问必须来自 attached session
- SQLite stream 的 `wait/chunk/close` 必须来自 owner session
- 非 owner / 非 attached / 已过期 session 会被拒绝

另外，当前 `detach_space` 也要求调用方先清掉自己在该空间下拥有的 backend binding。  
正确顺序应是：

1. `disable_sqlite` / `disable_lancedb`
2. `detach_space`
3. `shutdown`

## 7.3 SQLite stream 在 Rust SDK 与 FFI 的能力不同

Rust SDK 暴露真正增量式接口：

- `open`
- `wait_metrics`
- `read_chunk`
- `close`

FFI 当前暴露的是“一次性收集型 stream 兼容接口”，如果上游需要真正增量消费，建议优先使用 Rust SDK。

## 7.4 自动恢复只恢复“期望状态”，不恢复已结束会话

SDK / FFI 会自动恢复：

- attach 过的 space
- enable 过的 SQLite binding
- enable 过的 LanceDB binding

但不会恢复：

- 已显式 `shutdown` 的旧会话
- 已 `disable` / `detach` 的资源
- 已结束的 SQLite stream

---

## 8. 给上游的简版结论

如果只用一句话概括这次接入变化：

> **客户端身份已经从“宿主提供稳定 client_id”切换为“宿主提供 client_name，controller 分配 client_session_id”；后续所有资源归属、自动回收、自动恢复都围绕 `client_session_id` 运行。**

这次改动带来的直接收益是：

- 同名宿主可并发存在
- 不再发生同名会话合并
- 宿主掉线后资源能自动回收
- 所有客户端都结束后，`managed` controller 才能正确自停
- controller 重启或丢失后，SDK / FFI 可以自动重连并重新初始化

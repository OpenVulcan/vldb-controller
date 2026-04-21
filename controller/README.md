# vldb-controller

`vldb-controller` 是 `vldb-controller` workspace 里的**控制服务进程 crate**。

它是整个工程里真正持有数据库后端的一侧。

它负责：

- gRPC 控制服务
- space registry
- client lease
- backend enable / disable
- SQLite 完整数据面
- LanceDB 完整数据面
- managed 模式空闲自停

它不负责：

- 宿主侧 SDK 形态
- 直接对外暴露 Rust 库 API 给宿主

一句话说：

**`vldb-controller` 是服务本体，`vldb-controller-client` 是宿主接入 SDK。**

## 当前定位

这是一个 **service-first** crate。

它的主形态是：

- 本地 gRPC 控制服务进程

当前支持：

- `service` 模式
- `managed` 模式

默认 bind：

- `127.0.0.1:19801`

这意味着：

- 默认端点可作为共享 controller
- 宿主改端口即可得到独立 controller

## 当前已具备能力

### 控制面

- `GetStatus`
- `RegisterClient`
- `RenewClientLease`
- `UnregisterClient`
- `ListClients`
- `AttachSpace`
- `DetachSpace`
- `ListSpaces`
- `EnableSqlite`
- `DisableSqlite`
- `EnableLanceDb`
- `DisableLanceDb`

### SQLite 完整数据面

- `ExecuteSqliteScript`
- `QuerySqliteJson`
- 内部已补齐：
  - `execute_batch`
  - `query_stream`
  - `tokenize_text`
  - `list_custom_words`
  - `upsert_custom_word`
  - `remove_custom_word`
  - `ensure_fts_index`
  - `rebuild_fts_index`
  - `upsert_fts_document`
  - `delete_fts_document`
  - `search_fts`

### LanceDB 完整数据面

- `CreateLanceDbTable`
- `UpsertLanceDb`
- `SearchLanceDb`
- `DeleteLanceDb`
- 内部已补齐：
  - `drop_table`

## 依赖边界

本 crate 直接依赖：

- [vldb-sqlite](https://github.com/OpenVulcan/vldb-sqlite)
- [vldb-lancedb](https://github.com/OpenVulcan/vldb-lancedb)
- `vldb-controller-client`

其中：

- `vldb-controller-client`
  - 提供共享协议绑定与轻量契约类型
- `vldb-sqlite / vldb-lancedb`
  - 提供数据库能力实现

也就是说：

- 协议和宿主 SDK 在 `client/`
- 真正的数据库后端在 `controller/`

## 代码结构

```text
src/
├─ main.rs
├─ cli.rs
├─ server.rs
└─ core/
   ├─ mod.rs
   ├─ runtime.rs
   ├─ sqlite.rs
   └─ lancedb.rs
```

### `src/main.rs`

负责：

- 启动 gRPC 服务进程
- 托管模式关闭接线

### `src/cli.rs`

负责：

- 参数驱动启动解析
- 无配置文件启动模型

### `src/server.rs`

负责：

- gRPC 服务实现
- protobuf 请求与核心类型映射
- 控制面 / 数据面入口

### `src/core/runtime.rs`

负责：

- space registry
- client lease
- inflight request 统计
- shutdown 判定

### `src/core/sqlite.rs`

负责：

- SQLite backend 封装
- `execute_script`
- `execute_batch`
- `query_json`
- `query_stream`
- `tokenize_text`
- 自定义词典
- FTS 全能力

### `src/core/lancedb.rs`

负责：

- LanceDB backend 封装
- `create_table`
- `upsert`
- `search`
- `delete`
- `drop_table`

## 启动参数

当前支持：

```text
--bind <HOST:PORT>
--mode <service|managed>
--minimum-uptime-secs <SECONDS>
--idle-timeout-secs <SECONDS>
--default-lease-ttl-secs <SECONDS>
```

默认值：

- `--bind 127.0.0.1:19801`
- `--mode managed`
- `--minimum-uptime-secs 300`
- `--idle-timeout-secs 900`
- `--default-lease-ttl-secs 120`

## 编译

### 检查

```powershell
cargo check -p vldb-controller
```

### release

```powershell
cargo build -p vldb-controller --release
```

## 测试

```powershell
cargo test -p vldb-controller
```

## 当前边界

当前版本已经是：

- 可运行的控制服务第一版
- 且内部已经具备 SQLite / LanceDB 完整能力面

当前还没做的重点有：

- 对外 gRPC / SDK / FFI 对齐完整能力面
- 进程探测与僵尸客户端清理
- bm25 backend
- 更完整的发现 / 重连状态机
- 管理面与强制关闭策略细化

## 相关文档

- [../README.md](../README.md)
- [../docs/CONTROLLER_ARCHITECTURE.md](../docs/CONTROLLER_ARCHITECTURE.md)

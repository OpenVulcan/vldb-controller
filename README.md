# vldb-controller

`vldb-controller` 是 Vulcan 本地数据体系中的**空间级数据库控制服务**。  
它的目标是作为宿主统一接入层，**对外承接并统一替代**：

- `vldb-sqlite`
- `vldb-lancedb`

对外暴露的数据能力入口，而在内部继续复用它们的数据库实现。  
它在上层增加统一的：

- `space_id / space_label`
- 客户端租约与进程登记
- backend 按空间启停
- 共享实例与独立实例
- 托管自停策略

一句话说：

**`vldb-controller` 对外统一承接 SQLite/LanceDB 能力入口，对内复用 `vldb-sqlite` 与 `vldb-lancedb` 的实现，并增加空间级控制面。**

## 当前定位

当前仓库已经调整为 **workspace + service-first** 结构：

- `controller/`
  - gRPC 控制服务进程
- `client/`
  - 宿主侧 Rust client SDK
- `ffi/`
  - 跨语言 FFI 导出层
- 默认无配置文件
- 所有行为通过启动参数与 RPC 请求驱动

子项目入口：

- [client/README.md](client/README.md)
- [controller/README.md](controller/README.md)
- [ffi/README.md](ffi/README.md)

当前目标命名已经明确区分：

- `vldb-controller`
  - 控制服务进程
- `vldb_controller_client`
  - 宿主侧 Rust client SDK
- `vldb_controller_ffi`
  - 跨语言 FFI 导出层

这次拆分的直接目的就是：

- `client` 不再链接 `vldb-sqlite`
- `client` 不再链接 `vldb-lancedb`
- 重型数据库依赖仅保留在 `controller` 服务端
- `client` 作为 Rust SDK 存在，不再默认产出体积巨大的 `cdylib`
- `ffi` 单独负责 TS / Go / Python / C 的动态库接入

当前 `ffi/` 已经提供：

- 原生结构化 FFI 模式
- `_json` FFI 模式
- 控制面与完整 SQLite / LanceDB 数据面的双模式导出
- `_json` 数据面请求直接使用嵌套 JSON 结构，不再要求 JSON 字符串套 JSON

当前第一版已落地：

- 参数驱动启动
- gRPC 控制面
- 宿主侧 controller client 完整 SDK 代理层
- space attach / detach
- client register / renew / unregister
- SQLite / LanceDB backend enable / disable
- SQLite 全能力内部封装：
  - `execute_script`
  - `execute_batch`
  - `query_json`
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
- LanceDB 全能力内部封装：
  - `create_table`
  - `upsert`
  - `search`
  - `delete`
  - `drop_table`
- 托管模式空闲自停判定

## 共享与独立实例模型

`vldb-controller` **不要求全机唯一实例**。  
它只在 **bind 地址** 维度唯一：

- 默认共享实例：
  - `127.0.0.1:19801`
- 宿主自定义端口：
  - 即可得到自己的独立 controller 实例

这意味着：

- 多个宿主都默认使用 `127.0.0.1:19801` 时，等同共享同一个 controller
- 某个宿主如果指定 `127.0.0.1:19811`，它就会拥有自己的独立 controller

## 生命周期模型

controller 的生命周期**不依赖当前连接数**，因为 gRPC 并不要求宿主始终保持长连接。  
当前托管模式的关闭判定依赖：

- `client lease`
- 最近请求活动
- 执行中请求数量
- `minimum_uptime`
- `idle_timeout`

也就是说：

- 不是“没有连接就退出”
- 而是“没有活跃租约、没有执行中请求、请求空闲超过超时窗口，并且已满足最小时长”后才允许退出

## 启动参数

当前支持：

```text
--bind <HOST:PORT>
--mode <service|managed>
--minimum-uptime-secs <SECONDS>
--idle-timeout-secs <SECONDS>
--default-lease-ttl-secs <SECONDS>
```

### 默认值

- `--bind 127.0.0.1:19801`
- `--mode managed`
- `--minimum-uptime-secs 300`
- `--idle-timeout-secs 900`
- `--default-lease-ttl-secs 120`

## 当前代码结构

```text
client/
├─ Cargo.toml
├─ build.rs
└─ src/
   ├─ client.rs
   ├─ lib.rs
   └─ types.rs

controller/
├─ Cargo.toml
└─ src/
   ├─ cli.rs
   ├─ main.rs
   ├─ server.rs
   └─ core/
      ├─ mod.rs
      ├─ sqlite.rs
      ├─ lancedb.rs
      └─ runtime.rs

ffi/
├─ Cargo.toml
├─ README.md
├─ include/
│  └─ vldb_controller_ffi.h
└─ src/
   └─ lib.rs

proto/
└─ v1/
   └─ controller.proto
```

### `controller/src/core`

负责：

- 空间模型
- 客户端租约
- backend 生命周期
- 托管自停判定
- SQLite / LanceDB runtime 封装

### `controller/src/server.rs`

负责：

- gRPC 请求映射
- protobuf 与核心层类型转换
- 托管模式关闭监视循环

### `client/src/client.rs`

负责：

- controller endpoint 连接
- 自动唤起
- 客户端注册与后台续约
- 宿主侧控制面代理

### `ffi/src/lib.rs`

负责：

- 面向非 Rust 宿主的 C ABI 导出
- 原生模式与 JSON 模式双接口
- 控制面与完整 SQLite / LanceDB 数据面的 `*_free` 内存释放函数
- SQLite `_json` blob 参数的 `bytes_base64` 包装兼容

### `controller/src/cli.rs`

负责：

- 启动参数解析
- 无配置文件的参数驱动启动模型

### `controller/src/main.rs`

负责：

- 启动 `vldb-controller` gRPC 服务进程
- 在托管模式下接入自停关闭信号

## 设计说明

更完整的设计说明见：

- [docs/CONTROLLER_ARCHITECTURE.md](docs/CONTROLLER_ARCHITECTURE.md)

## 依赖策略

当前仓库默认使用 **git + rev** 固定版本依赖：

- `vldb-sqlite`
- `vldb-lancedb`

这样做的目的：

- 新仓库 clone 后可直接编译
- 不依赖开发机本地路径
- 避免上游分支漂移导致 controller 行为不可复现

## 开发

### 编译控制服务

```powershell
cargo check -p vldb-controller
```

### 编译宿主 SDK

```powershell
cargo check -p vldb-controller-client
```

### 编译 FFI 导出层

```powershell
cargo check -p vldb-controller-ffi
```

### 测试

```powershell
cargo test --workspace
```

## 当前能力边界

当前第一版**已经有**：

- gRPC 控制面
- controller client 代理层
- 客户端租约管理
- space attach / detach
- SQLite backend 启停
- SQLite 内部完整能力面封装
- LanceDB backend 启停
- LanceDB 内部完整能力面封装
- 托管模式空闲关闭判定

当前第一版**还没有**：

- TS / Go / Python 的独立示例工程
- 进程探测与僵尸 session 清理增强
- bm25 backend

所以当前更准确地说，它已经是：

**可运行的控制服务第一版，且 gRPC 与 Rust SDK 已具备完整 SQLite/LanceDB 能力面**

跨语言 FFI 导出面已经补齐到与 gRPC / Rust SDK 同步。

## License

MIT

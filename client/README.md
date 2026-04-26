# vldb-controller-client

`vldb-controller-client` 是 `vldb-controller` workspace 里的**宿主侧 Rust SDK**。

它的职责很明确：

- 提供 gRPC 协议绑定
- 提供轻量共享类型
- 提供 controller client 代理
- 帮助宿主完成：
  - 连接 controller
  - endpoint 不可用时自动唤起
  - 客户端注册
  - 租约续约
  - 控制面与完整数据面调用

它**不负责**：

- SQLite 后端实现
- LanceDB 后端实现
- 控制器 runtime 生命周期
- 进程内数据 ownership

一句话说：

**`vldb-controller-client` 是宿主接入 SDK，不是数据库后端，也不是控制服务本体。**

## 当前定位

当前 crate 只保留：

- `rlib`

不默认产出：

- `cdylib`

原因是：

- 它本质上是 Rust SDK
- 非 Rust 宿主更适合直接走 gRPC 协议
- 如果默认产出动态库，会把整套 gRPC 客户端栈也一起打进去，体积不划算

如果后面确实需要：

- FFI 接入
- 动态库分发

建议单独再做一个更轻的 FFI 包装层，而不是直接拿本 crate 作为跨语言动态库。

## 提供的主要内容

### 1. 协议绑定

- `rpc`

来自：

- [proto/v1/controller.proto](proto/v1/controller.proto)

当前包含：

- gRPC client / server contract
- protobuf 消息结构

### 2. 共享类型

- `types`

主要包括：

- `ControllerProcessMode`
- `ControllerRuntimeConfig`
- `ControllerServerConfig`
- `SpaceRegistration`
- `ClientRegistration`
- `ControllerSqliteEnableRequest`
- `ControllerLanceDbEnableRequest`
- `ControllerSqliteValue`
- `ControllerLanceDbColumnDef`
- `ControllerLanceDbColumnType`
- `ControllerLanceDbInputFormat`
- `ControllerLanceDbOutputFormat`
- `ControllerStatusSnapshot`
- `SpaceSnapshot`

这些类型是：

- 宿主侧 client
- controller 服务侧

共同使用的轻量契约类型。

### 3. 宿主代理

- `client`

主要导出：

- `ControllerClient`
- `ControllerClientConfig`

当前已经支持：

- endpoint 标准化
- 自动连接
- 自动唤起 controller
- client 注册
- 后台 lease 续约
- `attach_space`
- `detach_space`
- `list_clients`
- `list_spaces`
- SQLite 控制与完整数据面调用
- LanceDB 控制与完整数据面调用

## 依赖边界

本 crate **不直接依赖**：

- `vldb-sqlite`
- `vldb-lancedb`

这条边界是刻意设计的。

控制器服务进程负责：

- 数据库后端
- lifecycle
- backend routing

而宿主 SDK 只负责：

- 协议
- 请求转发
- 自动发现 / 自动唤起

## 代码结构

```text
src/
├─ lib.rs
├─ client.rs
└─ types.rs
```

### `src/lib.rs`

负责：

- crate 导出面
- `rpc / types / client` 聚合

### `src/types.rs`

负责：

- controller 共享契约类型
- 轻量请求/响应结构

### `src/client.rs`

负责：

- 宿主控制代理
- 自动唤起
- 后台续约
- gRPC 请求封装

## 编译

### 检查

```powershell
cargo check -p vldb-controller-client
```

### release

```powershell
cargo build -p vldb-controller-client --release
```

## 测试

```powershell
cargo test -p vldb-controller-client
```

## 与 controller crate 的关系

- `vldb-controller-client`
  - 宿主 SDK
- `vldb-controller`
  - 控制服务进程

服务端也会复用本 crate 的：

- `rpc`
- `types`

但服务端自己的：

- runtime
- sqlite/lancedb backend
- gRPC server 实现

都在 `controller/` crate 里，不在这里。

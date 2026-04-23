# vldb-controller 当前工作树下游影响面报告

## 一、报告目的

本报告基于 **当前未提交工作树** 重新整理对下游集成方的真实影响面，重点覆盖：

- 方案 A 会话模型切换
- SDK / FFI / gRPC 接口变化
- controller 自动回收与自动自停行为变化
- 客户端自动重连与自动重初始化行为

配套的全量接口说明见 [CLIENT_FFI_API_INTEGRATION.md](/D:/projects/vldb-controller/docs/CLIENT_FFI_API_INTEGRATION.md)。

---

## 二、本轮变更的核心结论

这次不是简单的字段重命名，而是 **客户端身份模型整体升级**：

### 1. 从“宿主提供主键”改成“控制器分配会话主键”

旧模型：

- 宿主传 `client_id`
- controller 直接拿它当主键
- 同名注册会被合并
- 同名宿主可能继承旧 attachment / backend

新模型：

- 宿主传 `client_name`
- controller 在注册时分配 `client_session_id`
- `client_name` 只用于诊断，可重复
- `client_session_id` 才是不可预测的唯一会话主键
- 同名宿主不会再被合并，也不会继承旧状态

### 2. 客户端丢失后的资源现在能自动回收

当前 controller 已支持：

- 显式 `shutdown/unregister` 后回收会话拥有的资源
- 租约超时后自动回收会话拥有的资源

回收对象包括：

- space attachment
- SQLite binding
- LanceDB binding
- SQLite query stream

### 3. 只有在所有客户端都结束后，托管 controller 才会自停

当前 `managed` 模式下，controller 不会因为单个客户端断开就立即退出。  
只有当所有 client session 都结束，并且没有残留 backend / stream / inflight request，且满足最小运行时间和空闲时间后，controller 才会自停。

### 4. SDK / FFI 现在支持自动重连与自动重初始化

如果 controller 不可达、被拉起失败后恢复、或 controller 已重启导致旧 session 消失：

- SDK / FFI 会自动重新注册
- 获取新的 `client_session_id`
- 自动重放之前缓存的：
  - space attachment
  - SQLite binding
  - LanceDB binding

这满足“出现无法连接时自动重启服务并重新初始化连接”的目标。

---

## 三、为什么必须这样改

这次改动主要为了解决以下现实问题：

### 1. 同名宿主并发时会被错误合并

旧模型下，如果两个宿主都传同一个 `client_id`，controller 会把它们视为同一个客户端，这在多窗口、多实例、同名宿主并发时是错误的。

### 2. 宿主重启后容易继承旧状态

旧模型下，宿主如果复用旧 `client_id`，可能继承上一个进程残留的 attachment / backend / ownership，导致生命周期混乱。

### 3. 托管模式无法真正闭环

如果宿主已经死亡，但 backend 仍绑定在旧逻辑 client 上，controller 会一直认为还有资源残留，导致 `managed` 模式无法自停。

### 4. 自动恢复需要会话级身份，而不是名字级身份

SDK 想要做到“controller 丢了后自动重连并重放资源”，前提是要能明确区分：

- 旧会话已经失效
- 新会话已经建立

这必须依赖 controller 分配的唯一 session id。

---

## 四、代码变更快照

当前主要改动文件：

| 模块 | 文件 |
|------|------|
| Rust SDK | `client/src/client.rs` `client/src/types.rs` |
| Controller 核心 | `controller/src/core/runtime.rs` `controller/src/server.rs` |
| proto | `proto/v1/controller.proto` |
| FFI | `ffi/src/lib.rs` `ffi/include/vldb_controller_ffi.h` |

当前 `git diff --stat`：

- 7 个文件变更
- 1576 行新增
- 386 行删除

当前验证结果：

- `cargo test`：通过，**48/48**

---

## 五、接口变化总览

## 5.1 protobuf / gRPC 变化

### 5.1.1 兼容性结论

- **线格式兼容**：字段号仍是原位置，且类型仍是 `string`
- **源码不兼容**：只要上游重新生成 gRPC stub，就会看到新的字段名与新语义

也就是说：

> 已发布的旧二进制客户端如果继续按旧线格式发包，线层仍能通信；  
> 但任何重新生成代码的上游，都必须按新的字段名与新语义接入。

### 5.1.2 主要字段变化

| 消息 | 旧字段 | 新字段 | 新语义 |
|------|--------|--------|--------|
| `RegisterClientRequest` | `client_id` | `client_name` | 宿主名称，不再是主键 |
| `ClientLeaseSnapshot` | `client_id` | `client_session_id` | controller 分配的唯一会话 ID |
| `ClientLeaseSnapshot` | 无 | `client_name` | 注册时的宿主名称 |
| 其余所有 session 相关请求 | `client_id` | `client_session_id` | 后续调用必须绑定具体会话 |

### 5.1.3 上游 gRPC 正确调用顺序

上游如果直连 gRPC，应改成：

1. `RegisterClient(client_name, host_kind, process_id, process_name, lease_ttl_secs)`
2. 读取返回中的 `client.client_session_id`
3. 后续：
   - `AttachSpace`
   - `DetachSpace`
   - `EnableSqlite`
   - `EnableLanceDb`
   - 所有 SQLite / LanceDB 数据面
   - 所有 SQLite stream follow-up
   
   全部都使用这个 `client_session_id`

如果仍把一个稳定名字当主键继续传，会在新 stub 语义下直接走错模型。

补充说明：

- `ListClients` 现在只保留诊断价值
- `ListClients` 现在要求绑定 `client_session_id`，只返回当前 session 自己的租约快照
- `ListSpaces` 现在要求绑定 `client_session_id`，只返回当前 session 可见的空间
- `ListSpaces` 不再暴露 `space_root` 与 SQLite/LanceDB backend 的物理路径目标
- `DetachSpaceResponse.space` 不再回传其他仍存活会话可见的空间快照，成功解绑后统一返回空快照
- 上游不能依赖 `ListClients` 反查或拼接业务 session
- `GetStatus`、`ListClients`、`ListSpaces` 这类纯诊断请求不会再延长 `managed` 模式的空闲自停窗口
- Rust SDK / FFI 的 `get_status`、`list_clients`、`list_spaces` 也不会因为 `auto_spawn=true` 就在 controller 已自停后再次把它拉起
- 直连 gRPC 的 `ListClientsRequest` / `ListSpacesRequest` 现在都必须显式携带 `client_session_id`

## 5.2 Rust SDK 变化

### 5.2.1 类型变化

| 旧类型字段 | 新类型字段 |
|------------|------------|
| `ClientRegistration.client_id` | `ClientRegistration.client_name` |
| `ClientLeaseSnapshot.client_id` | `ClientLeaseSnapshot.client_session_id` |
| 无 | `ClientLeaseSnapshot.client_name` |

### 5.2.2 行为变化

| 旧行为 | 新行为 |
|--------|--------|
| 同名注册可能合并 | 同名注册不会合并 |
| 同名宿主可能继承旧 attachment / backend | 不再继承旧会话状态 |
| `connect()` 注册一个逻辑 client | `connect()` 注册或恢复一个 session |
| controller 丢失后需要宿主自行重建状态 | SDK 自动重建 session 并重放期望状态 |

### 5.2.3 需要上游检查的代码

如果上游直接使用 Rust SDK，需要检查：

1. 所有构造 `ClientRegistration` 的地方，把 `client_id` 改成 `client_name`
2. 如果有直接读取 `ClientLeaseSnapshot` 的地方，把 `client_id` 改成：
   - 业务主键用 `client_session_id`
   - 展示名称用 `client_name`
3. 如果有“同名宿主应复用旧状态”的历史假设，需要删除

## 5.3 FFI 变化

### 5.3.1 结构体变化

| 旧字段 | 新字段 |
|--------|--------|
| `FfiClientRegistration.client_id` | `FfiClientRegistration.client_name` |

### 5.3.2 行为变化

- FFI 不再把宿主提供的字段当作最终主键
- FFI 在 `client_connect` 后自动拿到新的 `client_session_id`
- 后续所有 native / JSON 数据面调用都由句柄内部自动携带该 session id

### 5.3.3 对上游绑定代码的影响

需要改：

- C / C++ / Go / Python / Node-API 等语言里对 `FfiClientRegistration` 的字段赋值

不需要改：

- FFI 调用顺序
- FFI 句柄生命周期
- 大多数 `client_*` 调用参数

---

## 六、行为变化总览

## 6.1 匿名 backend binding 已被禁止

现在不允许绕过注册和附着流程，直接匿名启用 SQLite / LanceDB backend。
同时，匿名 `attach_space` 也已被禁止。

标准流程必须是：

1. 注册 session
2. `attach_space`
3. `enable_sqlite` / `enable_lancedb`

这会让所有 backend 都能被追踪到 owner session，从而在宿主丢失后自动回收。

## 6.2 资源归属检查更严格

当前 controller 会基于 `client_session_id` 执行真实的归属检查：

- 已归属 space 的数据面调用，必须来自 attached session
- SQLite stream 的 `wait/chunk/close`，必须来自 owner session
- 非 owner / 非 attached / 已过期 session 会被拒绝
- `detach_space` 现在也不能绕过 backend 生命周期；调用方必须先 `disable_*`，再 `detach_space`

## 6.3 客户端掉线后的关闭时机

controller 不会在 transport 短暂抖动时立刻清资源，而是等待租约超时：

- 默认续租间隔：`30s`
- 默认租约 TTL：`120s`

因此：

- 短暂网络波动不会马上导致会话结束
- 长时间失联才会触发自动回收
- 所有客户端都结束并满足空闲条件后，`managed` controller 才会退出

## 6.4 自动重连与自动重初始化

当前 SDK / FFI 具备以下能力：

1. controller 不可达时，若开启 `auto_spawn`，会尝试自动拉起 controller
2. controller 重启后，旧 `client_session_id` 失效时，会重新注册新 session
3. 会自动重放：
   - 已 attach 的 space
   - 已 enable 的 SQLite binding
   - 已 enable 的 LanceDB binding

不自动重放的内容：

- 已关闭会话的旧状态
- 已关闭 backend
- 已关闭 stream

---

## 七、对上游的实际影响等级

### 必须立即关注

| 项目 | 影响等级 | 原因 |
|------|----------|------|
| 重新生成 proto stub 的 gRPC 客户端 | 高 | 字段名与字段语义均已变化 |
| Rust SDK 构造 `ClientRegistration` 的代码 | 高 | `client_id` 已改为 `client_name` |
| FFI 绑定层构造 `FfiClientRegistration` 的代码 | 高 | 字段已改名 |
| 依赖“同名客户端应合并”的逻辑 | 高 | 新模型明确禁止合并与继承 |

### 建议重点验证

| 项目 | 影响等级 | 原因 |
|------|----------|------|
| controller 重启后的宿主自动恢复链路 | 中 | 新增自动重连与状态重放 |
| 宿主异常退出后的 backend 自动回收 | 中 | 生命周期逻辑发生实质变化 |
| 托管模式自动退出行为 | 中 | 现在以“所有 session 都结束”为前提 |

### 正向收益为主

| 项目 | 影响 |
|------|------|
| 多个同名宿主并发存在 | 正向收益，互不合并 |
| controller 重启后的自动恢复 | 正向收益，宿主无需手工重建大部分状态 |
| 宿主长时间丢失后的自动回收 | 正向收益，controller 生命周期终于闭环 |

---

## 八、建议上游执行的回归验证

建议至少补做以下 10 项验证：

1. 同名 `client_name` 的两个宿主可同时注册，并拿到不同 `client_session_id`
2. 两个同名宿主分别 attach / enable 不同资源，彼此互不继承
3. 一个宿主显式 `shutdown` 后，只回收它自己的资源
4. 一个宿主长时间断线后，只回收它自己的资源
5. 另一个仍存活的宿主资源不受影响
6. 所有宿主都结束后，`managed` controller 会在租约与空闲窗口后退出
7. controller 重启后，SDK 能自动注册新 session 并恢复 attach / enable 状态
8. 直连 gRPC 客户端能正确读取 `RegisterClientResponse.client.client_session_id`
9. FFI 绑定层能正确使用 `client_name` 字段创建客户端
10. 匿名 `enable_sqlite` / `enable_lancedb` 会被拒绝

---

## 九、给上游的简版结论

如果只用一句话概括这次改动：

> **客户端身份已经从“宿主传稳定 `client_id`”切换为“宿主传 `client_name`，controller 分配 `client_session_id`”；后续所有资源归属、自动回收、自动恢复都围绕 `client_session_id` 运行。**

这次改动的本质收益是：

- 不再按同名宿主合并
- 宿主结束后资源能自动清理
- 所有客户端都结束后，托管 controller 才能正确退出
- controller 丢失后，SDK / FFI 能自动重连并重新初始化

---

**报告日期**：2026-04-23  
**适用范围**：当前未提交工作树  
**用途**：提交给上游评估接口适配与回归测试范围

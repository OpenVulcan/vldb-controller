# vldb-controller 当前未提交改动下游影响面报告

## 一、报告目的

本报告基于 **当前未提交工作树** 重新整理下游影响面，用于提交给集成方评估接入成本与兼容性风险。

这份报告同时修正了旧版 `DOWNSTREAM_IMPACT_REPORT.md` 中已经与代码不一致的描述，尤其是以下几类：

- FFI 句柄模型与空指针约束
- Rust SDK 公开方法签名变化
- gRPC 错误码与权限校验行为
- Controller 启动参数边界规则
- 运行时维护循环与 SQLite 流回收策略

配套的全量接口说明见 [CLIENT_FFI_API_INTEGRATION.md](/D:/projects/vldb-controller/docs/CLIENT_FFI_API_INTEGRATION.md)。

## 二、当前变更快照

截至当前工作树，主要未提交代码变更如下：

| 模块 | 文件 |
|------|------|
| Rust SDK | `client/src/client.rs` `client/src/lib.rs` `client/src/types.rs` |
| Controller 启动与服务 | `controller/src/cli.rs` `controller/src/main.rs` `controller/src/server.rs` |
| Controller 核心 | `controller/src/core/runtime.rs` `controller/src/core/sqlite.rs` `controller/src/core/lancedb.rs` |
| FFI | `ffi/src/lib.rs` |
| 依赖 | `controller/Cargo.toml` `Cargo.lock` |

当前 `git diff --stat` 统计为：

- 12 个代码/依赖文件变更
- 1979 行新增
- 584 行删除

本轮已验证：

- `cargo test` 通过，当前为 **45/45**
- `cargo clippy --all-targets --all-features -- -D warnings` 通过
- `cargo check --all-targets` 通过

## 三、与旧版报告不一致的关键校正

下表列出旧版报告里最容易误导集成方的内容，以及当前代码的真实状态。

| 旧版表述 | 当前真实状态 | 集成方应如何理解 |
|----------|--------------|------------------|
| FFI 只是“补充线程安全文档” | FFI 已改成 **真实堆地址不透明句柄 + 全局注册表 + 内部串行化互斥**，并且能拒绝伪造句柄、释放后句柄、重复 free | 这是 **实现级修复**，不是纯文档变更 |
| FFI `error_out` 现在必须非空 | **错误**。`error_out` 仍然是可选的；真正必须非空的是 `vldb_controller_ffi_client_create` / `_create_json` 的 `client_out` | 旧绑定如果传 `NULL error_out` 仍可工作；但 `client_out` 不能为 `NULL` |
| FFI 所有输出槽都必须提供 | **错误**。除 create 的 `client_out` 外，`response_out` / `result_out` / 其他输出槽仍然可以为 `NULL`，而且当前实现会跳过分配，避免泄漏 | 这对弱绑定语言是正向兼容改进 |
| gRPC 客户端“完全无影响” | protobuf 未变，但 **错误码语义和权限校验行为已变化** | 协议兼容，不代表行为完全不变 |
| 所有公共 API 签名未变 | **错误**。`ControllerClientConfig::endpoint_url()` 与 `bind_addr()` 已从 `String` 改为 `Result<String, BoxError>` | 直接调用这两个方法的 Rust 代码会有编译期适配成本 |
| 启动参数要求“所有超时值必须 > 0” | **错误**。当前仅 `default_lease_ttl_secs` 必须大于 `0`；`minimum_uptime_secs` 和 `idle_timeout_secs` 允许为 `0` | 若旧集成文档按“全部 >0”处理，需要修正文档 |
| `--bind :PORT` / `PORT` 只是解析支持 | 当前不仅解析支持，而且主程序已真正使用规范化后的 `bind_addr`，启动链路可用 | 旧版“解析通过但启动失败”的问题已修复 |
| IPv6 不支持 | **错误**。当前 CLI 绑定地址走 `SocketAddr` 解析，合法的 bracketed IPv6，如 `[::1]:19801`，已可被接受 | 若集成方需要 IPv6，可以纳入验证范围 |
| stale stream 回收只在 managed shutdown watcher 中执行 | **错误**。当前 `service` 和 `managed` 两种模式都会启动统一维护循环并执行流回收；`managed` 只是在此基础上额外支持自停 | 常驻服务模式下也有 stale stream 清理 |
| stale stream 是“固定创建时间 TTL” | **错误**。当前只回收 **已完成** 且 **空闲超过阈值** 的流，回收锚点取 `completed_at` 与 `last_accessed_at` 的较晚者 | 这避免了刚完成的长查询被立刻清掉 |

## 四、按使用方划分的影响面

### 1. 对 Rust SDK 使用者 (`vldb-controller-client`)

#### 1.1 直接兼容的部分

以下改动对绝大多数只通过 `ControllerClient` 调用控制器的使用者是向后兼容的：

- gRPC protobuf 协议未改
- `ControllerClient` 大多数业务方法签名保持稳定
- `BoxError` 仍然是 `Box<dyn Error + Send + Sync + 'static>`
- 新增 `VldbControllerError` 只是让错误可被更精细地分类
- `ControllerProcessMode`、`SpaceKind`、SQLite/LanceDB 相关枚举新增了 `to_proto_value()` / `from_proto_value()`，属于能力增强

#### 1.2 需要明确适配的部分

这次存在一个 **真实的公开 API 签名变化**：

| 方法 | 旧返回类型 | 新返回类型 | 影响 |
|------|------------|------------|------|
| `ControllerClientConfig::endpoint_url()` | `String` | `Result<String, BoxError>` | 直接调用者需要加 `?` / `expect` |
| `ControllerClientConfig::bind_addr()` | `String` | `Result<String, BoxError>` | 直接调用者需要加 `?` / `expect` |

如果下游代码显式调用了这两个方法，典型适配方式如下：

```rust
let endpoint = config.endpoint_url()?;
let bind_addr = config.bind_addr()?;
```

#### 1.3 auto-spawn 地址规则变化

这部分对自定义自动拉起逻辑的集成方尤其重要：

- `endpoint_url()` 现在会标准化 `PORT`、`:PORT`、`http://...`、`https://...`
- `bind_addr()` 现在只返回 **可被 controller 实际绑定的具体套接字地址**
- `localhost:PORT` 会在 auto-spawn 绑定场景下收敛为 `127.0.0.1:PORT`
- 非本地主机名，例如 `controller.internal:19811`，虽然仍可作为连接端点，但 **不能再被自动转换成 controller 的 bind 地址**

这意味着：

- 如果下游只是“连接已有 controller”，主机名端点仍然可用
- 如果下游依赖 `auto_spawn` 自动拉起本地 controller，就不应该把远端主机名拿去当 bind 地址

#### 1.4 错误类型与状态语义

当前 SDK 已统一收敛到 `VldbControllerError`，下游如果做精细错误匹配，建议改为：

```rust
use vldb_controller_client::VldbControllerError;

if let Some(ctrl_err) = error.downcast_ref::<VldbControllerError>() {
    match ctrl_err {
        VldbControllerError::InvalidInput(msg) => { /* 参数错误 */ }
        VldbControllerError::TransportError(source) => { /* 传输错误 */ }
        _ => {}
    }
}
```

### 2. 对 FFI 使用者（C / TypeScript / Go / Python）

#### 2.1 ABI 层面的兼容性

以下内容保持稳定：

- `extern "C"` 函数名未变
- `#[repr(C)]` 结构体布局未变
- 错误仍通过错误码 + `error_out` 组合返回

也就是说，**ABI 没变，行为变了**。

#### 2.2 FFI 句柄模型的真实变化

当前 FFI 句柄已不是旧版报告里描述的“仅文档说明线程安全”，而是实际改成：

- 真实堆地址做不透明句柄
- 全局注册表保存存活句柄状态
- 同一 handle 上的调用经互斥串行化
- `free` 时移除注册表并释放外层壳体
- 释放后的句柄再次调用会安全失败
- 伪造句柄指针会被拒绝，不会被当成有效会话

这对绑定层的意义是：

- 同一 handle 在多线程环境下更安全
- stale pointer 不再容易触发 UAF
- 手工构造“猜测句柄值”的绑定侧 bug 不再可能误打到真实实例

#### 2.3 FFI 输出槽约束的真实规则

当前规则应按下面理解：

| 接口类别 | 必填输出槽 | 说明 |
|----------|------------|------|
| `vldb_controller_ffi_client_create` | `client_out` 必填 | 为 `NULL` 会直接失败 |
| `vldb_controller_ffi_client_create_json` | `client_out` 必填 | 为 `NULL` 会直接失败 |
| 其他 `response_out` / `result_out` | 可选 | 为 `NULL` 时不分配输出对象，不泄漏 |
| `error_out` | 可选 | 为 `NULL` 仍可返回错误码，只是不回写错误字符串 |

这意味着旧版报告里“所有关键指针都要非空，尤其是 `error_out`”并不准确。

#### 2.4 需要集成方重点确认的行为变化

- 如果旧绑定曾传 `NULL client_out` 给 create/create_json，现在会稳定失败而不是走未定义行为
- 如果旧绑定省略 `response_out` / `result_out`，现在是安全的，不再有额外分配泄漏
- 同一 handle 重复 `free` 目前是幂等安全失败路径，不会再次解引用已释放壳体

### 3. 对 gRPC 客户端（任何语言）

#### 3.1 协议层兼容

以下内容保持不变：

- `controller.proto` 未修改
- 所有 RPC 名称和消息结构保持兼容
- protobuf 字段布局未变

#### 3.2 行为层变化

虽然协议兼容，但以下行为已经变化，集成方不能再按“完全无影响”理解：

#### A. 错误码语义更准确了

当前 `map_box_error()` 会按 `VldbControllerError` 做映射：

- `InvalidInput` / `ClientNotFound` / `SpaceNotFound` → `invalid_argument`
- `TransportError` → `unavailable`
- `BackendError` / `LockPoisoned` / 其他内部问题 → `internal`

这意味着如果下游曾把某些业务错误当成 `internal` 处理，现在需要留意它们已收敛为更准确的 `invalid_argument`。

#### B. owned space / stream 的访问被真正限制

当前 controller 已把 `client_id` 真正下沉到 runtime 数据面检查：

- 已归属 space 上的 SQLite / LanceDB 操作需要 attached client
- 已归属 stream 的 `wait_metrics` / `read_chunk` / `close` 需要 owner client
- 匿名访问已归属资源会被拒绝
- 非 owner / 非 attached client 访问会被拒绝

这对集成方的影响是：

- 如果以前存在“知道 `space_id` / `binding_id` 就直接打”的调用习惯，现在可能被拒绝
- 如果以前在 stream 后续请求里不传 `client_id`，现在对已归属流会失败

#### C. SQLite 路径的执行线程模型变了

SQLite 的脚本执行、查询、stream wait/chunk/close、分词、词典、FTS 等路径，当前都统一通过 `spawn_blocking` 包装，不再阻塞 tonic async worker。

这通常是纯正向变化：

- 不需要改调用方式
- 高并发下延迟更稳定
- 但如果下游监控过内部线程模型，需要更新预期

### 4. 对 Controller 启动与部署使用者

#### 4.1 bind 地址规则

当前 CLI 实际支持并能正确启动以下格式：

```bash
vldb-controller --bind 19801
vldb-controller --bind :19801
vldb-controller --bind 127.0.0.1:19801
vldb-controller --bind [::1]:19801
```

规范化结果如下：

| 输入 | 实际 bind_addr |
|------|----------------|
| `19801` | `127.0.0.1:19801` |
| `:19801` | `0.0.0.0:19801` |
| `127.0.0.1:19801` | 原样使用 |
| `[::1]:19801` | 原样使用 |

#### 4.2 超时与租约参数规则

旧版报告中“所有超时值必须大于 0”的说法已经不成立。当前规则是：

| 参数 | 当前规则 |
|------|----------|
| `minimum_uptime_secs` | 允许为 `0` |
| `idle_timeout_secs` | 允许为 `0` |
| `default_lease_ttl_secs` | 必须大于 `0` |
| `idle_timeout_secs >= minimum_uptime_secs` | 必须满足 |
| `default_lease_ttl_secs > idle_timeout_secs` | 仅警告，不阻止启动 |

#### 4.3 维护循环行为

当前运行时维护循环已经统一：

- `service` 模式会周期性清理过期 client 和 stale SQLite stream
- `managed` 模式在此基础上额外判断是否满足自停条件

因此旧版“只有 managed 才会处理 stale stream”的说法需要作废。

## 五、对集成方的实际影响等级

### 必须关注

| 项目 | 影响等级 | 原因 |
|------|----------|------|
| 直接调用 `endpoint_url()` / `bind_addr()` 的 Rust 代码 | 中 | 公开方法返回类型已改为 `Result` |
| 依赖匿名访问 owned space / owned stream 的 gRPC 客户端 | 中 | 现在会被权限校验拒绝 |
| auto-spawn 场景下把远端主机名当 bind 地址的使用方式 | 中 | 现在会被拒绝转换 |
| create/create_json 传 `NULL client_out` 的 FFI 调用方 | 中 | 现在稳定失败 |

### 建议关注

| 项目 | 影响等级 | 原因 |
|------|----------|------|
| 依赖旧 gRPC 状态码语义的客户端 | 低 | 业务输入错误现在更可能收到 `invalid_argument` |
| 依赖旧版报告配置规则的部署脚本 | 低 | `0` 秒边界与 IPv6 结论已变化 |
| FFI 绑定层自己缓存/伪造句柄值的异常实现 | 低 | 现在会被注册表拒绝 |

### 基本无感知或正向收益

| 项目 | 影响 |
|------|------|
| 仅通过 protobuf/gRPC 正常调用 controller 的客户端 | 协议层无改动 |
| 高并发 SQLite 使用场景 | 正向收益，async worker 不再被同步 I/O 直接占住 |
| service 模式长时间运行实例 | 正向收益，完成态 stale stream 现在能被回收 |

## 六、建议集成方执行的回归验证

建议至少补做以下 8 项验证：

1. Rust SDK 若直接调用 `endpoint_url()` / `bind_addr()`，确认已处理 `Result`
2. auto-spawn 场景分别验证 `19801`、`:19801`、`127.0.0.1:19801`、`localhost:19801`
3. 如需 IPv6，验证 `[::1]:PORT` 绑定与连接链路
4. 已归属 space 上，匿名 client 与非 attached client 的 SQLite / LanceDB 操作应被拒绝
5. 已归属 stream 上，匿名 client 与非 owner client 的 `wait/chunk/close` 应被拒绝
6. FFI create/create_json 传 `NULL client_out` 时应返回错误码而非崩溃
7. FFI 省略 `response_out` / `result_out` 时应无泄漏、无崩溃
8. 依赖 gRPC 状态码做重试或告警分类的逻辑，确认 `invalid_argument` / `unavailable` / `internal` 新语义

## 七、给集成方的简版结论

如果只用一句话概括当前未提交改动的下游影响：

> **协议层基本兼容，但行为层已经更严格、更准确，尤其是权限校验、错误语义、FFI 句柄安全和 auto-spawn 地址规则；其中唯一明确的 Rust SDK 编译期变更，是 `ControllerClientConfig::endpoint_url()` / `bind_addr()` 改成了返回 `Result`。**

## 八、当前验证结论

基于当前未提交工作树，已验证：

- `cargo test`：通过，**45/45**
- `cargo clippy --all-targets --all-features -- -D warnings`：通过
- `cargo check --all-targets`：通过

---

**报告日期**：2026-04-22  
**适用范围**：当前未提交工作树  
**用途**：提交给集成方做兼容性评估与接入改造排期

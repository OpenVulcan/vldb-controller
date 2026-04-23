# vldb-controller 架构说明

## 1. 项目定位

`vldb-controller` 是一个**空间级数据库控制服务**。

它的目标是**对外统一替代**：

- `vldb-sqlite`
- `vldb-lancedb`

的宿主接入面，同时在内部继续复用它们的数据库实现。  
它在此基础上增加一层稳定控制面，用于解决：

- 多进程共享同一数据库空间时的 ownership 问题
- 宿主需要共享或独立实例切换的问题
- backend 生命周期统一管理的问题
- 后续 `space_controller` 模式所需的统一协议问题

## 2. 设计原则

### 2.1 service-first

controller 的主形态是**独立服务**，而不是对外优先暴露 Rust API。

当前仓库采用 workspace 结构：

- `controller/`
  - 服务端 crate
- `client/`
  - 宿主接入 crate
- `ffi/`
  - 跨语言 ABI 导出 crate

其中 Rust 内核模块主要位于 `controller/`，用途是：

- 服务实现复用
- 单元测试
- 宿主侧 client SDK
- 后续客户端 SDK 与服务层共享逻辑

当前命名边界已经固定：

- `vldb-controller`
  - 控制服务进程
- `vldb_controller_client`
  - 宿主接入时使用的 Rust client SDK
- `vldb_controller_ffi`
  - 面向 TS / Go / Python / C 的 FFI 导出层

这种拆分的目的，是确保：

- `client` 不再直接链接 SQLite / LanceDB 重型依赖
- `controller` 独占数据库后端与生命周期控制逻辑
- `client` 只保留 Rust SDK 形态，不默认产出重量级动态库
- `ffi` 单独承担跨语言 ABI、JSON 模式与原生模式双接口

### 2.2 无配置文件

controller 默认不读取配置文件。  
它只接受：

- 启动参数
- gRPC 请求

这样可以让它保持轻量、明确且便于宿主托管。

### 2.3 共享与独立都必须支持

controller 不应强制全机唯一。

当前模型是：

- **同一个 bind 地址只允许一个实例**
- **不同 bind 地址允许并存多个实例**

默认共享地址：

- `127.0.0.1:19801`

如果宿主指定不同地址，例如：

- `127.0.0.1:19811`

则等同获得一个独立 controller。

### 2.4 生命周期不能依赖连接数

gRPC 的使用方式未必是长连接，因此：

- 不能用“当前有没有连接”判断 controller 是否应该退出

当前托管模式的关闭判定依赖：

- `client lease`
- 最近请求活动
- 执行中请求数量
- `minimum_uptime`
- `idle_timeout`

## 3. 两种运行模式

### 3.1 `service`

适合：

- 常驻服务
- 手动启动
- 系统托管

特点：

- 不会因空闲自动退出
- 生命周期由外部显式管理

### 3.2 `managed`

适合：

- IDE
- MCP
- CLI 工具链
- 宿主自动拉起

特点：

- 支持空闲自停
- 但退出条件不看连接数
- 必须满足：
  - 已超过最小存活时长
  - 没有活跃客户端租约
  - 没有执行中请求
  - 最近请求活动已超时

## 4. 空间模型

当前核心空间字段：

- `space_id`
- `space_label`
- `space_kind`
- `space_root`

说明：

- `space_id`：稳定唯一标识
- `space_label`：宿主提供的稳定标签
- `space_kind`：`root/user/project`
- `space_root`：宿主已解析的物理空间根路径

当前 controller 不负责猜项目标签。  
项目级标签被视为宿主前置条件，controller 只负责复用。

## 5. 客户端模型

每个调用方都可以注册一个 `client session`：

- `client_name`
- `host_kind`
- `process_id`
- `process_name`
- `lease_ttl_secs`

controller 在注册成功后会返回：

- `client_session_id`

当前用途：

- 维护租约与会话边界
- 记录空间附着关系
- 记录 backend owner
- 为托管模式空闲关闭判定提供依据
- 为后续进程探测与管理界面提供基础数据

补充说明：

- `client_name` 只用于诊断，可重复
- `client_session_id` 才是唯一会话主键
- 同名宿主不会被合并，也不会继承旧会话状态
- `attach_space` / `enable_sqlite` / `enable_lancedb` 都必须绑定到真实 `client_session_id`

## 6. 宿主代理层模型

当前仓库已经补上最小 `controller client` 代理层。

它的职责不是承载完整数据面，而是先解决宿主接入控制服务时最基础的几个问题：

- 连接指定 endpoint
- endpoint 不可用时自动唤起 controller
- 注册当前客户端
- 后台续约 lease
- 为宿主转发 attach / enable 等控制面请求

当前 client 设计遵循：

- endpoint 决定共享还是独立实例
- 自动唤起只针对当前 endpoint
- 不依赖“长连接一直保持”
- 续约依赖定时任务，而不是连接是否存在

也就是说：

- 默认 `127.0.0.1:19801` 表示共享 controller
- 自定义端口表示独立 controller
- 宿主不需要自己手动管理 controller 生命周期细节

## 7. FFI 导出层模型

当前 workspace 已新增：

- `ffi/`

它的职责不是承载 controller 服务，也不是链接 SQLite / LanceDB 后端，而是：

- 依赖 `client/`
- 对外导出稳定 C ABI
- 同时提供：
  - 原生结构化接口
  - `_json` 接口

这样后续：

- Rust 宿主使用 `client/`
- TS / Go / Python / C 等宿主使用 `ffi/`

边界会更清晰。

## 8. backend 模型

当前内建 backend：

- SQLite
- LanceDB

每个 space 都可以独立启停：

- `EnableSqlite`
- `DisableSqlite`
- `EnableLanceDb`
- `DisableLanceDb`

当前这层只负责：

- backend 生命周期
- 运行时实例管理
- 状态快照

当前 SQLite 内部能力面已经补齐：

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

当前 LanceDB 内部能力面也已经补齐：

- `create_table`
- `upsert`
- `search`
- `delete`
- `drop_table`

## 9. 当前协议边界

当前 gRPC 控制面包含：

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
- `ExecuteSqliteScript`
- `QuerySqliteJson`
- `EnableLanceDb`
- `DisableLanceDb`
- `CreateLanceDbTable`
- `UpsertLanceDb`
- `SearchLanceDb`

诊断接口补充约束：

- `ListClients` 仅用于诊断，不再暴露其他会话的真实 `client_session_id`
- `ListClients` 也不再暴露其他会话的 `attached_space_ids`
- `GetStatus` / `ListClients` / `ListSpaces` 这类纯诊断请求不会刷新托管自停的空闲窗口
- `DeleteLanceDb`

这表示当前版本重点是：

- 生命周期
- 控制面
- 空间与 backend 状态管理
- 先完成内部完整能力面，再统一升级外部协议

当前仍不是完整的对外数据总线，因为仍缺：

- 对外 gRPC / client / ffi 与完整能力面的统一对齐
- bm25 控制与数据面
- 更细粒度的 controller client 发现/重连状态机
- 进程探测与僵尸客户端清理

## 10. 与后续 `luaskills` 对接关系

当前预期的数据库模式有三类：

1. `dynamic_library`
2. `host_callback`
3. `space_controller`

`vldb-controller` 对应的就是第三类：

- `space_controller`

其目标是让 `luaskills` 后续在不改变上层 skill 数据访问语义的前提下，把底层数据库访问切换到：

- 共享 controller
- 或独立 controller

## 11. 后续优先事项

后续优先建议：

1. 增加进程探测与僵尸客户端清理
2. 引入 bm25 backend 控制面
3. 增加 controller client 的重连状态机与发现缓存
4. 增加宿主侧示例接入
5. 评估管理面状态查询与强制关闭接口

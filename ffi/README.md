# vldb-controller-ffi

`vldb-controller-ffi` 是 `vldb-controller` workspace 中专门面向跨语言宿主的 FFI 导出层。

它的职责不是实现 controller 服务，也不是承载 SQLite / LanceDB 后端，而是：

- 依赖 `vldb-controller-client`
- 对外导出稳定 C ABI
- 同时提供：
  - 原生结构化调用模式
  - JSON 调用模式

一句话说：

**`client/` 是 Rust SDK，`ffi/` 是跨语言 ABI 包装层。**

## 定位

当前 `ffi/` crate 面向：

- TypeScript / Node
- Go
- Python
- C / C++
- 其他能够动态加载 C ABI 的宿主

如果宿主本身是 Rust，优先直接依赖：

- [../client/README.md](../client/README.md)

如果宿主不是 Rust，优先依赖本 crate 生成的：

- `vldb_controller_ffi.dll`
- 或其他平台上的 `cdylib`

## 两种调用模式

### 1. 原生模式

原生模式适合：

- Go
- C / C++
- 有能力处理 ABI 结构体的高性能宿主

特点：

- 使用固定 `#[repr(C)]` 对应结构
- 返回原生结果指针
- 需要宿主调用配套 `*_free` 函数释放内存

### 2. JSON 模式

JSON 模式适合：

- Python
- TypeScript / Node
- 原型验证与快速接入

特点：

- 请求输入是 JSON 字符串
- 返回输出也是 JSON 字符串
- 宿主只需要处理 UTF-8 文本与 `string_free`
- 数据面请求直接使用嵌套 JSON 结构，不需要再把内层参数手工序列化成 JSON 字符串

## 当前导出能力

当前 FFI 已经覆盖完整控制面与完整 SQLite / LanceDB 数据面：

- client 创建与释放
- controller 连接
- controller 关闭
- 获取 controller 状态
- attach space
- detach space
- list spaces
- 启用 / 关闭 SQLite backend
- SQLite `execute_script`
- SQLite `execute_batch`
- SQLite `query_json`
- SQLite `query_stream`
- SQLite `tokenize_text`
- SQLite `list_custom_words`
- SQLite `upsert_custom_word`
- SQLite `remove_custom_word`
- SQLite `ensure_fts_index`
- SQLite `rebuild_fts_index`
- SQLite `upsert_fts_document`
- SQLite `delete_fts_document`
- SQLite `search_fts`
- 启用 / 关闭 LanceDB backend
- LanceDB `create_table`
- LanceDB `upsert`
- LanceDB `search`
- LanceDB `delete`
- LanceDB `drop_table`

并且上述能力都同时提供：

- 原生接口
- `_json` 接口

对于 SQLite `_json` 参数模式，二进制 blob 需要使用稳定包装对象：

```json
{"type":"bytes_base64","base64":"..."}
```

或：

```json
{"__type":"bytes_base64","base64":"..."}
```

普通标量仍然直接使用：

- `null`
- `bool`
- `number`
- `string`

SQLite `_json` 请求中的 `params` / `batch_params` 现在都应直接传 JSON 数组：

```json
{
  "space_id": "ROOT",
  "sql": "INSERT INTO demo(value_blob, value_text) VALUES (?, ?)",
  "params": [
    {"type":"bytes_base64","base64":"AQID"},
    "  keep whitespace  "
  ]
}
```

LanceDB `_json` 请求中的建表、检索、删除参数也应直接作为嵌套对象传入，而不是再使用 `request_json` 字符串包装。

## 常量协议

当前 FFI 稳定整数协议如下：

### process mode

- `0 => service`
- `1 => managed`

### space kind

- `0 => root`
- `1 => user`
- `2 => project`

具体常量与结构声明见：

- [include/vldb_controller_ffi.h](include/vldb_controller_ffi.h)

## 输出文件

本 crate 当前会生成：

- Rust 用的 `rlib`
- 跨语言用的 `cdylib`

也就是说：

- Rust 宿主应优先依赖 `client/`
- 非 Rust 宿主应优先依赖 `ffi/`

## 构建

### 调试构建

```powershell
cargo build -p vldb-controller-ffi
```

### 发布构建

```powershell
cargo build -p vldb-controller-ffi --release
```

## 头文件

当前 FFI 头文件位于：

- [include/vldb_controller_ffi.h](include/vldb_controller_ffi.h)

头文件中已经定义：

- 原生请求结构
- 原生返回结构
- 稳定模式常量
- 所有导出函数声明
- 内存释放函数声明

## 使用建议

### Rust 宿主

优先使用：

- `vldb-controller-client`

原因：

- 类型更自然
- 无需手动管理 ABI 指针
- 更适合 Rust 生态

### 非 Rust 宿主

优先使用：

- `vldb-controller-ffi`

如果宿主偏动态语言：

- 优先 `_json` 接口

如果宿主偏静态语言且追求性能：

- 优先原生接口

即便如此，仍建议宿主在释放 client 句柄前显式调用 `shutdown`，以便更快释放 controller 端的租约；`*_free` 现在会做一次 best-effort 注销兜底，但不应取代显式关闭。

## 当前边界

当前仍未补齐的主要是：

- TS / Go / Python 的独立示例工程
- 更高层的自动发现 / 自动唤起 FFI 包装示例

但 ABI 与控制面 / 数据面能力本身已经完整到位。

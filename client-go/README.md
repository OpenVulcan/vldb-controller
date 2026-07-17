# vldb-controller client-go

`client-go/controller` 是不依赖 FFI/cgo 的 Go 原生 SDK。它直接使用 protobuf/gRPC 连接 `vldb-controller`，并在 Go 层实现：

- controller 连接与注册
- 后台租约续期
- 客户端租约列表查询
- controller 丢失后的重连与期望状态重放
- controller 不可达时自动启动普通进程
- SQLite 与 LanceDB 数据面调用包装

示例目录：

```text
examples/basic
examples/sqlite_native
examples/smoke_sqlite_native
```

生成 protobuf：

```powershell
$env:PATH = "$(go env GOPATH)\bin;$env:PATH"
.\scripts\generate_proto.ps1
```

# client-go 原生 SDK 对接说明

## 定位

`client-go/controller` 是不依赖 FFI/cgo 的 Go 原生 SDK。它直接通过 protobuf/gRPC 连接 `vldb-controller`，适合希望避免动态库分发、cgo 交叉编译成本和跨语言内存管理的 Go 宿主。

## 当前能力

client-go 当前覆盖：

1. controller endpoint 连接。
2. client session 注册、续租、注销与客户端租约列表查询。
3. endpoint 不可达时按配置自动启动普通 controller 进程。
4. 连接成功后读取 `GetStatus`，确认端点确实是 controller。
5. controller 丢失后的重连与期望状态重放。
6. space attach / detach / list。
7. SQLite 控制面、查询、批量、stream、分词、自定义词和 FTS 能力。
8. LanceDB enable / disable / create / upsert / search / delete / drop 能力。

## 默认启动行为

默认 `Config` 使用：

```text
AutoSpawn = true
SpawnProcessMode = managed
```

含义：

1. 先尝试连接现有 endpoint 并读取 `GetStatus`。
2. 只要 endpoint 上存在合法 controller，就直接共享该实例。
3. endpoint 不可达且允许自动启动时，启动配置的普通 controller 进程。
4. 设置 `AutoSpawn = false` 可以只连接现有实例。

## protobuf 生成

首次开发或协议变更后执行：

```powershell
$env:PATH = "$(go env GOPATH)\bin;$env:PATH"
.\client-go\scripts\generate_proto.ps1
```

生成代码位于：

```text
client-go/v1
```

## 基础用法

```go
package main

import (
	"context"
	"os"

	"github.com/OpenVulcan/vldb-controller/client-go/controller"
)

func main() {
	ctx := context.Background()
	client := controller.New(controller.DefaultConfig(), controller.ClientRegistration{
		ClientName:  "example-go-host",
		HostKind:    "go",
		ProcessID:   uint32(os.Getpid()),
		ProcessName: "example",
	})
	if err := client.Connect(ctx); err != nil {
		panic(err)
	}
	defer client.Shutdown(ctx)
}
```

## 示例

当前示例均使用 `client-go/controller` 原生类型，不直接依赖 pb 包：

```text
client-go/examples/basic
client-go/examples/sqlite_native
client-go/examples/smoke_sqlite_native
```

## 注意事项

1. client-go 公开 API 使用 Go 原生结构体，生成的 protobuf 类型仅作为内部协议层使用。
2. client-go 会维护 desired state，用于 controller 重启后的自动重放。
3. client-go 只接受能返回 `GetStatus` 的 controller 端点，不把“端口可连”当成就绪。
4. client-go 不包含操作系统服务注册、发现或启动逻辑。

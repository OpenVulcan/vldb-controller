package main

import (
	"context"
	"os"
	"path/filepath"
	"time"

	"github.com/OpenVulcan/vldb-controller/client-go/controller"
)

// main demonstrates SQLite calls through client-go native SDK types.
// main 演示通过 client-go 原生 SDK 类型调用 SQLite。
func main() {
	ctx, cancel := context.WithTimeout(context.Background(), 30*time.Second)
	defer cancel()

	config := controller.DefaultConfig()
	client := controller.New(config, controller.ClientRegistration{
		ClientName:  "client-go-sqlite-example",
		HostKind:    "go",
		ProcessID:   uint32(os.Getpid()),
		ProcessName: "client-go-sqlite",
		LeaseTTL:    2 * time.Minute,
	})
	defer func() {
		_ = client.Shutdown(context.Background())
	}()

	if err := client.Connect(ctx); err != nil {
		panic(err)
	}
	if _, err := client.AttachSpace(ctx, controller.SpaceRegistration{
		SpaceID:    "example-space",
		SpaceLabel: "Example Space",
		SpaceKind:  controller.SpaceKindProject,
		SpaceRoot:  os.TempDir(),
	}); err != nil {
		panic(err)
	}
	if err := client.EnableSqlite(ctx, &controller.SqliteEnableRequest{
		SpaceID:            "example-space",
		BindingID:          "main",
		DBPath:             filepath.Join(os.TempDir(), "vldb-controller-client-go-example.sqlite"),
		ConnectionPoolSize: 4,
		BusyTimeoutMs:      5000,
		JournalMode:        "WAL",
		Synchronous:        "NORMAL",
		ForeignKeys:        true,
		TempStore:          "MEMORY",
		EnforceDBFileLock:  true,
		TrustedSchema:      false,
		Defensive:          true,
	}); err != nil {
		panic(err)
	}
	if _, err := client.ExecuteSqliteScript(ctx, &controller.SqliteExecuteScriptRequest{
		SpaceID:   "example-space",
		BindingID: "main",
		SQL:       "CREATE TABLE IF NOT EXISTS notes (id INTEGER PRIMARY KEY, body TEXT);",
	}); err != nil {
		panic(err)
	}
}

package main

import (
	"context"
	"encoding/json"
	"fmt"
	"os"
	"path/filepath"
	"time"

	"github.com/OpenVulcan/vldb-controller/client-go/controller"
)

// queryRow stores one SQLite smoke-test row returned as JSON.
// queryRow 存储一条以 JSON 返回的 SQLite 冒烟测试行。
type queryRow struct {
	// ID is the inserted row identifier.
	// ID 是插入行标识符。
	ID int64 `json:"id"`
	// Body is the inserted note body.
	// Body 是插入的笔记正文。
	Body string `json:"body"`
}

// main verifies data creation and reading through the Go-native SDK.
// main 通过 Go 原生 SDK 验证数据创建与读取。
func main() {
	ctx, cancel := context.WithTimeout(context.Background(), 45*time.Second)
	defer cancel()

	executable := os.Getenv("VLDB_CONTROLLER_EXE")
	if executable == "" {
		executable = "vldb-controller"
	}
	endpoint := os.Getenv("VLDB_CONTROLLER_ENDPOINT")
	if endpoint == "" {
		endpoint = "http://127.0.0.1:19831"
	}

	tempRoot := os.TempDir()
	dbPath := filepath.Join(tempRoot, fmt.Sprintf("vldb-controller-client-go-smoke-%d.sqlite", time.Now().UnixNano()))
	defer func() {
		_ = os.Remove(dbPath)
		_ = os.Remove(dbPath + "-wal")
		_ = os.Remove(dbPath + "-shm")
	}()

	config := controller.DefaultConfig()
	config.Endpoint = endpoint
	config.SpawnExecutable = executable
	config.MinimumUptime = 2 * time.Second
	config.IdleTimeout = 3 * time.Second
	config.StartupTimeout = 20 * time.Second

	client := controller.New(config, controller.ClientRegistration{
		ClientName:  "client-go-smoke-sqlite",
		HostKind:    "go",
		ProcessID:   uint32(os.Getpid()),
		ProcessName: "client-go-smoke-sqlite",
		LeaseTTL:    30 * time.Second,
	})
	defer func() {
		_ = client.Shutdown(context.Background())
	}()

	if err := client.Connect(ctx); err != nil {
		panic(err)
	}
	status, err := client.GetStatus(ctx)
	if err != nil {
		panic(err)
	}
	if expected := processModeFromEnv(controller.ProcessModeManaged); status.ProcessMode != expected {
		panic(fmt.Sprintf("unexpected process mode: %s", status.ProcessMode))
	}
	clients, err := client.ListClients(ctx)
	if err != nil {
		panic(err)
	}
	if len(clients) == 0 {
		panic("expected at least one registered client")
	}

	if _, err := client.AttachSpace(ctx, controller.SpaceRegistration{
		SpaceID:    "smoke-space",
		SpaceLabel: "Smoke Space",
		SpaceKind:  controller.SpaceKindProject,
		SpaceRoot:  tempRoot,
	}); err != nil {
		panic(err)
	}
	if err := client.EnableSqlite(ctx, &controller.SqliteEnableRequest{
		SpaceID:            "smoke-space",
		BindingID:          "main",
		DBPath:             dbPath,
		ConnectionPoolSize: 2,
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
		SpaceID:   "smoke-space",
		BindingID: "main",
		SQL:       "CREATE TABLE IF NOT EXISTS notes (id INTEGER PRIMARY KEY, body TEXT NOT NULL);",
	}); err != nil {
		panic(err)
	}
	if _, err := client.ExecuteSqliteBatch(ctx, &controller.SqliteExecuteBatchRequest{
		SpaceID:   "smoke-space",
		BindingID: "main",
		SQL:       "INSERT INTO notes(body) VALUES (?);",
		Items: []controller.SqliteExecuteBatchItem{
			{Params: []controller.SqliteValue{{Kind: controller.SqliteValueString, StringValue: "hello from client-go smoke"}}},
		},
	}); err != nil {
		panic(err)
	}
	query, err := client.QuerySqliteJSON(ctx, &controller.SqliteQueryJSONRequest{
		SpaceID:   "smoke-space",
		BindingID: "main",
		SQL:       "SELECT id, body FROM notes ORDER BY id;",
	})
	if err != nil {
		panic(err)
	}
	var rows []queryRow
	if err := json.Unmarshal([]byte(query.JSONData), &rows); err != nil {
		panic(err)
	}
	if query.RowCount != 1 || len(rows) != 1 || rows[0].Body != "hello from client-go smoke" {
		panic(fmt.Sprintf("unexpected query result: row_count=%d rows=%+v", query.RowCount, rows))
	}

	fmt.Printf("smoke ok endpoint=%s process_mode=%s clients=%d rows=%d body=%q\n", status.BindAddr, status.ProcessMode, len(clients), query.RowCount, rows[0].Body)
}

// processModeFromEnv resolves the smoke-test expected process mode.
// processModeFromEnv 解析冒烟测试预期进程模式。
func processModeFromEnv(defaultMode controller.ProcessMode) controller.ProcessMode {
	switch os.Getenv("VLDB_CONTROLLER_EXPECT_PROCESS_MODE") {
	case "":
		return defaultMode
	case "managed":
		return controller.ProcessModeManaged
	case "service":
		return controller.ProcessModeService
	default:
		panic(fmt.Sprintf("unsupported VLDB_CONTROLLER_EXPECT_PROCESS_MODE: %s", os.Getenv("VLDB_CONTROLLER_EXPECT_PROCESS_MODE")))
	}
}

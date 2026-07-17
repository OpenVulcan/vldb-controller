package main

import (
	"context"
	"fmt"
	"os"
	"time"

	"github.com/OpenVulcan/vldb-controller/client-go/controller"
)

// main connects through the Go-native SDK and prints the controller status.
// main 通过 Go 原生 SDK 连接并打印 controller 状态。
func main() {
	ctx, cancel := context.WithTimeout(context.Background(), 20*time.Second)
	defer cancel()

	config := controller.DefaultConfig()
	client := controller.New(config, controller.ClientRegistration{
		ClientName:  "client-go-basic-example",
		HostKind:    "go",
		ProcessID:   uint32(os.Getpid()),
		ProcessName: "client-go-basic",
		LeaseTTL:    2 * time.Minute,
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
	fmt.Printf("controller=%s process_mode=%s active_clients=%d\n", status.BindAddr, status.ProcessMode, status.ActiveClients)
}

package controller

import "testing"

// TestDefaultConfigUsesManagedAutoSpawn verifies the shared-process startup defaults.
// TestDefaultConfigUsesManagedAutoSpawn 验证共享进程的默认启动配置。
func TestDefaultConfigUsesManagedAutoSpawn(t *testing.T) {
	config := DefaultConfig()

	if !config.AutoSpawn {
		t.Fatal("expected automatic startup to be enabled")
	}
	if config.SpawnProcessMode != ProcessModeManaged {
		t.Fatalf("expected managed process mode, got %s", config.SpawnProcessMode)
	}
}

// TestBindAddressNormalizesLocalhost verifies local endpoint bind conversion.
// TestBindAddressNormalizesLocalhost 验证本地端点绑定转换。
func TestBindAddressNormalizesLocalhost(t *testing.T) {
	bind, err := bindAddress("http://localhost:19801")
	if err != nil {
		t.Fatalf("bind address should normalize: %v", err)
	}
	if bind != "127.0.0.1:19801" {
		t.Fatalf("unexpected bind address: %s", bind)
	}
}

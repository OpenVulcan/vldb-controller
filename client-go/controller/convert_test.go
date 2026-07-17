package controller

import (
	"reflect"
	"testing"

	pb "github.com/OpenVulcan/vldb-controller/client-go/v1"
)

// TestMapClientLeaseSnapshot verifies native client lease mapping.
// TestMapClientLeaseSnapshot 验证原生客户端租约映射。
func TestMapClientLeaseSnapshot(t *testing.T) {
	snapshot := mapClientLeaseSnapshot(&pb.ClientLeaseSnapshot{
		ClientSessionId:  "session-1",
		ClientName:       "go-host",
		HostKind:         "go",
		ProcessId:        42,
		ProcessName:      "go-host.exe",
		LastSeenUnixMs:   1000,
		ExpiresAtUnixMs:  2000,
		AttachedSpaceIds: []string{"root", "project"},
	})

	if snapshot == nil {
		t.Fatal("expected mapped client lease snapshot")
	}
	if snapshot.ClientSessionID != "session-1" {
		t.Fatalf("unexpected client session id: %s", snapshot.ClientSessionID)
	}
	if snapshot.ClientName != "go-host" || snapshot.HostKind != "go" {
		t.Fatalf("unexpected client identity: %+v", snapshot)
	}
	if snapshot.ProcessID != 42 || snapshot.ProcessName != "go-host.exe" {
		t.Fatalf("unexpected process identity: %+v", snapshot)
	}
	if snapshot.LastSeenUnixMs != 1000 || snapshot.ExpiresAtUnixMs != 2000 {
		t.Fatalf("unexpected lease timestamps: %+v", snapshot)
	}
	if !reflect.DeepEqual(snapshot.AttachedSpaceIDs, []string{"root", "project"}) {
		t.Fatalf("unexpected attached spaces: %#v", snapshot.AttachedSpaceIDs)
	}
}

// TestMapClientLeaseSnapshotCopiesAttachedSpaces verifies slice ownership.
// TestMapClientLeaseSnapshotCopiesAttachedSpaces 验证附着空间切片所有权。
func TestMapClientLeaseSnapshotCopiesAttachedSpaces(t *testing.T) {
	source := &pb.ClientLeaseSnapshot{AttachedSpaceIds: []string{"space-a"}}
	snapshot := mapClientLeaseSnapshot(source)
	source.AttachedSpaceIds[0] = "space-b"

	if snapshot.AttachedSpaceIDs[0] != "space-a" {
		t.Fatalf("expected attached spaces to be copied, got %#v", snapshot.AttachedSpaceIDs)
	}
}

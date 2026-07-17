package controller

import (
	"context"
	"errors"
	"fmt"
	"net"
	"net/url"
	"os/exec"
	"strings"
	"sync"
	"time"

	pb "github.com/OpenVulcan/vldb-controller/client-go/v1"
	"google.golang.org/grpc"
	"google.golang.org/grpc/codes"
	"google.golang.org/grpc/credentials/insecure"
	"google.golang.org/grpc/status"
)

// Client is a Go-native controller SDK client.
// Client 是 Go 原生 controller SDK 客户端。
type Client struct {
	// mu protects connection, session, desired state, and renewer fields.
	// mu 保护连接、会话、期望状态和续租任务字段。
	mu sync.Mutex
	// config stores normalized endpoint and launch settings.
	// config 保存标准化后的端点与启动设置。
	config Config
	// registration stores host registration fields.
	// registration 保存宿主注册字段。
	registration ClientRegistration
	// conn is the active gRPC client connection.
	// conn 是当前活跃 gRPC 客户端连接。
	conn *grpc.ClientConn
	// rpc is the generated protobuf gRPC client.
	// rpc 是生成的 protobuf gRPC 客户端。
	rpc pb.ControllerServiceClient
	// clientSessionID is the controller-assigned session identifier.
	// clientSessionID 是 controller 分配的会话标识符。
	clientSessionID string
	// renewCancel stops the background lease renewer.
	// renewCancel 停止后台租约续期任务。
	renewCancel context.CancelFunc
	// attachedSpaces records desired space attachments for replay.
	// attachedSpaces 记录用于重放的期望空间附着。
	attachedSpaces map[string]SpaceRegistration
	// sqliteBindings records desired SQLite bindings for replay.
	// sqliteBindings 记录用于重放的期望 SQLite 绑定。
	sqliteBindings map[sqliteBindingKey]*SqliteEnableRequest
	// lancedbBindings records desired LanceDB bindings for replay.
	// lancedbBindings 记录用于重放的期望 LanceDB 绑定。
	lancedbBindings map[lancedbBindingKey]*LanceDBEnableRequest
}

// New creates one Go-native controller SDK client.
// New 创建一个 Go 原生 controller SDK 客户端。
func New(config Config, registration ClientRegistration) *Client {
	config = config.normalized()
	return &Client{
		config:          config,
		registration:    registration,
		attachedSpaces:  map[string]SpaceRegistration{},
		sqliteBindings:  map[sqliteBindingKey]*SqliteEnableRequest{},
		lancedbBindings: map[lancedbBindingKey]*LanceDBEnableRequest{},
	}
}

// Connect ensures the controller is reachable, registers a session, and starts renewal.
// Connect 确保 controller 可达、注册会话并启动续租。
func (c *Client) Connect(ctx context.Context) error {
	if err := c.ensureReady(ctx); err != nil {
		return err
	}
	if err := c.ensureConnected(ctx); err != nil {
		return err
	}
	if err := c.ensureRegistered(ctx); err != nil {
		return err
	}
	c.ensureRenewer()
	return nil
}

// Shutdown unregisters the current session and closes the connection.
// Shutdown 注销当前会话并关闭连接。
func (c *Client) Shutdown(ctx context.Context) error {
	c.mu.Lock()
	cancel := c.renewCancel
	c.renewCancel = nil
	rpc := c.rpc
	sessionID := c.clientSessionID
	conn := c.conn
	c.clientSessionID = ""
	c.conn = nil
	c.rpc = nil
	c.attachedSpaces = map[string]SpaceRegistration{}
	c.sqliteBindings = map[sqliteBindingKey]*SqliteEnableRequest{}
	c.lancedbBindings = map[lancedbBindingKey]*LanceDBEnableRequest{}
	c.mu.Unlock()

	if cancel != nil {
		cancel()
	}
	if rpc != nil && sessionID != "" {
		_, _ = rpc.UnregisterClient(ctx, &pb.UnregisterClientRequest{ClientSessionId: sessionID})
	}
	if conn != nil {
		return conn.Close()
	}
	return nil
}

// GetStatus returns the current controller status without auto-spawning a new controller.
// GetStatus 返回当前 controller 状态且不会自动拉起新的 controller。
func (c *Client) GetStatus(ctx context.Context) (*StatusSnapshot, error) {
	if err := c.ensureConnected(ctx); err != nil {
		return nil, err
	}
	c.mu.Lock()
	rpc := c.rpc
	c.mu.Unlock()
	response, err := rpc.GetStatus(ctx, &pb.GetStatusRequest{})
	if err != nil {
		return nil, err
	}
	return mapStatusSnapshot(response.Status)
}

// ListClients lists controller client leases visible to the current session.
// ListClients 列出当前会话可见的 controller 客户端租约。
func (c *Client) ListClients(ctx context.Context) ([]*ClientLeaseSnapshot, error) {
	var clients []*ClientLeaseSnapshot
	err := c.withSession(ctx, func(rpc pb.ControllerServiceClient, sessionID string) error {
		response, err := rpc.ListClients(ctx, &pb.ListClientsRequest{ClientSessionId: sessionID})
		if err != nil {
			return err
		}
		clients = make([]*ClientLeaseSnapshot, 0, len(response.Clients))
		for _, client := range response.Clients {
			clients = append(clients, mapClientLeaseSnapshot(client))
		}
		return nil
	})
	return clients, err
}

// AttachSpace attaches one runtime space and remembers it for recovery.
// AttachSpace 附着一个运行时空间并记录用于恢复。
func (c *Client) AttachSpace(ctx context.Context, registration SpaceRegistration) (*SpaceSnapshot, error) {
	var snapshot *SpaceSnapshot
	err := c.withSession(ctx, func(rpc pb.ControllerServiceClient, sessionID string) error {
		response, err := rpc.AttachSpace(ctx, registration.toProto(sessionID))
		if err != nil {
			return err
		}
		snapshot = mapSpaceSnapshot(response.Space)
		c.mu.Lock()
		c.attachedSpaces[registration.SpaceID] = registration
		c.mu.Unlock()
		return nil
	})
	return snapshot, err
}

// DetachSpace detaches one runtime space and forgets it from recovery state.
// DetachSpace 解除一个运行时空间附着并从恢复状态中遗忘。
func (c *Client) DetachSpace(ctx context.Context, spaceID string) (bool, error) {
	var detached bool
	err := c.withSession(ctx, func(rpc pb.ControllerServiceClient, sessionID string) error {
		response, err := rpc.DetachSpace(ctx, &pb.DetachSpaceRequest{ClientSessionId: sessionID, SpaceId: spaceID})
		if err != nil {
			return err
		}
		detached = response.Detached
		c.mu.Lock()
		delete(c.attachedSpaces, spaceID)
		c.mu.Unlock()
		return nil
	})
	return detached, err
}

// ListSpaces lists spaces visible to the current session.
// ListSpaces 列出当前会话可见的空间。
func (c *Client) ListSpaces(ctx context.Context) ([]*SpaceSnapshot, error) {
	var spaces []*SpaceSnapshot
	err := c.withSession(ctx, func(rpc pb.ControllerServiceClient, sessionID string) error {
		response, err := rpc.ListSpaces(ctx, &pb.ListSpacesRequest{ClientSessionId: sessionID})
		if err != nil {
			return err
		}
		spaces = make([]*SpaceSnapshot, 0, len(response.Spaces))
		for _, space := range response.Spaces {
			spaces = append(spaces, mapSpaceSnapshot(space))
		}
		return nil
	})
	return spaces, err
}

// EnableSqlite enables one SQLite backend and remembers it for recovery.
// EnableSqlite 启用一个 SQLite 后端并记录用于恢复。
func (c *Client) EnableSqlite(ctx context.Context, request *SqliteEnableRequest) error {
	return c.withSession(ctx, func(rpc pb.ControllerServiceClient, sessionID string) error {
		req := request.clone()
		if _, err := rpc.EnableSqlite(ctx, req.toProto(sessionID)); err != nil {
			return err
		}
		c.mu.Lock()
		c.sqliteBindings[sqliteBindingKey{SpaceID: req.SpaceID, BindingID: req.BindingID}] = req.clone()
		c.mu.Unlock()
		return nil
	})
}

// DisableSqlite disables one SQLite backend and forgets it from recovery state.
// DisableSqlite 关闭一个 SQLite 后端并从恢复状态中遗忘。
func (c *Client) DisableSqlite(ctx context.Context, spaceID string, bindingID string) (bool, error) {
	var disabled bool
	err := c.withSession(ctx, func(rpc pb.ControllerServiceClient, sessionID string) error {
		response, err := rpc.DisableSqlite(ctx, &pb.DisableBackendRequest{ClientSessionId: sessionID, SpaceId: spaceID, BindingId: bindingID})
		if err != nil {
			return err
		}
		disabled = response.Disabled
		c.mu.Lock()
		delete(c.sqliteBindings, sqliteBindingKey{SpaceID: spaceID, BindingID: bindingID})
		c.mu.Unlock()
		return nil
	})
	return disabled, err
}

// ExecuteSqliteScript executes one SQLite script request.
// ExecuteSqliteScript 执行一个 SQLite 脚本请求。
func (c *Client) ExecuteSqliteScript(ctx context.Context, request *SqliteExecuteScriptRequest) (*SqliteExecuteScriptResponse, error) {
	var response *SqliteExecuteScriptResponse
	err := c.withSession(ctx, func(rpc pb.ControllerServiceClient, sessionID string) error {
		var err error
		raw, err := rpc.ExecuteSqliteScript(ctx, request.toProto(sessionID))
		response = mapSqliteExecuteScriptResponse(raw)
		return err
	})
	return response, err
}

// ExecuteSqliteBatch executes one SQLite batch request.
// ExecuteSqliteBatch 执行一个 SQLite 批量请求。
func (c *Client) ExecuteSqliteBatch(ctx context.Context, request *SqliteExecuteBatchRequest) (*SqliteExecuteBatchResponse, error) {
	var response *SqliteExecuteBatchResponse
	err := c.withSession(ctx, func(rpc pb.ControllerServiceClient, sessionID string) error {
		var err error
		raw, err := rpc.ExecuteSqliteBatch(ctx, request.toProto(sessionID))
		response = mapSqliteExecuteBatchResponse(raw)
		return err
	})
	return response, err
}

// QuerySqliteJSON executes one SQLite JSON query request.
// QuerySqliteJSON 执行一个 SQLite JSON 查询请求。
func (c *Client) QuerySqliteJSON(ctx context.Context, request *SqliteQueryJSONRequest) (*SqliteQueryJSONResponse, error) {
	var response *SqliteQueryJSONResponse
	err := c.withSession(ctx, func(rpc pb.ControllerServiceClient, sessionID string) error {
		var err error
		raw, err := rpc.QuerySqliteJson(ctx, request.toProto(sessionID))
		response = mapSqliteQueryJSONResponse(raw)
		return err
	})
	return response, err
}

// QuerySqliteStream opens one SQLite query stream.
// QuerySqliteStream 打开一个 SQLite 查询流。
func (c *Client) QuerySqliteStream(ctx context.Context, request *SqliteQueryStreamRequest) (*SqliteQueryStreamResponse, error) {
	var response *SqliteQueryStreamResponse
	err := c.withSession(ctx, func(rpc pb.ControllerServiceClient, sessionID string) error {
		var err error
		raw, err := rpc.QuerySqliteStream(ctx, request.toProto(sessionID))
		response = mapSqliteQueryStreamResponse(raw)
		return err
	})
	return response, err
}

// QuerySqliteStreamWaitMetrics waits for stream metrics.
// QuerySqliteStreamWaitMetrics 等待查询流指标。
func (c *Client) QuerySqliteStreamWaitMetrics(ctx context.Context, request *SqliteQueryStreamWaitMetricsRequest) (*SqliteQueryStreamWaitMetricsResponse, error) {
	var response *SqliteQueryStreamWaitMetricsResponse
	err := c.withSession(ctx, func(rpc pb.ControllerServiceClient, sessionID string) error {
		var err error
		raw, err := rpc.QuerySqliteStreamWaitMetrics(ctx, request.toProto(sessionID))
		response = mapSqliteQueryStreamWaitMetricsResponse(raw)
		return err
	})
	return response, err
}

// QuerySqliteStreamChunk reads one stream chunk.
// QuerySqliteStreamChunk 读取一个查询流分块。
func (c *Client) QuerySqliteStreamChunk(ctx context.Context, request *SqliteQueryStreamChunkRequest) (*SqliteQueryStreamChunkResponse, error) {
	var response *SqliteQueryStreamChunkResponse
	err := c.withSession(ctx, func(rpc pb.ControllerServiceClient, sessionID string) error {
		var err error
		raw, err := rpc.QuerySqliteStreamChunk(ctx, request.toProto(sessionID))
		response = mapSqliteQueryStreamChunkResponse(raw)
		return err
	})
	return response, err
}

// QuerySqliteStreamClose closes one stream.
// QuerySqliteStreamClose 关闭一个查询流。
func (c *Client) QuerySqliteStreamClose(ctx context.Context, request *SqliteQueryStreamCloseRequest) (*SqliteQueryStreamCloseResponse, error) {
	var response *SqliteQueryStreamCloseResponse
	err := c.withSession(ctx, func(rpc pb.ControllerServiceClient, sessionID string) error {
		var err error
		raw, err := rpc.QuerySqliteStreamClose(ctx, request.toProto(sessionID))
		response = mapSqliteQueryStreamCloseResponse(raw)
		return err
	})
	return response, err
}

// TokenizeSqliteText tokenizes text through the SQLite backend.
// TokenizeSqliteText 通过 SQLite 后端对文本分词。
func (c *Client) TokenizeSqliteText(ctx context.Context, request *SqliteTokenizeTextRequest) (*SqliteTokenizeTextResponse, error) {
	var response *SqliteTokenizeTextResponse
	err := c.withSession(ctx, func(rpc pb.ControllerServiceClient, sessionID string) error {
		var err error
		raw, err := rpc.TokenizeSqliteText(ctx, request.toProto(sessionID))
		response = mapSqliteTokenizeTextResponse(raw)
		return err
	})
	return response, err
}

// ListSqliteCustomWords lists custom tokenizer words.
// ListSqliteCustomWords 列出自定义分词词条。
func (c *Client) ListSqliteCustomWords(ctx context.Context, request *SqliteBackendRequest) (*SqliteListCustomWordsResponse, error) {
	var response *SqliteListCustomWordsResponse
	err := c.withSession(ctx, func(rpc pb.ControllerServiceClient, sessionID string) error {
		var err error
		raw, err := rpc.ListSqliteCustomWords(ctx, request.toProto(sessionID))
		response = mapSqliteListCustomWordsResponse(raw)
		return err
	})
	return response, err
}

// UpsertSqliteCustomWord inserts or updates one custom tokenizer word.
// UpsertSqliteCustomWord 新增或更新一个自定义分词词条。
func (c *Client) UpsertSqliteCustomWord(ctx context.Context, request *SqliteCustomWordRequest) (*SqliteDictionaryMutationResponse, error) {
	var response *SqliteDictionaryMutationResponse
	err := c.withSession(ctx, func(rpc pb.ControllerServiceClient, sessionID string) error {
		var err error
		raw, err := rpc.UpsertSqliteCustomWord(ctx, request.toUpsertProto(sessionID))
		response = mapSqliteDictionaryMutationResponse(raw)
		return err
	})
	return response, err
}

// RemoveSqliteCustomWord removes one custom tokenizer word.
// RemoveSqliteCustomWord 删除一个自定义分词词条。
func (c *Client) RemoveSqliteCustomWord(ctx context.Context, request *SqliteCustomWordRequest) (*SqliteDictionaryMutationResponse, error) {
	var response *SqliteDictionaryMutationResponse
	err := c.withSession(ctx, func(rpc pb.ControllerServiceClient, sessionID string) error {
		var err error
		raw, err := rpc.RemoveSqliteCustomWord(ctx, request.toRemoveProto(sessionID))
		response = mapSqliteDictionaryMutationResponse(raw)
		return err
	})
	return response, err
}

// EnsureSqliteFtsIndex ensures one SQLite FTS index exists.
// EnsureSqliteFtsIndex 确保一个 SQLite FTS 索引存在。
func (c *Client) EnsureSqliteFtsIndex(ctx context.Context, request *SqliteFTSIndexRequest) (*SqliteEnsureFTSIndexResponse, error) {
	var response *SqliteEnsureFTSIndexResponse
	err := c.withSession(ctx, func(rpc pb.ControllerServiceClient, sessionID string) error {
		var err error
		raw, err := rpc.EnsureSqliteFtsIndex(ctx, request.toEnsureProto(sessionID))
		response = mapSqliteEnsureFTSIndexResponse(raw)
		return err
	})
	return response, err
}

// RebuildSqliteFtsIndex rebuilds one SQLite FTS index.
// RebuildSqliteFtsIndex 重建一个 SQLite FTS 索引。
func (c *Client) RebuildSqliteFtsIndex(ctx context.Context, request *SqliteFTSIndexRequest) (*SqliteRebuildFTSIndexResponse, error) {
	var response *SqliteRebuildFTSIndexResponse
	err := c.withSession(ctx, func(rpc pb.ControllerServiceClient, sessionID string) error {
		var err error
		raw, err := rpc.RebuildSqliteFtsIndex(ctx, request.toRebuildProto(sessionID))
		response = mapSqliteRebuildFTSIndexResponse(raw)
		return err
	})
	return response, err
}

// UpsertSqliteFtsDocument writes one SQLite FTS document.
// UpsertSqliteFtsDocument 写入一个 SQLite FTS 文档。
func (c *Client) UpsertSqliteFtsDocument(ctx context.Context, request *SqliteFTSDocumentRequest) (*SqliteFTSMutationResponse, error) {
	var response *SqliteFTSMutationResponse
	err := c.withSession(ctx, func(rpc pb.ControllerServiceClient, sessionID string) error {
		var err error
		raw, err := rpc.UpsertSqliteFtsDocument(ctx, request.toProto(sessionID))
		response = mapSqliteFTSMutationResponse(raw)
		return err
	})
	return response, err
}

// DeleteSqliteFtsDocument deletes one SQLite FTS document.
// DeleteSqliteFtsDocument 删除一个 SQLite FTS 文档。
func (c *Client) DeleteSqliteFtsDocument(ctx context.Context, request *SqliteFTSDeleteDocumentRequest) (*SqliteFTSMutationResponse, error) {
	var response *SqliteFTSMutationResponse
	err := c.withSession(ctx, func(rpc pb.ControllerServiceClient, sessionID string) error {
		var err error
		raw, err := rpc.DeleteSqliteFtsDocument(ctx, request.toProto(sessionID))
		response = mapSqliteFTSMutationResponse(raw)
		return err
	})
	return response, err
}

// SearchSqliteFts searches one SQLite FTS index.
// SearchSqliteFts 检索一个 SQLite FTS 索引。
func (c *Client) SearchSqliteFts(ctx context.Context, request *SqliteFTSSearchRequest) (*SqliteFTSSearchResponse, error) {
	var response *SqliteFTSSearchResponse
	err := c.withSession(ctx, func(rpc pb.ControllerServiceClient, sessionID string) error {
		var err error
		raw, err := rpc.SearchSqliteFts(ctx, request.toProto(sessionID))
		response = mapSqliteFTSSearchResponse(raw)
		return err
	})
	return response, err
}

// EnableLanceDB enables one LanceDB backend and remembers it for recovery.
// EnableLanceDB 启用一个 LanceDB 后端并记录用于恢复。
func (c *Client) EnableLanceDB(ctx context.Context, request *LanceDBEnableRequest) error {
	return c.withSession(ctx, func(rpc pb.ControllerServiceClient, sessionID string) error {
		req := request.clone()
		if _, err := rpc.EnableLanceDb(ctx, req.toProto(sessionID)); err != nil {
			return err
		}
		c.mu.Lock()
		c.lancedbBindings[lancedbBindingKey{SpaceID: req.SpaceID, BindingID: req.BindingID}] = req.clone()
		c.mu.Unlock()
		return nil
	})
}

// DisableLanceDB disables one LanceDB backend and forgets it from recovery state.
// DisableLanceDB 关闭一个 LanceDB 后端并从恢复状态中遗忘。
func (c *Client) DisableLanceDB(ctx context.Context, spaceID string, bindingID string) (bool, error) {
	var disabled bool
	err := c.withSession(ctx, func(rpc pb.ControllerServiceClient, sessionID string) error {
		response, err := rpc.DisableLanceDb(ctx, &pb.DisableBackendRequest{ClientSessionId: sessionID, SpaceId: spaceID, BindingId: bindingID})
		if err != nil {
			return err
		}
		disabled = response.Disabled
		c.mu.Lock()
		delete(c.lancedbBindings, lancedbBindingKey{SpaceID: spaceID, BindingID: bindingID})
		c.mu.Unlock()
		return nil
	})
	return disabled, err
}

// CreateLanceDBTable creates one LanceDB table.
// CreateLanceDBTable 创建一张 LanceDB 表。
func (c *Client) CreateLanceDBTable(ctx context.Context, request *LanceDBCreateTableRequest) (*LanceDBCreateTableResponse, error) {
	var response *LanceDBCreateTableResponse
	err := c.withSession(ctx, func(rpc pb.ControllerServiceClient, sessionID string) error {
		var err error
		raw, err := rpc.CreateLanceDbTable(ctx, request.toProto(sessionID))
		response = mapLanceDBCreateTableResponse(raw)
		return err
	})
	return response, err
}

// UpsertLanceDB writes rows into one LanceDB table.
// UpsertLanceDB 向一张 LanceDB 表写入行。
func (c *Client) UpsertLanceDB(ctx context.Context, request *LanceDBUpsertRequest) (*LanceDBUpsertResponse, error) {
	var response *LanceDBUpsertResponse
	err := c.withSession(ctx, func(rpc pb.ControllerServiceClient, sessionID string) error {
		var err error
		raw, err := rpc.UpsertLanceDb(ctx, request.toProto(sessionID))
		response = mapLanceDBUpsertResponse(raw)
		return err
	})
	return response, err
}

// SearchLanceDB searches rows in one LanceDB table.
// SearchLanceDB 检索一张 LanceDB 表中的行。
func (c *Client) SearchLanceDB(ctx context.Context, request *LanceDBSearchRequest) (*LanceDBSearchResponse, error) {
	var response *LanceDBSearchResponse
	err := c.withSession(ctx, func(rpc pb.ControllerServiceClient, sessionID string) error {
		var err error
		raw, err := rpc.SearchLanceDb(ctx, request.toProto(sessionID))
		response = mapLanceDBSearchResponse(raw)
		return err
	})
	return response, err
}

// DeleteLanceDB deletes rows in one LanceDB table.
// DeleteLanceDB 删除一张 LanceDB 表中的行。
func (c *Client) DeleteLanceDB(ctx context.Context, request *LanceDBDeleteRequest) (*LanceDBDeleteResponse, error) {
	var response *LanceDBDeleteResponse
	err := c.withSession(ctx, func(rpc pb.ControllerServiceClient, sessionID string) error {
		var err error
		raw, err := rpc.DeleteLanceDb(ctx, request.toProto(sessionID))
		response = mapLanceDBDeleteResponse(raw)
		return err
	})
	return response, err
}

// DropLanceDBTable drops one LanceDB table.
// DropLanceDBTable 删除一张 LanceDB 表。
func (c *Client) DropLanceDBTable(ctx context.Context, request *LanceDBDropTableRequest) (*LanceDBDropTableResponse, error) {
	var response *LanceDBDropTableResponse
	err := c.withSession(ctx, func(rpc pb.ControllerServiceClient, sessionID string) error {
		var err error
		raw, err := rpc.DropLanceDbTable(ctx, request.toProto(sessionID))
		response = mapLanceDBDropTableResponse(raw)
		return err
	})
	return response, err
}

// ensureReady connects to an existing endpoint or starts the configured target.
// ensureReady 连接已有端点或启动配置目标。
func (c *Client) ensureReady(ctx context.Context) error {
	if _, err := c.probeStatus(ctx); err == nil {
		return nil
	}
	if !c.config.AutoSpawn {
		return fmt.Errorf("controller endpoint %q is unavailable and automatic startup is disabled", c.config.Endpoint)
	}

	deadline := time.Now().Add(c.config.StartupTimeout)
	started := false
	for {
		if _, err := c.probeStatus(ctx); err == nil {
			return nil
		}
		if !started {
			if err := c.spawnController(); err != nil {
				return err
			}
			started = true
		}
		if time.Now().After(deadline) {
			return fmt.Errorf("controller endpoint %q did not become ready within %s", c.config.Endpoint, c.config.StartupTimeout)
		}
		select {
		case <-ctx.Done():
			return ctx.Err()
		case <-time.After(c.config.StartupRetryInterval):
		}
	}
}

// spawnController starts one foreground controller process.
// spawnController 启动一个前台 controller 进程。
func (c *Client) spawnController() error {
	bindAddr, err := bindAddress(c.config.Endpoint)
	if err != nil {
		return err
	}
	cmd := exec.Command(
		c.config.SpawnExecutable,
		"--bind", bindAddr,
		"--mode", string(c.config.SpawnProcessMode),
		"--minimum-uptime-secs", fmt.Sprintf("%.0f", c.config.MinimumUptime.Seconds()),
		"--idle-timeout-secs", fmt.Sprintf("%.0f", c.config.IdleTimeout.Seconds()),
		"--default-lease-ttl-secs", fmt.Sprintf("%.0f", c.config.DefaultLeaseTTL.Seconds()),
	)
	return cmd.Start()
}

// probe checks whether the endpoint accepts one gRPC connection.
// probe 检查端点是否接受一个 gRPC 连接。
func (c *Client) probe(ctx context.Context) error {
	target, err := dialTarget(c.config.Endpoint)
	if err != nil {
		return err
	}
	probeCtx, cancel := context.WithTimeout(ctx, c.config.ConnectTimeout)
	defer cancel()
	conn, err := grpc.DialContext(probeCtx, target, grpc.WithTransportCredentials(insecure.NewCredentials()), grpc.WithBlock())
	if err != nil {
		return err
	}
	return conn.Close()
}

// probeStatus checks whether the endpoint accepts gRPC and exposes controller status.
// probeStatus 检查端点是否接受 gRPC 并暴露 controller 状态。
func (c *Client) probeStatus(ctx context.Context) (*StatusSnapshot, error) {
	target, err := dialTarget(c.config.Endpoint)
	if err != nil {
		return nil, err
	}
	probeCtx, cancel := context.WithTimeout(ctx, c.config.ConnectTimeout)
	defer cancel()
	conn, err := grpc.DialContext(probeCtx, target, grpc.WithTransportCredentials(insecure.NewCredentials()), grpc.WithBlock())
	if err != nil {
		return nil, err
	}
	defer conn.Close()
	response, err := pb.NewControllerServiceClient(conn).GetStatus(probeCtx, &pb.GetStatusRequest{})
	if err != nil {
		return nil, err
	}
	if response.Status == nil {
		return nil, errors.New("controller status payload is missing")
	}
	return mapStatusSnapshot(response.Status)
}

// ensureConnected creates the gRPC connection when needed.
// ensureConnected 在需要时创建 gRPC 连接。
func (c *Client) ensureConnected(ctx context.Context) error {
	c.mu.Lock()
	if c.rpc != nil {
		c.mu.Unlock()
		return nil
	}
	c.mu.Unlock()

	target, err := dialTarget(c.config.Endpoint)
	if err != nil {
		return err
	}
	dialCtx, cancel := context.WithTimeout(ctx, c.config.ConnectTimeout)
	defer cancel()
	conn, err := grpc.DialContext(dialCtx, target, grpc.WithTransportCredentials(insecure.NewCredentials()), grpc.WithBlock())
	if err != nil {
		return err
	}
	rpc := pb.NewControllerServiceClient(conn)
	c.mu.Lock()
	c.conn = conn
	c.rpc = rpc
	c.mu.Unlock()
	return nil
}

// ensureRegistered registers a controller session when missing.
// ensureRegistered 在会话缺失时注册 controller 会话。
func (c *Client) ensureRegistered(ctx context.Context) error {
	c.mu.Lock()
	if c.clientSessionID != "" {
		c.mu.Unlock()
		return nil
	}
	rpc := c.rpc
	c.mu.Unlock()
	response, err := rpc.RegisterClient(ctx, c.registration.toProto())
	if err != nil {
		return err
	}
	c.mu.Lock()
	c.clientSessionID = response.Client.ClientSessionId
	c.mu.Unlock()
	return c.replayDesiredState(ctx)
}

// withSession executes one operation with a valid session and retries once after recoverable loss.
// withSession 使用有效会话执行一次操作，并在可恢复丢失后重试一次。
func (c *Client) withSession(ctx context.Context, op func(pb.ControllerServiceClient, string) error) error {
	if err := c.Connect(ctx); err != nil {
		return err
	}
	c.mu.Lock()
	rpc := c.rpc
	sessionID := c.clientSessionID
	c.mu.Unlock()
	err := op(rpc, sessionID)
	if !isRecoverable(err) {
		return err
	}
	c.resetConnection()
	if err := c.Connect(ctx); err != nil {
		return err
	}
	c.mu.Lock()
	rpc = c.rpc
	sessionID = c.clientSessionID
	c.mu.Unlock()
	return op(rpc, sessionID)
}

// replayDesiredState replays spaces and backend bindings after session recovery.
// replayDesiredState 在会话恢复后重放空间与后端绑定。
func (c *Client) replayDesiredState(ctx context.Context) error {
	c.mu.Lock()
	rpc := c.rpc
	sessionID := c.clientSessionID
	spaces := make([]SpaceRegistration, 0, len(c.attachedSpaces))
	for _, registration := range c.attachedSpaces {
		spaces = append(spaces, registration)
	}
	sqliteBindings := make([]*SqliteEnableRequest, 0, len(c.sqliteBindings))
	for _, request := range c.sqliteBindings {
		sqliteBindings = append(sqliteBindings, request.clone())
	}
	lancedbBindings := make([]*LanceDBEnableRequest, 0, len(c.lancedbBindings))
	for _, request := range c.lancedbBindings {
		lancedbBindings = append(lancedbBindings, request.clone())
	}
	c.mu.Unlock()

	for _, registration := range spaces {
		if _, err := rpc.AttachSpace(ctx, registration.toProto(sessionID)); err != nil {
			return err
		}
	}
	for _, request := range sqliteBindings {
		if _, err := rpc.EnableSqlite(ctx, request.toProto(sessionID)); err != nil {
			return err
		}
	}
	for _, request := range lancedbBindings {
		if _, err := rpc.EnableLanceDb(ctx, request.toProto(sessionID)); err != nil {
			return err
		}
	}
	return nil
}

// ensureRenewer starts the background lease renewer exactly once.
// ensureRenewer 确保后台租约续期任务只启动一次。
func (c *Client) ensureRenewer() {
	c.mu.Lock()
	if c.renewCancel != nil {
		c.mu.Unlock()
		return
	}
	ctx, cancel := context.WithCancel(context.Background())
	c.renewCancel = cancel
	interval := c.config.LeaseRenewInterval
	c.mu.Unlock()

	go func() {
		ticker := time.NewTicker(interval)
		defer ticker.Stop()
		for {
			select {
			case <-ctx.Done():
				return
			case <-ticker.C:
				_ = c.renewOnce(ctx)
			}
		}
	}()
}

// renewOnce performs one lease renewal.
// renewOnce 执行一次租约续期。
func (c *Client) renewOnce(ctx context.Context) error {
	c.mu.Lock()
	rpc := c.rpc
	sessionID := c.clientSessionID
	ttl := c.registration.LeaseTTL
	c.mu.Unlock()
	if rpc == nil || sessionID == "" {
		return nil
	}
	_, err := rpc.RenewClientLease(ctx, &pb.RenewClientLeaseRequest{ClientSessionId: sessionID, LeaseTtlSecs: uint64(ttl.Seconds())})
	return err
}

// resetConnection clears the current connection and session state.
// resetConnection 清理当前连接与会话状态。
func (c *Client) resetConnection() {
	c.mu.Lock()
	conn := c.conn
	c.conn = nil
	c.rpc = nil
	c.clientSessionID = ""
	c.mu.Unlock()
	if conn != nil {
		_ = conn.Close()
	}
}

// isRecoverable reports whether one RPC error should trigger reconnect and replay.
// isRecoverable 判断一个 RPC 错误是否应触发重连和重放。
func isRecoverable(err error) bool {
	if err == nil {
		return false
	}
	code := status.Code(err)
	return code == codes.Unavailable || code == codes.NotFound || code == codes.Unauthenticated || strings.Contains(err.Error(), "client session")
}

// dialTarget converts endpoint text into a gRPC dial target.
// dialTarget 将端点文本转换为 gRPC 拨号目标。
func dialTarget(endpoint string) (string, error) {
	trimmed := strings.TrimSpace(endpoint)
	if strings.HasPrefix(trimmed, "http://") || strings.HasPrefix(trimmed, "https://") {
		parsed, err := url.Parse(trimmed)
		if err != nil {
			return "", err
		}
		return parsed.Host, nil
	}
	return trimmed, nil
}

// bindAddress converts a local endpoint into a controller bind address.
// bindAddress 将本地端点转换为 controller 绑定地址。
func bindAddress(endpoint string) (string, error) {
	target, err := dialTarget(endpoint)
	if err != nil {
		return "", err
	}
	if strings.HasPrefix(target, ":") {
		return "0.0.0.0" + target, nil
	}
	host, port, err := net.SplitHostPort(target)
	if err != nil {
		if _, portErr := net.LookupPort("tcp", target); portErr == nil {
			return "127.0.0.1:" + target, nil
		}
		return "", err
	}
	if host == "localhost" {
		host = "127.0.0.1"
	}
	if host != "127.0.0.1" && host != "0.0.0.0" && host != "::1" && host != "::" {
		return "", fmt.Errorf("endpoint %q cannot be converted into a local bind address", endpoint)
	}
	return net.JoinHostPort(host, port), nil
}

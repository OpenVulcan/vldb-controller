package controller

import (
	"fmt"

	pb "github.com/OpenVulcan/vldb-controller/client-go/v1"
)

// toProto converts a client registration into the protobuf request shape.
// toProto 将客户端注册转换为 protobuf 请求形态。
func (r ClientRegistration) toProto() *pb.RegisterClientRequest {
	return &pb.RegisterClientRequest{
		ClientName:   r.ClientName,
		HostKind:     r.HostKind,
		ProcessId:    r.ProcessID,
		ProcessName:  r.ProcessName,
		LeaseTtlSecs: uint64(r.LeaseTTL.Seconds()),
	}
}

// toProto converts a space registration into the protobuf request shape.
// toProto 将空间注册转换为 protobuf 请求形态。
func (r SpaceRegistration) toProto(clientSessionID string) *pb.AttachSpaceRequest {
	return &pb.AttachSpaceRequest{
		ClientSessionId: clientSessionID,
		SpaceId:         r.SpaceID,
		SpaceLabel:      r.SpaceLabel,
		SpaceKind:       mapSpaceKindToProto(r.SpaceKind),
		SpaceRoot:       r.SpaceRoot,
	}
}

// mapStatusSnapshot converts protobuf status into the Go-native shape.
// mapStatusSnapshot 将 protobuf 状态转换为 Go 原生形态。
func mapStatusSnapshot(snapshot *pb.ControllerStatusSnapshot) (*StatusSnapshot, error) {
	if snapshot == nil {
		return nil, fmt.Errorf("controller status payload is missing")
	}
	return &StatusSnapshot{
		ProcessMode:         mapProcessModeFromProto(snapshot.ProcessMode),
		BindAddr:            snapshot.BindAddr,
		StartedAtUnixMs:     snapshot.StartedAtUnixMs,
		LastRequestAtUnixMs: snapshot.LastRequestAtUnixMs,
		MinimumUptimeSecs:   snapshot.MinimumUptimeSecs,
		IdleTimeoutSecs:     snapshot.IdleTimeoutSecs,
		DefaultLeaseTTLSecs: snapshot.DefaultLeaseTtlSecs,
		ActiveClients:       snapshot.ActiveClients,
		AttachedSpaces:      snapshot.AttachedSpaces,
		InflightRequests:    snapshot.InflightRequests,
		ShutdownCandidate:   snapshot.ShutdownCandidate,
	}, nil
}

// mapSpaceSnapshot converts protobuf space state into the Go-native shape.
// mapSpaceSnapshot 将 protobuf 空间状态转换为 Go 原生形态。
func mapSpaceSnapshot(snapshot *pb.SpaceSnapshot) *SpaceSnapshot {
	if snapshot == nil {
		return nil
	}
	return &SpaceSnapshot{
		SpaceID:         snapshot.SpaceId,
		SpaceLabel:      snapshot.SpaceLabel,
		SpaceKind:       mapSpaceKindFromProto(snapshot.SpaceKind),
		SpaceRoot:       snapshot.SpaceRoot,
		AttachedClients: snapshot.AttachedClients,
		SQLite:          mapBackendStatus(snapshot.Sqlite),
		LanceDB:         mapBackendStatus(snapshot.Lancedb),
	}
}

// mapClientLeaseSnapshot converts protobuf client lease state into the Go-native shape.
// mapClientLeaseSnapshot 将 protobuf 客户端租约状态转换为 Go 原生形态。
func mapClientLeaseSnapshot(snapshot *pb.ClientLeaseSnapshot) *ClientLeaseSnapshot {
	if snapshot == nil {
		return nil
	}
	return &ClientLeaseSnapshot{
		ClientSessionID:  snapshot.ClientSessionId,
		ClientName:       snapshot.ClientName,
		HostKind:         snapshot.HostKind,
		ProcessID:        snapshot.ProcessId,
		ProcessName:      snapshot.ProcessName,
		LastSeenUnixMs:   snapshot.LastSeenUnixMs,
		ExpiresAtUnixMs:  snapshot.ExpiresAtUnixMs,
		AttachedSpaceIDs: append([]string(nil), snapshot.AttachedSpaceIds...),
	}
}

// mapBackendStatus converts protobuf backend state into the Go-native shape.
// mapBackendStatus 将 protobuf 后端状态转换为 Go 原生形态。
func mapBackendStatus(status *pb.BackendStatus) *BackendStatus {
	if status == nil {
		return nil
	}
	return &BackendStatus{Enabled: status.Enabled, Mode: status.Mode, Target: status.Target}
}

// toProto converts a SQLite enable request into protobuf.
// toProto 将 SQLite 启用请求转换为 protobuf。
func (r *SqliteEnableRequest) toProto(clientSessionID string) *pb.EnableSqliteRequest {
	if r == nil {
		r = &SqliteEnableRequest{}
	}
	return &pb.EnableSqliteRequest{
		ClientSessionId:        clientSessionID,
		SpaceId:                r.SpaceID,
		BindingId:              r.BindingID,
		DbPath:                 r.DBPath,
		ConnectionPoolSize:     r.ConnectionPoolSize,
		BusyTimeoutMs:          r.BusyTimeoutMs,
		JournalMode:            r.JournalMode,
		Synchronous:            r.Synchronous,
		ForeignKeys:            r.ForeignKeys,
		TempStore:              r.TempStore,
		WalAutocheckpointPages: r.WalAutocheckpointPages,
		CacheSizeKib:           r.CacheSizeKib,
		MmapSizeBytes:          r.MmapSizeBytes,
		EnforceDbFileLock:      r.EnforceDBFileLock,
		ReadOnly:               r.ReadOnly,
		AllowUriFilenames:      r.AllowURIFilenames,
		TrustedSchema:          r.TrustedSchema,
		Defensive:              r.Defensive,
	}
}

// clone returns a detached copy of one SQLite enable request.
// clone 返回一份独立的 SQLite 启用请求副本。
func (r *SqliteEnableRequest) clone() *SqliteEnableRequest {
	if r == nil {
		return &SqliteEnableRequest{}
	}
	clone := *r
	return &clone
}

// mapSqliteValues converts Go-native SQLite values into protobuf values.
// mapSqliteValues 将 Go 原生 SQLite 值转换为 protobuf 值。
func mapSqliteValues(values []SqliteValue) []*pb.SqliteValue {
	items := make([]*pb.SqliteValue, 0, len(values))
	for _, value := range values {
		items = append(items, mapSqliteValue(value))
	}
	return items
}

// mapSqliteValue converts one Go-native SQLite value into protobuf.
// mapSqliteValue 将一个 Go 原生 SQLite 值转换为 protobuf。
func mapSqliteValue(value SqliteValue) *pb.SqliteValue {
	switch value.Kind {
	case SqliteValueInt64:
		return &pb.SqliteValue{Kind: &pb.SqliteValue_Int64Value{Int64Value: value.Int64Value}}
	case SqliteValueFloat64:
		return &pb.SqliteValue{Kind: &pb.SqliteValue_Float64Value{Float64Value: value.Float64Value}}
	case SqliteValueString:
		return &pb.SqliteValue{Kind: &pb.SqliteValue_StringValue{StringValue: value.StringValue}}
	case SqliteValueBytes:
		return &pb.SqliteValue{Kind: &pb.SqliteValue_BytesValue{BytesValue: value.BytesValue}}
	case SqliteValueBool:
		return &pb.SqliteValue{Kind: &pb.SqliteValue_BoolValue{BoolValue: value.BoolValue}}
	default:
		return &pb.SqliteValue{Kind: &pb.SqliteValue_NullValue{NullValue: &pb.NullValue{}}}
	}
}

// toProto converts a SQLite script request into protobuf.
// toProto 将 SQLite 脚本请求转换为 protobuf。
func (r *SqliteExecuteScriptRequest) toProto(clientSessionID string) *pb.ExecuteSqliteScriptRequest {
	if r == nil {
		r = &SqliteExecuteScriptRequest{}
	}
	return &pb.ExecuteSqliteScriptRequest{
		ClientSessionId: clientSessionID,
		SpaceId:         r.SpaceID,
		BindingId:       r.BindingID,
		Sql:             r.SQL,
		Params:          mapSqliteValues(r.Params),
	}
}

// mapSqliteExecuteScriptResponse converts protobuf script result into Go-native shape.
// mapSqliteExecuteScriptResponse 将 protobuf 脚本结果转换为 Go 原生形态。
func mapSqliteExecuteScriptResponse(response *pb.ExecuteSqliteScriptResponse) *SqliteExecuteScriptResponse {
	if response == nil {
		return nil
	}
	return &SqliteExecuteScriptResponse{
		Success:         response.Success,
		Message:         response.Message,
		RowsChanged:     response.RowsChanged,
		LastInsertRowID: response.LastInsertRowid,
	}
}

// toProto converts a SQLite batch request into protobuf.
// toProto 将 SQLite 批量请求转换为 protobuf。
func (r *SqliteExecuteBatchRequest) toProto(clientSessionID string) *pb.ExecuteSqliteBatchRequest {
	if r == nil {
		r = &SqliteExecuteBatchRequest{}
	}
	items := make([]*pb.ExecuteSqliteBatchItem, 0, len(r.Items))
	for _, item := range r.Items {
		items = append(items, &pb.ExecuteSqliteBatchItem{Params: mapSqliteValues(item.Params)})
	}
	return &pb.ExecuteSqliteBatchRequest{
		ClientSessionId: clientSessionID,
		SpaceId:         r.SpaceID,
		BindingId:       r.BindingID,
		Sql:             r.SQL,
		Items:           items,
	}
}

// mapSqliteExecuteBatchResponse converts protobuf batch result into Go-native shape.
// mapSqliteExecuteBatchResponse 将 protobuf 批量结果转换为 Go 原生形态。
func mapSqliteExecuteBatchResponse(response *pb.ExecuteSqliteBatchResponse) *SqliteExecuteBatchResponse {
	if response == nil {
		return nil
	}
	return &SqliteExecuteBatchResponse{
		Success:            response.Success,
		Message:            response.Message,
		RowsChanged:        response.RowsChanged,
		LastInsertRowID:    response.LastInsertRowid,
		StatementsExecuted: response.StatementsExecuted,
	}
}

// toProto converts a SQLite JSON query request into protobuf.
// toProto 将 SQLite JSON 查询请求转换为 protobuf。
func (r *SqliteQueryJSONRequest) toProto(clientSessionID string) *pb.QuerySqliteJsonRequest {
	if r == nil {
		r = &SqliteQueryJSONRequest{}
	}
	return &pb.QuerySqliteJsonRequest{
		ClientSessionId: clientSessionID,
		SpaceId:         r.SpaceID,
		BindingId:       r.BindingID,
		Sql:             r.SQL,
		Params:          mapSqliteValues(r.Params),
	}
}

// mapSqliteQueryJSONResponse converts protobuf JSON query result into Go-native shape.
// mapSqliteQueryJSONResponse 将 protobuf JSON 查询结果转换为 Go 原生形态。
func mapSqliteQueryJSONResponse(response *pb.QuerySqliteJsonResponse) *SqliteQueryJSONResponse {
	if response == nil {
		return nil
	}
	return &SqliteQueryJSONResponse{JSONData: response.JsonData, RowCount: response.RowCount}
}

// toProto converts a SQLite stream request into protobuf.
// toProto 将 SQLite 流式查询请求转换为 protobuf。
func (r *SqliteQueryStreamRequest) toProto(clientSessionID string) *pb.QuerySqliteStreamRequest {
	if r == nil {
		r = &SqliteQueryStreamRequest{}
	}
	return &pb.QuerySqliteStreamRequest{
		ClientSessionId: clientSessionID,
		SpaceId:         r.SpaceID,
		BindingId:       r.BindingID,
		Sql:             r.SQL,
		Params:          mapSqliteValues(r.Params),
		TargetChunkSize: r.TargetChunkSize,
	}
}

// mapSqliteQueryStreamResponse converts protobuf stream open result into Go-native shape.
// mapSqliteQueryStreamResponse 将 protobuf 流打开结果转换为 Go 原生形态。
func mapSqliteQueryStreamResponse(response *pb.QuerySqliteStreamResponse) *SqliteQueryStreamResponse {
	if response == nil {
		return nil
	}
	return &SqliteQueryStreamResponse{StreamID: response.StreamId, MetricsReady: response.MetricsReady}
}

// toProto converts a wait-metrics request into protobuf.
// toProto 将等待指标请求转换为 protobuf。
func (r *SqliteQueryStreamWaitMetricsRequest) toProto(clientSessionID string) *pb.QuerySqliteStreamWaitMetricsRequest {
	if r == nil {
		r = &SqliteQueryStreamWaitMetricsRequest{}
	}
	return &pb.QuerySqliteStreamWaitMetricsRequest{ClientSessionId: clientSessionID, StreamId: r.StreamID}
}

// mapSqliteQueryStreamWaitMetricsResponse converts protobuf stream metrics into Go-native shape.
// mapSqliteQueryStreamWaitMetricsResponse 将 protobuf 流指标转换为 Go 原生形态。
func mapSqliteQueryStreamWaitMetricsResponse(response *pb.QuerySqliteStreamWaitMetricsResponse) *SqliteQueryStreamWaitMetricsResponse {
	if response == nil {
		return nil
	}
	return &SqliteQueryStreamWaitMetricsResponse{RowCount: response.RowCount, ChunkCount: response.ChunkCount, TotalBytes: response.TotalBytes}
}

// toProto converts a stream chunk request into protobuf.
// toProto 将流分块请求转换为 protobuf。
func (r *SqliteQueryStreamChunkRequest) toProto(clientSessionID string) *pb.QuerySqliteStreamChunkRequest {
	if r == nil {
		r = &SqliteQueryStreamChunkRequest{}
	}
	return &pb.QuerySqliteStreamChunkRequest{ClientSessionId: clientSessionID, StreamId: r.StreamID, Index: r.Index}
}

// mapSqliteQueryStreamChunkResponse converts protobuf stream chunk into Go-native shape.
// mapSqliteQueryStreamChunkResponse 将 protobuf 流分块转换为 Go 原生形态。
func mapSqliteQueryStreamChunkResponse(response *pb.QuerySqliteStreamChunkResponse) *SqliteQueryStreamChunkResponse {
	if response == nil {
		return nil
	}
	return &SqliteQueryStreamChunkResponse{Chunk: response.Chunk}
}

// toProto converts a stream close request into protobuf.
// toProto 将流关闭请求转换为 protobuf。
func (r *SqliteQueryStreamCloseRequest) toProto(clientSessionID string) *pb.QuerySqliteStreamCloseRequest {
	if r == nil {
		r = &SqliteQueryStreamCloseRequest{}
	}
	return &pb.QuerySqliteStreamCloseRequest{ClientSessionId: clientSessionID, StreamId: r.StreamID}
}

// mapSqliteQueryStreamCloseResponse converts protobuf stream close status into Go-native shape.
// mapSqliteQueryStreamCloseResponse 将 protobuf 流关闭状态转换为 Go 原生形态。
func mapSqliteQueryStreamCloseResponse(response *pb.QuerySqliteStreamCloseResponse) *SqliteQueryStreamCloseResponse {
	if response == nil {
		return nil
	}
	return &SqliteQueryStreamCloseResponse{Closed: response.Closed}
}

// toProto converts a tokenization request into protobuf.
// toProto 将分词请求转换为 protobuf。
func (r *SqliteTokenizeTextRequest) toProto(clientSessionID string) *pb.TokenizeSqliteTextRequest {
	if r == nil {
		r = &SqliteTokenizeTextRequest{}
	}
	return &pb.TokenizeSqliteTextRequest{
		ClientSessionId: clientSessionID,
		SpaceId:         r.SpaceID,
		BindingId:       r.BindingID,
		TokenizerMode:   mapTokenizerModeToProto(r.TokenizerMode),
		Text:            r.Text,
		SearchMode:      r.SearchMode,
	}
}

// mapSqliteTokenizeTextResponse converts protobuf tokenization output into Go-native shape.
// mapSqliteTokenizeTextResponse 将 protobuf 分词输出转换为 Go 原生形态。
func mapSqliteTokenizeTextResponse(response *pb.TokenizeSqliteTextResponse) *SqliteTokenizeTextResponse {
	if response == nil {
		return nil
	}
	return &SqliteTokenizeTextResponse{TokenizerMode: response.TokenizerMode, NormalizedText: response.NormalizedText, Tokens: response.Tokens, FTSQuery: response.FtsQuery}
}

// toProto converts a backend request into protobuf.
// toProto 将后端请求转换为 protobuf。
func (r *SqliteBackendRequest) toProto(clientSessionID string) *pb.ListSqliteCustomWordsRequest {
	if r == nil {
		r = &SqliteBackendRequest{}
	}
	return &pb.ListSqliteCustomWordsRequest{ClientSessionId: clientSessionID, SpaceId: r.SpaceID, BindingId: r.BindingID}
}

// mapSqliteListCustomWordsResponse converts protobuf custom words into Go-native shape.
// mapSqliteListCustomWordsResponse 将 protobuf 自定义词转换为 Go 原生形态。
func mapSqliteListCustomWordsResponse(response *pb.ListSqliteCustomWordsResponse) *SqliteListCustomWordsResponse {
	if response == nil {
		return nil
	}
	words := make([]SqliteCustomWordEntry, 0, len(response.Words))
	for _, word := range response.Words {
		words = append(words, SqliteCustomWordEntry{Word: word.Word, Weight: word.Weight})
	}
	return &SqliteListCustomWordsResponse{Success: response.Success, Message: response.Message, Words: words}
}

// toUpsertProto converts a custom word request into protobuf upsert shape.
// toUpsertProto 将自定义词请求转换为 protobuf upsert 形态。
func (r *SqliteCustomWordRequest) toUpsertProto(clientSessionID string) *pb.UpsertSqliteCustomWordRequest {
	if r == nil {
		r = &SqliteCustomWordRequest{}
	}
	return &pb.UpsertSqliteCustomWordRequest{ClientSessionId: clientSessionID, SpaceId: r.SpaceID, BindingId: r.BindingID, Word: r.Word, Weight: r.Weight}
}

// toRemoveProto converts a custom word request into protobuf remove shape.
// toRemoveProto 将自定义词请求转换为 protobuf remove 形态。
func (r *SqliteCustomWordRequest) toRemoveProto(clientSessionID string) *pb.RemoveSqliteCustomWordRequest {
	if r == nil {
		r = &SqliteCustomWordRequest{}
	}
	return &pb.RemoveSqliteCustomWordRequest{ClientSessionId: clientSessionID, SpaceId: r.SpaceID, BindingId: r.BindingID, Word: r.Word}
}

// mapSqliteDictionaryMutationResponse converts protobuf dictionary mutation status into Go-native shape.
// mapSqliteDictionaryMutationResponse 将 protobuf 词典变更状态转换为 Go 原生形态。
func mapSqliteDictionaryMutationResponse(response *pb.SqliteDictionaryMutationResponse) *SqliteDictionaryMutationResponse {
	if response == nil {
		return nil
	}
	return &SqliteDictionaryMutationResponse{Success: response.Success, Message: response.Message, AffectedRows: response.AffectedRows}
}

// toEnsureProto converts an FTS index request into protobuf ensure shape.
// toEnsureProto 将 FTS 索引请求转换为 protobuf ensure 形态。
func (r *SqliteFTSIndexRequest) toEnsureProto(clientSessionID string) *pb.EnsureSqliteFtsIndexRequest {
	if r == nil {
		r = &SqliteFTSIndexRequest{}
	}
	return &pb.EnsureSqliteFtsIndexRequest{ClientSessionId: clientSessionID, SpaceId: r.SpaceID, BindingId: r.BindingID, IndexName: r.IndexName, TokenizerMode: mapTokenizerModeToProto(r.TokenizerMode)}
}

// toRebuildProto converts an FTS index request into protobuf rebuild shape.
// toRebuildProto 将 FTS 索引请求转换为 protobuf rebuild 形态。
func (r *SqliteFTSIndexRequest) toRebuildProto(clientSessionID string) *pb.RebuildSqliteFtsIndexRequest {
	if r == nil {
		r = &SqliteFTSIndexRequest{}
	}
	return &pb.RebuildSqliteFtsIndexRequest{ClientSessionId: clientSessionID, SpaceId: r.SpaceID, BindingId: r.BindingID, IndexName: r.IndexName, TokenizerMode: mapTokenizerModeToProto(r.TokenizerMode)}
}

// mapSqliteEnsureFTSIndexResponse converts protobuf ensure-index status into Go-native shape.
// mapSqliteEnsureFTSIndexResponse 将 protobuf 确认索引状态转换为 Go 原生形态。
func mapSqliteEnsureFTSIndexResponse(response *pb.EnsureSqliteFtsIndexResponse) *SqliteEnsureFTSIndexResponse {
	if response == nil {
		return nil
	}
	return &SqliteEnsureFTSIndexResponse{Success: response.Success, Message: response.Message, IndexName: response.IndexName, TokenizerMode: response.TokenizerMode}
}

// mapSqliteRebuildFTSIndexResponse converts protobuf rebuild-index status into Go-native shape.
// mapSqliteRebuildFTSIndexResponse 将 protobuf 重建索引状态转换为 Go 原生形态。
func mapSqliteRebuildFTSIndexResponse(response *pb.RebuildSqliteFtsIndexResponse) *SqliteRebuildFTSIndexResponse {
	if response == nil {
		return nil
	}
	return &SqliteRebuildFTSIndexResponse{Success: response.Success, Message: response.Message, IndexName: response.IndexName, TokenizerMode: response.TokenizerMode, ReindexedRows: response.ReindexedRows}
}

// toProto converts an FTS document request into protobuf.
// toProto 将 FTS 文档请求转换为 protobuf。
func (r *SqliteFTSDocumentRequest) toProto(clientSessionID string) *pb.UpsertSqliteFtsDocumentRequest {
	if r == nil {
		r = &SqliteFTSDocumentRequest{}
	}
	return &pb.UpsertSqliteFtsDocumentRequest{ClientSessionId: clientSessionID, SpaceId: r.SpaceID, BindingId: r.BindingID, IndexName: r.IndexName, TokenizerMode: mapTokenizerModeToProto(r.TokenizerMode), Id: r.ID, FilePath: r.FilePath, Title: r.Title, Content: r.Content}
}

// toProto converts an FTS document deletion request into protobuf.
// toProto 将 FTS 文档删除请求转换为 protobuf。
func (r *SqliteFTSDeleteDocumentRequest) toProto(clientSessionID string) *pb.DeleteSqliteFtsDocumentRequest {
	if r == nil {
		r = &SqliteFTSDeleteDocumentRequest{}
	}
	return &pb.DeleteSqliteFtsDocumentRequest{ClientSessionId: clientSessionID, SpaceId: r.SpaceID, BindingId: r.BindingID, IndexName: r.IndexName, Id: r.ID}
}

// mapSqliteFTSMutationResponse converts protobuf FTS mutation status into Go-native shape.
// mapSqliteFTSMutationResponse 将 protobuf FTS 变更状态转换为 Go 原生形态。
func mapSqliteFTSMutationResponse(response *pb.SqliteFtsMutationResponse) *SqliteFTSMutationResponse {
	if response == nil {
		return nil
	}
	return &SqliteFTSMutationResponse{Success: response.Success, Message: response.Message, AffectedRows: response.AffectedRows, IndexName: response.IndexName}
}

// toProto converts an FTS search request into protobuf.
// toProto 将 FTS 检索请求转换为 protobuf。
func (r *SqliteFTSSearchRequest) toProto(clientSessionID string) *pb.SearchSqliteFtsRequest {
	if r == nil {
		r = &SqliteFTSSearchRequest{}
	}
	return &pb.SearchSqliteFtsRequest{ClientSessionId: clientSessionID, SpaceId: r.SpaceID, BindingId: r.BindingID, IndexName: r.IndexName, TokenizerMode: mapTokenizerModeToProto(r.TokenizerMode), Query: r.Query, Limit: r.Limit, Offset: r.Offset}
}

// mapSqliteFTSSearchResponse converts protobuf FTS search output into Go-native shape.
// mapSqliteFTSSearchResponse 将 protobuf FTS 检索输出转换为 Go 原生形态。
func mapSqliteFTSSearchResponse(response *pb.SearchSqliteFtsResponse) *SqliteFTSSearchResponse {
	if response == nil {
		return nil
	}
	hits := make([]SqliteFTSSearchHit, 0, len(response.Hits))
	for _, hit := range response.Hits {
		hits = append(hits, SqliteFTSSearchHit{
			ID:             hit.Id,
			FilePath:       hit.FilePath,
			Title:          hit.Title,
			TitleHighlight: hit.TitleHighlight,
			ContentSnippet: hit.ContentSnippet,
			Score:          hit.Score,
			Rank:           hit.Rank,
			RawScore:       hit.RawScore,
		})
	}
	return &SqliteFTSSearchResponse{Success: response.Success, Message: response.Message, IndexName: response.IndexName, TokenizerMode: response.TokenizerMode, NormalizedQuery: response.NormalizedQuery, FTSQuery: response.FtsQuery, Source: response.Source, QueryMode: response.QueryMode, Total: response.Total, Hits: hits}
}

// toProto converts a LanceDB enable request into protobuf.
// toProto 将 LanceDB 启用请求转换为 protobuf。
func (r *LanceDBEnableRequest) toProto(clientSessionID string) *pb.EnableLanceDbRequest {
	if r == nil {
		r = &LanceDBEnableRequest{}
	}
	return &pb.EnableLanceDbRequest{ClientSessionId: clientSessionID, SpaceId: r.SpaceID, BindingId: r.BindingID, DefaultDbPath: r.DefaultDBPath, DbRoot: r.DBRoot, ReadConsistencyIntervalMs: r.ReadConsistencyIntervalMs, MaxUpsertPayload: r.MaxUpsertPayload, MaxSearchLimit: r.MaxSearchLimit, MaxConcurrentRequests: r.MaxConcurrentRequests}
}

// clone returns a detached copy of one LanceDB enable request.
// clone 返回一份独立的 LanceDB 启用请求副本。
func (r *LanceDBEnableRequest) clone() *LanceDBEnableRequest {
	if r == nil {
		return &LanceDBEnableRequest{}
	}
	clone := *r
	return &clone
}

// toProto converts a LanceDB create-table request into protobuf.
// toProto 将 LanceDB 建表请求转换为 protobuf。
func (r *LanceDBCreateTableRequest) toProto(clientSessionID string) *pb.CreateLanceDbTableRequest {
	if r == nil {
		r = &LanceDBCreateTableRequest{}
	}
	columns := make([]*pb.LanceDbColumnDef, 0, len(r.Columns))
	for _, column := range r.Columns {
		columns = append(columns, &pb.LanceDbColumnDef{Name: column.Name, ColumnType: mapLanceDBColumnTypeToProto(column.ColumnType), VectorDim: column.VectorDim, Nullable: column.Nullable})
	}
	return &pb.CreateLanceDbTableRequest{ClientSessionId: clientSessionID, SpaceId: r.SpaceID, BindingId: r.BindingID, TableName: r.TableName, Columns: columns, OverwriteIfExists: r.OverwriteIfExists}
}

// mapLanceDBCreateTableResponse converts protobuf create-table output into Go-native shape.
// mapLanceDBCreateTableResponse 将 protobuf 建表输出转换为 Go 原生形态。
func mapLanceDBCreateTableResponse(response *pb.CreateLanceDbTableResponse) *LanceDBCreateTableResponse {
	if response == nil {
		return nil
	}
	return &LanceDBCreateTableResponse{Message: response.Message}
}

// toProto converts a LanceDB upsert request into protobuf.
// toProto 将 LanceDB 写入请求转换为 protobuf。
func (r *LanceDBUpsertRequest) toProto(clientSessionID string) *pb.UpsertLanceDbRequest {
	if r == nil {
		r = &LanceDBUpsertRequest{}
	}
	return &pb.UpsertLanceDbRequest{ClientSessionId: clientSessionID, SpaceId: r.SpaceID, BindingId: r.BindingID, TableName: r.TableName, InputFormat: mapLanceDBInputFormatToProto(r.InputFormat), Data: r.Data, KeyColumns: r.KeyColumns}
}

// mapLanceDBUpsertResponse converts protobuf upsert output into Go-native shape.
// mapLanceDBUpsertResponse 将 protobuf 写入输出转换为 Go 原生形态。
func mapLanceDBUpsertResponse(response *pb.UpsertLanceDbResponse) *LanceDBUpsertResponse {
	if response == nil {
		return nil
	}
	return &LanceDBUpsertResponse{Message: response.Message, Version: response.Version, InputRows: response.InputRows, InsertedRows: response.InsertedRows, UpdatedRows: response.UpdatedRows, DeletedRows: response.DeletedRows}
}

// toProto converts a LanceDB search request into protobuf.
// toProto 将 LanceDB 检索请求转换为 protobuf。
func (r *LanceDBSearchRequest) toProto(clientSessionID string) *pb.SearchLanceDbRequest {
	if r == nil {
		r = &LanceDBSearchRequest{}
	}
	return &pb.SearchLanceDbRequest{ClientSessionId: clientSessionID, SpaceId: r.SpaceID, BindingId: r.BindingID, TableName: r.TableName, Vector: r.Vector, Limit: r.Limit, Filter: r.Filter, VectorColumn: r.VectorColumn, OutputFormat: mapLanceDBOutputFormatToProto(r.OutputFormat)}
}

// mapLanceDBSearchResponse converts protobuf search output into Go-native shape.
// mapLanceDBSearchResponse 将 protobuf 检索输出转换为 Go 原生形态。
func mapLanceDBSearchResponse(response *pb.SearchLanceDbResponse) *LanceDBSearchResponse {
	if response == nil {
		return nil
	}
	return &LanceDBSearchResponse{Message: response.Message, Format: response.Format, Rows: response.Rows, Data: response.Data}
}

// toProto converts a LanceDB delete request into protobuf.
// toProto 将 LanceDB 删除请求转换为 protobuf。
func (r *LanceDBDeleteRequest) toProto(clientSessionID string) *pb.DeleteLanceDbRequest {
	if r == nil {
		r = &LanceDBDeleteRequest{}
	}
	return &pb.DeleteLanceDbRequest{ClientSessionId: clientSessionID, SpaceId: r.SpaceID, BindingId: r.BindingID, TableName: r.TableName, Condition: r.Condition}
}

// mapLanceDBDeleteResponse converts protobuf delete output into Go-native shape.
// mapLanceDBDeleteResponse 将 protobuf 删除输出转换为 Go 原生形态。
func mapLanceDBDeleteResponse(response *pb.DeleteLanceDbResponse) *LanceDBDeleteResponse {
	if response == nil {
		return nil
	}
	return &LanceDBDeleteResponse{Message: response.Message, Version: response.Version, DeletedRows: response.DeletedRows}
}

// toProto converts a LanceDB drop-table request into protobuf.
// toProto 将 LanceDB 删表请求转换为 protobuf。
func (r *LanceDBDropTableRequest) toProto(clientSessionID string) *pb.DropLanceDbTableRequest {
	if r == nil {
		r = &LanceDBDropTableRequest{}
	}
	return &pb.DropLanceDbTableRequest{ClientSessionId: clientSessionID, SpaceId: r.SpaceID, BindingId: r.BindingID, TableName: r.TableName}
}

// mapLanceDBDropTableResponse converts protobuf drop-table output into Go-native shape.
// mapLanceDBDropTableResponse 将 protobuf 删表输出转换为 Go 原生形态。
func mapLanceDBDropTableResponse(response *pb.DropLanceDbTableResponse) *LanceDBDropTableResponse {
	if response == nil {
		return nil
	}
	return &LanceDBDropTableResponse{Message: response.Message}
}

// mapProcessModeFromProto converts protobuf process mode into Go-native mode.
// mapProcessModeFromProto 将 protobuf 进程模式转换为 Go 原生模式。
func mapProcessModeFromProto(mode pb.ControllerProcessMode) ProcessMode {
	if mode == pb.ControllerProcessMode_CONTROLLER_PROCESS_MODE_SERVICE {
		return ProcessModeService
	}
	return ProcessModeManaged
}

// mapSpaceKindToProto converts Go-native space kind into protobuf.
// mapSpaceKindToProto 将 Go 原生空间类型转换为 protobuf。
func mapSpaceKindToProto(kind SpaceKind) pb.SpaceKind {
	switch kind {
	case SpaceKindRoot:
		return pb.SpaceKind_SPACE_KIND_ROOT
	case SpaceKindUser:
		return pb.SpaceKind_SPACE_KIND_USER
	case SpaceKindProject:
		return pb.SpaceKind_SPACE_KIND_PROJECT
	default:
		return pb.SpaceKind_SPACE_KIND_UNSPECIFIED
	}
}

// mapSpaceKindFromProto converts protobuf space kind into Go-native kind.
// mapSpaceKindFromProto 将 protobuf 空间类型转换为 Go 原生类型。
func mapSpaceKindFromProto(kind pb.SpaceKind) SpaceKind {
	switch kind {
	case pb.SpaceKind_SPACE_KIND_ROOT:
		return SpaceKindRoot
	case pb.SpaceKind_SPACE_KIND_USER:
		return SpaceKindUser
	case pb.SpaceKind_SPACE_KIND_PROJECT:
		return SpaceKindProject
	default:
		return ""
	}
}

// mapTokenizerModeToProto converts Go-native tokenizer mode into protobuf.
// mapTokenizerModeToProto 将 Go 原生分词模式转换为 protobuf。
func mapTokenizerModeToProto(mode SqliteTokenizerMode) pb.SqliteTokenizerMode {
	if mode == SqliteTokenizerJieba {
		return pb.SqliteTokenizerMode_SQLITE_TOKENIZER_MODE_JIEBA
	}
	return pb.SqliteTokenizerMode_SQLITE_TOKENIZER_MODE_NONE
}

// mapLanceDBColumnTypeToProto converts Go-native LanceDB column type into protobuf.
// mapLanceDBColumnTypeToProto 将 Go 原生 LanceDB 列类型转换为 protobuf。
func mapLanceDBColumnTypeToProto(kind LanceDBColumnType) pb.LanceDbColumnType {
	switch kind {
	case LanceDBColumnTypeString:
		return pb.LanceDbColumnType_LANCEDB_COLUMN_TYPE_STRING
	case LanceDBColumnTypeInt64:
		return pb.LanceDbColumnType_LANCEDB_COLUMN_TYPE_INT64
	case LanceDBColumnTypeFloat64:
		return pb.LanceDbColumnType_LANCEDB_COLUMN_TYPE_FLOAT64
	case LanceDBColumnTypeBool:
		return pb.LanceDbColumnType_LANCEDB_COLUMN_TYPE_BOOL
	case LanceDBColumnTypeVectorFloat32:
		return pb.LanceDbColumnType_LANCEDB_COLUMN_TYPE_VECTOR_FLOAT32
	case LanceDBColumnTypeFloat32:
		return pb.LanceDbColumnType_LANCEDB_COLUMN_TYPE_FLOAT32
	case LanceDBColumnTypeUint64:
		return pb.LanceDbColumnType_LANCEDB_COLUMN_TYPE_UINT64
	case LanceDBColumnTypeInt32:
		return pb.LanceDbColumnType_LANCEDB_COLUMN_TYPE_INT32
	case LanceDBColumnTypeUint32:
		return pb.LanceDbColumnType_LANCEDB_COLUMN_TYPE_UINT32
	default:
		return pb.LanceDbColumnType_LANCEDB_COLUMN_TYPE_UNSPECIFIED
	}
}

// mapLanceDBInputFormatToProto converts Go-native LanceDB input format into protobuf.
// mapLanceDBInputFormatToProto 将 Go 原生 LanceDB 输入格式转换为 protobuf。
func mapLanceDBInputFormatToProto(format LanceDBInputFormat) pb.LanceDbInputFormat {
	if format == LanceDBInputFormatJSONRows {
		return pb.LanceDbInputFormat_LANCEDB_INPUT_FORMAT_JSON_ROWS
	}
	if format == LanceDBInputFormatArrowIPC {
		return pb.LanceDbInputFormat_LANCEDB_INPUT_FORMAT_ARROW_IPC
	}
	return pb.LanceDbInputFormat_LANCEDB_INPUT_FORMAT_UNSPECIFIED
}

// mapLanceDBOutputFormatToProto converts Go-native LanceDB output format into protobuf.
// mapLanceDBOutputFormatToProto 将 Go 原生 LanceDB 输出格式转换为 protobuf。
func mapLanceDBOutputFormatToProto(format LanceDBOutputFormat) pb.LanceDbOutputFormat {
	if format == LanceDBOutputFormatJSONRows {
		return pb.LanceDbOutputFormat_LANCEDB_OUTPUT_FORMAT_JSON_ROWS
	}
	if format == LanceDBOutputFormatArrowIPC {
		return pb.LanceDbOutputFormat_LANCEDB_OUTPUT_FORMAT_ARROW_IPC
	}
	return pb.LanceDbOutputFormat_LANCEDB_OUTPUT_FORMAT_UNSPECIFIED
}

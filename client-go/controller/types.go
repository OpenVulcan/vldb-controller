package controller

import "time"

// ClientRegistration contains stable host-supplied diagnostic registration fields.
// ClientRegistration 包含宿主提供的稳定诊断注册字段。
type ClientRegistration struct {
	// ClientName is diagnostic-only and may repeat across host processes.
	// ClientName 仅用于诊断，并允许在多个宿主进程间重复。
	ClientName string
	// HostKind identifies the host category.
	// HostKind 标识宿主类别。
	HostKind string
	// ProcessID is the host operating-system process identifier.
	// ProcessID 是宿主操作系统进程标识符。
	ProcessID uint32
	// ProcessName is the host process display name.
	// ProcessName 是宿主进程显示名称。
	ProcessName string
	// LeaseTTL optionally overrides the controller default lease TTL.
	// LeaseTTL 可选覆盖 controller 默认租约 TTL。
	LeaseTTL time.Duration
}

// SpaceKind identifies one logical runtime space kind.
// SpaceKind 标识一种逻辑运行时空间类型。
type SpaceKind string

const (
	// SpaceKindRoot selects the root runtime space kind.
	// SpaceKindRoot 选择根运行时空间类型。
	SpaceKindRoot SpaceKind = "root"
	// SpaceKindUser selects the user runtime space kind.
	// SpaceKindUser 选择用户运行时空间类型。
	SpaceKindUser SpaceKind = "user"
	// SpaceKindProject selects the project runtime space kind.
	// SpaceKindProject 选择项目运行时空间类型。
	SpaceKindProject SpaceKind = "project"
)

// SpaceRegistration describes one runtime space attachment.
// SpaceRegistration 描述一个运行时空间附着。
type SpaceRegistration struct {
	// SpaceID is the stable runtime space identifier.
	// SpaceID 是稳定运行时空间标识符。
	SpaceID string
	// SpaceLabel is the human-readable space label.
	// SpaceLabel 是人类可读空间标签。
	SpaceLabel string
	// SpaceKind is the logical space kind.
	// SpaceKind 是逻辑空间类型。
	SpaceKind SpaceKind
	// SpaceRoot is the physical runtime space root path.
	// SpaceRoot 是物理运行时空间根路径。
	SpaceRoot string
}

// StatusSnapshot describes one controller runtime status response.
// StatusSnapshot 描述一次 controller 运行状态响应。
type StatusSnapshot struct {
	// ProcessMode is the current lifecycle mode.
	// ProcessMode 是当前生命周期模式。
	ProcessMode ProcessMode
	// BindAddr is the controller bind address.
	// BindAddr 是 controller 绑定地址。
	BindAddr string
	// StartedAtUnixMs is the controller start timestamp in milliseconds.
	// StartedAtUnixMs 是 controller 启动时间戳，单位为毫秒。
	StartedAtUnixMs uint64
	// LastRequestAtUnixMs is the last non-diagnostic request timestamp in milliseconds.
	// LastRequestAtUnixMs 是最近一次非诊断请求时间戳，单位为毫秒。
	LastRequestAtUnixMs uint64
	// MinimumUptimeSecs is the minimum managed-process uptime.
	// MinimumUptimeSecs 是托管进程最小存活秒数。
	MinimumUptimeSecs uint64
	// IdleTimeoutSecs is the managed-process idle timeout.
	// IdleTimeoutSecs 是托管进程空闲超时秒数。
	IdleTimeoutSecs uint64
	// DefaultLeaseTTLSecs is the default client lease TTL.
	// DefaultLeaseTTLSecs 是默认客户端租约 TTL 秒数。
	DefaultLeaseTTLSecs uint64
	// ActiveClients is the active client count.
	// ActiveClients 是活跃客户端数量。
	ActiveClients uint32
	// AttachedSpaces is the attached space count.
	// AttachedSpaces 是已附着空间数量。
	AttachedSpaces uint32
	// InflightRequests is the current in-flight request count.
	// InflightRequests 是当前进行中的请求数量。
	InflightRequests uint32
	// ShutdownCandidate reports whether managed shutdown is currently allowed.
	// ShutdownCandidate 表示当前是否满足托管自停条件。
	ShutdownCandidate bool
}

// ClientLeaseSnapshot describes one registered client lease.
// ClientLeaseSnapshot 描述一个已注册客户端租约。
type ClientLeaseSnapshot struct {
	// ClientSessionID is the controller-assigned session identifier.
	// ClientSessionID 是 controller 分配的会话标识符。
	ClientSessionID string
	// ClientName is the diagnostic client name.
	// ClientName 是诊断客户端名称。
	ClientName string
	// HostKind identifies the host category.
	// HostKind 标识宿主类别。
	HostKind string
	// ProcessID is the host operating-system process identifier.
	// ProcessID 是宿主操作系统进程标识符。
	ProcessID uint32
	// ProcessName is the host process name.
	// ProcessName 是宿主进程名称。
	ProcessName string
	// LastSeenUnixMs is the last lease activity timestamp.
	// LastSeenUnixMs 是最近一次租约活动时间戳。
	LastSeenUnixMs uint64
	// ExpiresAtUnixMs is the lease expiration timestamp.
	// ExpiresAtUnixMs 是租约过期时间戳。
	ExpiresAtUnixMs uint64
	// AttachedSpaceIDs lists space identifiers attached by this client.
	// AttachedSpaceIDs 列出该客户端附着的空间标识。
	AttachedSpaceIDs []string
}

// BackendStatus describes one enabled backend state.
// BackendStatus 描述一个已启用后端状态。
type BackendStatus struct {
	// Enabled reports whether the backend is enabled.
	// Enabled 表示后端是否已启用。
	Enabled bool
	// Mode is the backend mode name.
	// Mode 是后端模式名称。
	Mode string
	// Target is the backend target path or logical endpoint.
	// Target 是后端目标路径或逻辑端点。
	Target string
}

// SpaceSnapshot describes one attached runtime space.
// SpaceSnapshot 描述一个已附着运行时空间。
type SpaceSnapshot struct {
	// SpaceID is the stable runtime space identifier.
	// SpaceID 是稳定运行时空间标识符。
	SpaceID string
	// SpaceLabel is the human-readable space label.
	// SpaceLabel 是人类可读空间标签。
	SpaceLabel string
	// SpaceKind is the logical space kind.
	// SpaceKind 是逻辑空间类型。
	SpaceKind SpaceKind
	// SpaceRoot is the physical runtime space root path.
	// SpaceRoot 是物理运行时空间根路径。
	SpaceRoot string
	// AttachedClients is the number of attached clients.
	// AttachedClients 是已附着客户端数量。
	AttachedClients uint32
	// SQLite describes the SQLite backend status when present.
	// SQLite 描述存在时的 SQLite 后端状态。
	SQLite *BackendStatus
	// LanceDB describes the LanceDB backend status when present.
	// LanceDB 描述存在时的 LanceDB 后端状态。
	LanceDB *BackendStatus
}

// SqliteEnableRequest configures one SQLite backend binding.
// SqliteEnableRequest 配置一个 SQLite 后端绑定。
type SqliteEnableRequest struct {
	// SpaceID is the target runtime space identifier.
	// SpaceID 是目标运行时空间标识。
	SpaceID string
	// BindingID is the backend binding identifier.
	// BindingID 是后端绑定标识。
	BindingID string
	// DBPath is the SQLite database path.
	// DBPath 是 SQLite 数据库路径。
	DBPath string
	// ConnectionPoolSize is the desired connection pool size.
	// ConnectionPoolSize 是期望连接池大小。
	ConnectionPoolSize uint32
	// BusyTimeoutMs is the SQLite busy timeout in milliseconds.
	// BusyTimeoutMs 是 SQLite busy timeout 毫秒数。
	BusyTimeoutMs uint64
	// JournalMode is the SQLite journal mode.
	// JournalMode 是 SQLite journal 模式。
	JournalMode string
	// Synchronous is the SQLite synchronous mode.
	// Synchronous 是 SQLite synchronous 模式。
	Synchronous string
	// ForeignKeys toggles foreign-key enforcement.
	// ForeignKeys 控制外键约束。
	ForeignKeys bool
	// TempStore configures SQLite temp_store.
	// TempStore 配置 SQLite temp_store。
	TempStore string
	// WalAutocheckpointPages configures WAL autocheckpoint pages.
	// WalAutocheckpointPages 配置 WAL 自动检查点页数。
	WalAutocheckpointPages uint32
	// CacheSizeKib configures SQLite cache size in KiB.
	// CacheSizeKib 配置 SQLite 缓存大小，单位 KiB。
	CacheSizeKib int64
	// MmapSizeBytes configures SQLite mmap size.
	// MmapSizeBytes 配置 SQLite mmap 大小。
	MmapSizeBytes uint64
	// EnforceDBFileLock toggles database file locking enforcement.
	// EnforceDBFileLock 控制数据库文件锁强制校验。
	EnforceDBFileLock bool
	// ReadOnly opens the database in read-only mode.
	// ReadOnly 以只读模式打开数据库。
	ReadOnly bool
	// AllowURIFilenames permits SQLite URI filenames.
	// AllowURIFilenames 允许 SQLite URI 文件名。
	AllowURIFilenames bool
	// TrustedSchema toggles SQLite trusted_schema.
	// TrustedSchema 控制 SQLite trusted_schema。
	TrustedSchema bool
	// Defensive toggles SQLite defensive mode.
	// Defensive 控制 SQLite defensive 模式。
	Defensive bool
}

// SqliteValueKind identifies the concrete SQLite value variant.
// SqliteValueKind 标识 SQLite 值的具体变体。
type SqliteValueKind string

const (
	// SqliteValueNull stores a null value.
	// SqliteValueNull 存储空值。
	SqliteValueNull SqliteValueKind = "null"
	// SqliteValueInt64 stores an int64 value.
	// SqliteValueInt64 存储 int64 值。
	SqliteValueInt64 SqliteValueKind = "int64"
	// SqliteValueFloat64 stores a float64 value.
	// SqliteValueFloat64 存储 float64 值。
	SqliteValueFloat64 SqliteValueKind = "float64"
	// SqliteValueString stores a string value.
	// SqliteValueString 存储字符串值。
	SqliteValueString SqliteValueKind = "string"
	// SqliteValueBytes stores a bytes value.
	// SqliteValueBytes 存储字节值。
	SqliteValueBytes SqliteValueKind = "bytes"
	// SqliteValueBool stores a bool value.
	// SqliteValueBool 存储 bool 值。
	SqliteValueBool SqliteValueKind = "bool"
)

// SqliteValue stores one typed SQLite parameter value.
// SqliteValue 存储一个类型化 SQLite 参数值。
type SqliteValue struct {
	// Kind selects the active value field.
	// Kind 选择当前生效的值字段。
	Kind SqliteValueKind
	// Int64Value stores an int64 value.
	// Int64Value 存储 int64 值。
	Int64Value int64
	// Float64Value stores a float64 value.
	// Float64Value 存储 float64 值。
	Float64Value float64
	// StringValue stores a string value.
	// StringValue 存储字符串值。
	StringValue string
	// BytesValue stores a bytes value.
	// BytesValue 存储字节值。
	BytesValue []byte
	// BoolValue stores a bool value.
	// BoolValue 存储 bool 值。
	BoolValue bool
}

// SqliteExecuteScriptRequest describes one SQLite script execution.
// SqliteExecuteScriptRequest 描述一次 SQLite 脚本执行。
type SqliteExecuteScriptRequest struct {
	// SpaceID is the target runtime space identifier.
	// SpaceID 是目标运行时空间标识。
	SpaceID string
	// BindingID is the backend binding identifier.
	// BindingID 是后端绑定标识。
	BindingID string
	// SQL is the script text.
	// SQL 是脚本文本。
	SQL string
	// Params are typed SQLite parameters.
	// Params 是类型化 SQLite 参数。
	Params []SqliteValue
}

// SqliteExecuteScriptResponse returns one SQLite script result.
// SqliteExecuteScriptResponse 返回一次 SQLite 脚本结果。
type SqliteExecuteScriptResponse struct {
	// Success reports whether execution succeeded.
	// Success 表示执行是否成功。
	Success bool
	// Message is the controller response message.
	// Message 是 controller 响应消息。
	Message string
	// RowsChanged is the affected row count.
	// RowsChanged 是受影响行数。
	RowsChanged int64
	// LastInsertRowID is the last inserted row id.
	// LastInsertRowID 是最近插入行 ID。
	LastInsertRowID int64
}

// SqliteExecuteBatchItem stores one batch parameter group.
// SqliteExecuteBatchItem 存储一组批量参数。
type SqliteExecuteBatchItem struct {
	// Params are typed SQLite parameters.
	// Params 是类型化 SQLite 参数。
	Params []SqliteValue
}

// SqliteExecuteBatchRequest describes one SQLite batch execution.
// SqliteExecuteBatchRequest 描述一次 SQLite 批量执行。
type SqliteExecuteBatchRequest struct {
	// SpaceID is the target runtime space identifier.
	// SpaceID 是目标运行时空间标识。
	SpaceID string
	// BindingID is the backend binding identifier.
	// BindingID 是后端绑定标识。
	BindingID string
	// SQL is the batch SQL text.
	// SQL 是批量 SQL 文本。
	SQL string
	// Items are parameter groups.
	// Items 是参数组。
	Items []SqliteExecuteBatchItem
}

// SqliteExecuteBatchResponse returns one SQLite batch result.
// SqliteExecuteBatchResponse 返回一次 SQLite 批量结果。
type SqliteExecuteBatchResponse struct {
	// Success reports whether execution succeeded.
	// Success 表示执行是否成功。
	Success bool
	// Message is the controller response message.
	// Message 是 controller 响应消息。
	Message string
	// RowsChanged is the affected row count.
	// RowsChanged 是受影响行数。
	RowsChanged int64
	// LastInsertRowID is the last inserted row id.
	// LastInsertRowID 是最近插入行 ID。
	LastInsertRowID int64
	// StatementsExecuted is the executed statement count.
	// StatementsExecuted 是已执行语句数量。
	StatementsExecuted int64
}

// SqliteQueryJSONRequest describes one SQLite JSON query.
// SqliteQueryJSONRequest 描述一次 SQLite JSON 查询。
type SqliteQueryJSONRequest struct {
	// SpaceID is the target runtime space identifier.
	// SpaceID 是目标运行时空间标识。
	SpaceID string
	// BindingID is the backend binding identifier.
	// BindingID 是后端绑定标识。
	BindingID string
	// SQL is the query SQL text.
	// SQL 是查询 SQL 文本。
	SQL string
	// Params are typed SQLite parameters.
	// Params 是类型化 SQLite 参数。
	Params []SqliteValue
}

// SqliteQueryJSONResponse returns one SQLite JSON query result.
// SqliteQueryJSONResponse 返回一次 SQLite JSON 查询结果。
type SqliteQueryJSONResponse struct {
	// JSONData stores the JSON encoded rows.
	// JSONData 存储 JSON 编码行数据。
	JSONData string
	// RowCount is the returned row count.
	// RowCount 是返回行数。
	RowCount uint64
}

// SqliteQueryStreamRequest describes one SQLite streaming query.
// SqliteQueryStreamRequest 描述一次 SQLite 流式查询。
type SqliteQueryStreamRequest struct {
	// SpaceID is the target runtime space identifier.
	// SpaceID 是目标运行时空间标识。
	SpaceID string
	// BindingID is the backend binding identifier.
	// BindingID 是后端绑定标识。
	BindingID string
	// SQL is the query SQL text.
	// SQL 是查询 SQL 文本。
	SQL string
	// Params are typed SQLite parameters.
	// Params 是类型化 SQLite 参数。
	Params []SqliteValue
	// TargetChunkSize is the preferred stream chunk size.
	// TargetChunkSize 是期望流分块大小。
	TargetChunkSize uint64
}

// SqliteQueryStreamResponse returns one stream open result.
// SqliteQueryStreamResponse 返回一次流打开结果。
type SqliteQueryStreamResponse struct {
	// StreamID is the controller stream identifier.
	// StreamID 是 controller 流标识。
	StreamID uint64
	// MetricsReady reports whether terminal metrics are ready.
	// MetricsReady 表示终态指标是否已就绪。
	MetricsReady bool
}

// SqliteQueryStreamWaitMetricsRequest waits for stream metrics.
// SqliteQueryStreamWaitMetricsRequest 等待查询流指标。
type SqliteQueryStreamWaitMetricsRequest struct {
	// StreamID is the controller stream identifier.
	// StreamID 是 controller 流标识。
	StreamID uint64
}

// SqliteQueryStreamWaitMetricsResponse returns stream metrics.
// SqliteQueryStreamWaitMetricsResponse 返回查询流指标。
type SqliteQueryStreamWaitMetricsResponse struct {
	// RowCount is the row count.
	// RowCount 是行数。
	RowCount uint64
	// ChunkCount is the chunk count.
	// ChunkCount 是分块数量。
	ChunkCount uint64
	// TotalBytes is the total byte count.
	// TotalBytes 是总字节数。
	TotalBytes uint64
}

// SqliteQueryStreamChunkRequest reads one stream chunk.
// SqliteQueryStreamChunkRequest 读取一个查询流分块。
type SqliteQueryStreamChunkRequest struct {
	// StreamID is the controller stream identifier.
	// StreamID 是 controller 流标识。
	StreamID uint64
	// Index is the chunk index.
	// Index 是分块索引。
	Index uint64
}

// SqliteQueryStreamChunkResponse returns one stream chunk.
// SqliteQueryStreamChunkResponse 返回一个查询流分块。
type SqliteQueryStreamChunkResponse struct {
	// Chunk is the raw chunk payload.
	// Chunk 是原始分块载荷。
	Chunk []byte
}

// SqliteQueryStreamCloseRequest closes one stream.
// SqliteQueryStreamCloseRequest 关闭一个查询流。
type SqliteQueryStreamCloseRequest struct {
	// StreamID is the controller stream identifier.
	// StreamID 是 controller 流标识。
	StreamID uint64
}

// SqliteQueryStreamCloseResponse returns stream close status.
// SqliteQueryStreamCloseResponse 返回查询流关闭状态。
type SqliteQueryStreamCloseResponse struct {
	// Closed reports whether the stream was closed.
	// Closed 表示流是否已关闭。
	Closed bool
}

// SqliteTokenizerMode identifies one SQLite tokenizer mode.
// SqliteTokenizerMode 标识一种 SQLite 分词模式。
type SqliteTokenizerMode string

const (
	// SqliteTokenizerNone disables tokenizer integration.
	// SqliteTokenizerNone 禁用分词器集成。
	SqliteTokenizerNone SqliteTokenizerMode = "none"
	// SqliteTokenizerJieba selects Jieba tokenization.
	// SqliteTokenizerJieba 选择 Jieba 分词。
	SqliteTokenizerJieba SqliteTokenizerMode = "jieba"
)

// SqliteTokenizeTextRequest tokenizes text through one SQLite backend.
// SqliteTokenizeTextRequest 通过一个 SQLite 后端对文本分词。
type SqliteTokenizeTextRequest struct {
	// SpaceID is the target runtime space identifier.
	// SpaceID 是目标运行时空间标识。
	SpaceID string
	// BindingID is the backend binding identifier.
	// BindingID 是后端绑定标识。
	BindingID string
	// TokenizerMode selects the tokenizer mode.
	// TokenizerMode 选择分词模式。
	TokenizerMode SqliteTokenizerMode
	// Text is the input text.
	// Text 是输入文本。
	Text string
	// SearchMode toggles search-mode tokenization.
	// SearchMode 控制搜索模式分词。
	SearchMode bool
}

// SqliteTokenizeTextResponse returns tokenization output.
// SqliteTokenizeTextResponse 返回分词输出。
type SqliteTokenizeTextResponse struct {
	// TokenizerMode is the resolved tokenizer mode.
	// TokenizerMode 是解析后的分词模式。
	TokenizerMode string
	// NormalizedText is the normalized input text.
	// NormalizedText 是归一化后的输入文本。
	NormalizedText string
	// Tokens are emitted tokens.
	// Tokens 是输出词元。
	Tokens []string
	// FTSQuery is the generated FTS query text.
	// FTSQuery 是生成的 FTS 查询文本。
	FTSQuery string
}

// SqliteBackendRequest identifies one SQLite backend binding.
// SqliteBackendRequest 标识一个 SQLite 后端绑定。
type SqliteBackendRequest struct {
	// SpaceID is the target runtime space identifier.
	// SpaceID 是目标运行时空间标识。
	SpaceID string
	// BindingID is the backend binding identifier.
	// BindingID 是后端绑定标识。
	BindingID string
}

// SqliteCustomWordEntry describes one custom tokenizer word.
// SqliteCustomWordEntry 描述一个自定义分词词条。
type SqliteCustomWordEntry struct {
	// Word is the custom word text.
	// Word 是自定义词文本。
	Word string
	// Weight is the custom word weight.
	// Weight 是自定义词权重。
	Weight uint64
}

// SqliteListCustomWordsResponse returns custom tokenizer words.
// SqliteListCustomWordsResponse 返回自定义分词词条。
type SqliteListCustomWordsResponse struct {
	// Success reports whether the operation succeeded.
	// Success 表示操作是否成功。
	Success bool
	// Message is the controller response message.
	// Message 是 controller 响应消息。
	Message string
	// Words are custom tokenizer entries.
	// Words 是自定义分词词条。
	Words []SqliteCustomWordEntry
}

// SqliteCustomWordRequest mutates one custom tokenizer word.
// SqliteCustomWordRequest 变更一个自定义分词词条。
type SqliteCustomWordRequest struct {
	// SpaceID is the target runtime space identifier.
	// SpaceID 是目标运行时空间标识。
	SpaceID string
	// BindingID is the backend binding identifier.
	// BindingID 是后端绑定标识。
	BindingID string
	// Word is the custom word text.
	// Word 是自定义词文本。
	Word string
	// Weight is the custom word weight.
	// Weight 是自定义词权重。
	Weight uint32
}

// SqliteDictionaryMutationResponse returns dictionary mutation status.
// SqliteDictionaryMutationResponse 返回词典变更状态。
type SqliteDictionaryMutationResponse struct {
	// Success reports whether the operation succeeded.
	// Success 表示操作是否成功。
	Success bool
	// Message is the controller response message.
	// Message 是 controller 响应消息。
	Message string
	// AffectedRows is the affected row count.
	// AffectedRows 是受影响行数。
	AffectedRows uint64
}

// SqliteFTSIndexRequest describes one FTS index operation.
// SqliteFTSIndexRequest 描述一次 FTS 索引操作。
type SqliteFTSIndexRequest struct {
	// SpaceID is the target runtime space identifier.
	// SpaceID 是目标运行时空间标识。
	SpaceID string
	// BindingID is the backend binding identifier.
	// BindingID 是后端绑定标识。
	BindingID string
	// IndexName is the FTS index name.
	// IndexName 是 FTS 索引名称。
	IndexName string
	// TokenizerMode selects the tokenizer mode.
	// TokenizerMode 选择分词模式。
	TokenizerMode SqliteTokenizerMode
}

// SqliteEnsureFTSIndexResponse returns ensure-index status.
// SqliteEnsureFTSIndexResponse 返回确认索引状态。
type SqliteEnsureFTSIndexResponse struct {
	// Success reports whether the operation succeeded.
	// Success 表示操作是否成功。
	Success bool
	// Message is the controller response message.
	// Message 是 controller 响应消息。
	Message string
	// IndexName is the FTS index name.
	// IndexName 是 FTS 索引名称。
	IndexName string
	// TokenizerMode is the resolved tokenizer mode.
	// TokenizerMode 是解析后的分词模式。
	TokenizerMode string
}

// SqliteRebuildFTSIndexResponse returns rebuild-index status.
// SqliteRebuildFTSIndexResponse 返回重建索引状态。
type SqliteRebuildFTSIndexResponse struct {
	// Success reports whether the operation succeeded.
	// Success 表示操作是否成功。
	Success bool
	// Message is the controller response message.
	// Message 是 controller 响应消息。
	Message string
	// IndexName is the FTS index name.
	// IndexName 是 FTS 索引名称。
	IndexName string
	// TokenizerMode is the resolved tokenizer mode.
	// TokenizerMode 是解析后的分词模式。
	TokenizerMode string
	// ReindexedRows is the rebuilt row count.
	// ReindexedRows 是重建行数。
	ReindexedRows uint64
}

// SqliteFTSDocumentRequest describes one FTS document mutation.
// SqliteFTSDocumentRequest 描述一次 FTS 文档变更。
type SqliteFTSDocumentRequest struct {
	// SpaceID is the target runtime space identifier.
	// SpaceID 是目标运行时空间标识。
	SpaceID string
	// BindingID is the backend binding identifier.
	// BindingID 是后端绑定标识。
	BindingID string
	// IndexName is the FTS index name.
	// IndexName 是 FTS 索引名称。
	IndexName string
	// TokenizerMode selects the tokenizer mode.
	// TokenizerMode 选择分词模式。
	TokenizerMode SqliteTokenizerMode
	// ID is the document identifier.
	// ID 是文档标识。
	ID string
	// FilePath is the document file path.
	// FilePath 是文档文件路径。
	FilePath string
	// Title is the document title.
	// Title 是文档标题。
	Title string
	// Content is the document content.
	// Content 是文档内容。
	Content string
}

// SqliteFTSDeleteDocumentRequest describes one FTS document deletion.
// SqliteFTSDeleteDocumentRequest 描述一次 FTS 文档删除。
type SqliteFTSDeleteDocumentRequest struct {
	// SpaceID is the target runtime space identifier.
	// SpaceID 是目标运行时空间标识。
	SpaceID string
	// BindingID is the backend binding identifier.
	// BindingID 是后端绑定标识。
	BindingID string
	// IndexName is the FTS index name.
	// IndexName 是 FTS 索引名称。
	IndexName string
	// ID is the document identifier.
	// ID 是文档标识。
	ID string
}

// SqliteFTSMutationResponse returns FTS mutation status.
// SqliteFTSMutationResponse 返回 FTS 变更状态。
type SqliteFTSMutationResponse struct {
	// Success reports whether the operation succeeded.
	// Success 表示操作是否成功。
	Success bool
	// Message is the controller response message.
	// Message 是 controller 响应消息。
	Message string
	// AffectedRows is the affected row count.
	// AffectedRows 是受影响行数。
	AffectedRows uint64
	// IndexName is the FTS index name.
	// IndexName 是 FTS 索引名称。
	IndexName string
}

// SqliteFTSSearchRequest describes one FTS search.
// SqliteFTSSearchRequest 描述一次 FTS 检索。
type SqliteFTSSearchRequest struct {
	// SpaceID is the target runtime space identifier.
	// SpaceID 是目标运行时空间标识。
	SpaceID string
	// BindingID is the backend binding identifier.
	// BindingID 是后端绑定标识。
	BindingID string
	// IndexName is the FTS index name.
	// IndexName 是 FTS 索引名称。
	IndexName string
	// TokenizerMode selects the tokenizer mode.
	// TokenizerMode 选择分词模式。
	TokenizerMode SqliteTokenizerMode
	// Query is the search query text.
	// Query 是检索文本。
	Query string
	// Limit is the maximum hit count.
	// Limit 是最大命中数量。
	Limit uint32
	// Offset is the hit offset.
	// Offset 是命中偏移。
	Offset uint32
}

// SqliteFTSSearchHit describes one FTS search hit.
// SqliteFTSSearchHit 描述一个 FTS 检索命中。
type SqliteFTSSearchHit struct {
	// ID is the document identifier.
	// ID 是文档标识。
	ID string
	// FilePath is the document file path.
	// FilePath 是文档文件路径。
	FilePath string
	// Title is the document title.
	// Title 是文档标题。
	Title string
	// TitleHighlight is the highlighted title.
	// TitleHighlight 是高亮标题。
	TitleHighlight string
	// ContentSnippet is the highlighted content snippet.
	// ContentSnippet 是高亮内容片段。
	ContentSnippet string
	// Score is the normalized score.
	// Score 是归一化得分。
	Score float64
	// Rank is the raw SQLite FTS rank.
	// Rank 是原始 SQLite FTS rank。
	Rank uint64
	// RawScore is the raw score.
	// RawScore 是原始得分。
	RawScore float64
}

// SqliteFTSSearchResponse returns one FTS search result.
// SqliteFTSSearchResponse 返回一次 FTS 检索结果。
type SqliteFTSSearchResponse struct {
	// Success reports whether the operation succeeded.
	// Success 表示操作是否成功。
	Success bool
	// Message is the controller response message.
	// Message 是 controller 响应消息。
	Message string
	// IndexName is the FTS index name.
	// IndexName 是 FTS 索引名称。
	IndexName string
	// TokenizerMode is the resolved tokenizer mode.
	// TokenizerMode 是解析后的分词模式。
	TokenizerMode string
	// NormalizedQuery is the normalized query.
	// NormalizedQuery 是归一化查询。
	NormalizedQuery string
	// FTSQuery is the generated FTS query.
	// FTSQuery 是生成的 FTS 查询。
	FTSQuery string
	// Source is the search source.
	// Source 是检索来源。
	Source string
	// QueryMode is the query mode.
	// QueryMode 是查询模式。
	QueryMode string
	// Total is the total hit count.
	// Total 是总命中数。
	Total uint64
	// Hits are search hits.
	// Hits 是检索命中。
	Hits []SqliteFTSSearchHit
}

// LanceDBEnableRequest configures one LanceDB backend binding.
// LanceDBEnableRequest 配置一个 LanceDB 后端绑定。
type LanceDBEnableRequest struct {
	// SpaceID is the target runtime space identifier.
	// SpaceID 是目标运行时空间标识。
	SpaceID string
	// BindingID is the backend binding identifier.
	// BindingID 是后端绑定标识。
	BindingID string
	// DefaultDBPath is the default database path.
	// DefaultDBPath 是默认数据库路径。
	DefaultDBPath string
	// DBRoot is the optional database root directory.
	// DBRoot 是可选数据库根目录。
	DBRoot string
	// ReadConsistencyIntervalMs is the read-consistency interval.
	// ReadConsistencyIntervalMs 是读一致性间隔毫秒数。
	ReadConsistencyIntervalMs uint64
	// MaxUpsertPayload is the maximum upsert payload size.
	// MaxUpsertPayload 是最大写入载荷大小。
	MaxUpsertPayload uint64
	// MaxSearchLimit is the maximum search limit.
	// MaxSearchLimit 是最大检索限制。
	MaxSearchLimit uint64
	// MaxConcurrentRequests is the maximum concurrent request count.
	// MaxConcurrentRequests 是最大并发请求数。
	MaxConcurrentRequests uint64
}

// LanceDBColumnType identifies one LanceDB column type.
// LanceDBColumnType 标识一个 LanceDB 列类型。
type LanceDBColumnType string

const (
	// LanceDBColumnTypeUnspecified leaves the column type unspecified.
	// LanceDBColumnTypeUnspecified 表示未指定列类型。
	LanceDBColumnTypeUnspecified LanceDBColumnType = "unspecified"
	// LanceDBColumnTypeString selects a string LanceDB column.
	// LanceDBColumnTypeString 选择 LanceDB 字符串列。
	LanceDBColumnTypeString LanceDBColumnType = "string"
	// LanceDBColumnTypeInt64 selects an int64 LanceDB column.
	// LanceDBColumnTypeInt64 选择 LanceDB int64 列。
	LanceDBColumnTypeInt64 LanceDBColumnType = "int64"
	// LanceDBColumnTypeFloat64 selects a float64 LanceDB column.
	// LanceDBColumnTypeFloat64 选择 LanceDB float64 列。
	LanceDBColumnTypeFloat64 LanceDBColumnType = "float64"
	// LanceDBColumnTypeBool selects a bool LanceDB column.
	// LanceDBColumnTypeBool 选择 LanceDB bool 列。
	LanceDBColumnTypeBool LanceDBColumnType = "bool"
	// LanceDBColumnTypeVectorFloat32 selects a float32 vector LanceDB column.
	// LanceDBColumnTypeVectorFloat32 选择 LanceDB float32 向量列。
	LanceDBColumnTypeVectorFloat32 LanceDBColumnType = "vector_float32"
	// LanceDBColumnTypeFloat32 selects a float32 LanceDB column.
	// LanceDBColumnTypeFloat32 选择 LanceDB float32 列。
	LanceDBColumnTypeFloat32 LanceDBColumnType = "float32"
	// LanceDBColumnTypeUint64 selects a uint64 LanceDB column.
	// LanceDBColumnTypeUint64 选择 LanceDB uint64 列。
	LanceDBColumnTypeUint64 LanceDBColumnType = "uint64"
	// LanceDBColumnTypeInt32 selects an int32 LanceDB column.
	// LanceDBColumnTypeInt32 选择 LanceDB int32 列。
	LanceDBColumnTypeInt32 LanceDBColumnType = "int32"
	// LanceDBColumnTypeUint32 selects a uint32 LanceDB column.
	// LanceDBColumnTypeUint32 选择 LanceDB uint32 列。
	LanceDBColumnTypeUint32 LanceDBColumnType = "uint32"
)

// LanceDBColumnDef describes one LanceDB table column.
// LanceDBColumnDef 描述一个 LanceDB 表列。
type LanceDBColumnDef struct {
	// Name is the column name.
	// Name 是列名。
	Name string
	// ColumnType is the column type.
	// ColumnType 是列类型。
	ColumnType LanceDBColumnType
	// VectorDim is the vector dimension for vector columns.
	// VectorDim 是向量列维度。
	VectorDim uint32
	// Nullable reports whether the column is nullable.
	// Nullable 表示列是否可空。
	Nullable bool
}

// LanceDBInputFormat identifies one LanceDB input payload format.
// LanceDBInputFormat 标识一种 LanceDB 输入载荷格式。
type LanceDBInputFormat string

const (
	// LanceDBInputFormatUnspecified leaves the input format unspecified.
	// LanceDBInputFormatUnspecified 表示未指定输入格式。
	LanceDBInputFormatUnspecified LanceDBInputFormat = "unspecified"
	// LanceDBInputFormatJSONRows selects JSON rows input.
	// LanceDBInputFormatJSONRows 选择 JSON 行输入。
	LanceDBInputFormatJSONRows LanceDBInputFormat = "json_rows"
	// LanceDBInputFormatArrowIPC selects Arrow IPC input.
	// LanceDBInputFormatArrowIPC 选择 Arrow IPC 输入。
	LanceDBInputFormatArrowIPC LanceDBInputFormat = "arrow_ipc"
)

// LanceDBOutputFormat identifies one LanceDB output payload format.
// LanceDBOutputFormat 标识一种 LanceDB 输出载荷格式。
type LanceDBOutputFormat string

const (
	// LanceDBOutputFormatUnspecified leaves the output format unspecified.
	// LanceDBOutputFormatUnspecified 表示未指定输出格式。
	LanceDBOutputFormatUnspecified LanceDBOutputFormat = "unspecified"
	// LanceDBOutputFormatArrowIPC selects Arrow IPC output.
	// LanceDBOutputFormatArrowIPC 选择 Arrow IPC 输出。
	LanceDBOutputFormatArrowIPC LanceDBOutputFormat = "arrow_ipc"
	// LanceDBOutputFormatJSONRows selects JSON rows output.
	// LanceDBOutputFormatJSONRows 选择 JSON 行输出。
	LanceDBOutputFormatJSONRows LanceDBOutputFormat = "json_rows"
)

// LanceDBCreateTableRequest describes one LanceDB table creation.
// LanceDBCreateTableRequest 描述一次 LanceDB 建表。
type LanceDBCreateTableRequest struct {
	// SpaceID is the target runtime space identifier.
	// SpaceID 是目标运行时空间标识。
	SpaceID string
	// BindingID is the backend binding identifier.
	// BindingID 是后端绑定标识。
	BindingID string
	// TableName is the target table name.
	// TableName 是目标表名。
	TableName string
	// Columns are table column definitions.
	// Columns 是表列定义。
	Columns []LanceDBColumnDef
	// OverwriteIfExists controls replacement of an existing table.
	// OverwriteIfExists 控制是否替换已有表。
	OverwriteIfExists bool
}

// LanceDBCreateTableResponse returns one LanceDB table creation result.
// LanceDBCreateTableResponse 返回一次 LanceDB 建表结果。
type LanceDBCreateTableResponse struct {
	// Message is the controller response message.
	// Message 是 controller 响应消息。
	Message string
}

// LanceDBUpsertRequest describes one LanceDB upsert operation.
// LanceDBUpsertRequest 描述一次 LanceDB 写入操作。
type LanceDBUpsertRequest struct {
	// SpaceID is the target runtime space identifier.
	// SpaceID 是目标运行时空间标识。
	SpaceID string
	// BindingID is the backend binding identifier.
	// BindingID 是后端绑定标识。
	BindingID string
	// TableName is the target table name.
	// TableName 是目标表名。
	TableName string
	// InputFormat is the payload format.
	// InputFormat 是载荷格式。
	InputFormat LanceDBInputFormat
	// Data is the raw payload.
	// Data 是原始载荷。
	Data []byte
	// KeyColumns are upsert key columns.
	// KeyColumns 是写入键列。
	KeyColumns []string
}

// LanceDBUpsertResponse returns one LanceDB upsert result.
// LanceDBUpsertResponse 返回一次 LanceDB 写入结果。
type LanceDBUpsertResponse struct {
	// Message is the controller response message.
	// Message 是 controller 响应消息。
	Message string
	// Version is the table version.
	// Version 是表版本。
	Version uint64
	// InputRows is the input row count.
	// InputRows 是输入行数。
	InputRows uint64
	// InsertedRows is the inserted row count.
	// InsertedRows 是插入行数。
	InsertedRows uint64
	// UpdatedRows is the updated row count.
	// UpdatedRows 是更新行数。
	UpdatedRows uint64
	// DeletedRows is the deleted row count.
	// DeletedRows 是删除行数。
	DeletedRows uint64
}

// LanceDBSearchRequest describes one LanceDB vector search.
// LanceDBSearchRequest 描述一次 LanceDB 向量检索。
type LanceDBSearchRequest struct {
	// SpaceID is the target runtime space identifier.
	// SpaceID 是目标运行时空间标识。
	SpaceID string
	// BindingID is the backend binding identifier.
	// BindingID 是后端绑定标识。
	BindingID string
	// TableName is the target table name.
	// TableName 是目标表名。
	TableName string
	// Vector is the query vector.
	// Vector 是查询向量。
	Vector []float32
	// Limit is the maximum result count.
	// Limit 是最大结果数量。
	Limit uint32
	// Filter is the optional filter expression.
	// Filter 是可选过滤表达式。
	Filter string
	// VectorColumn is the vector column name.
	// VectorColumn 是向量列名。
	VectorColumn string
	// OutputFormat is the response payload format.
	// OutputFormat 是响应载荷格式。
	OutputFormat LanceDBOutputFormat
}

// LanceDBSearchResponse returns one LanceDB vector search result.
// LanceDBSearchResponse 返回一次 LanceDB 向量检索结果。
type LanceDBSearchResponse struct {
	// Message is the controller response message.
	// Message 是 controller 响应消息。
	Message string
	// Format is the response data format.
	// Format 是响应数据格式。
	Format string
	// Rows is the returned row count.
	// Rows 是返回行数。
	Rows uint64
	// Data is the raw response payload.
	// Data 是原始响应载荷。
	Data []byte
}

// LanceDBDeleteRequest describes one LanceDB delete operation.
// LanceDBDeleteRequest 描述一次 LanceDB 删除操作。
type LanceDBDeleteRequest struct {
	// SpaceID is the target runtime space identifier.
	// SpaceID 是目标运行时空间标识。
	SpaceID string
	// BindingID is the backend binding identifier.
	// BindingID 是后端绑定标识。
	BindingID string
	// TableName is the target table name.
	// TableName 是目标表名。
	TableName string
	// Condition is the delete predicate.
	// Condition 是删除谓词。
	Condition string
}

// LanceDBDeleteResponse returns one LanceDB delete result.
// LanceDBDeleteResponse 返回一次 LanceDB 删除结果。
type LanceDBDeleteResponse struct {
	// Message is the controller response message.
	// Message 是 controller 响应消息。
	Message string
	// Version is the table version.
	// Version 是表版本。
	Version uint64
	// DeletedRows is the deleted row count.
	// DeletedRows 是删除行数。
	DeletedRows uint64
}

// LanceDBDropTableRequest describes one LanceDB table drop operation.
// LanceDBDropTableRequest 描述一次 LanceDB 删表操作。
type LanceDBDropTableRequest struct {
	// SpaceID is the target runtime space identifier.
	// SpaceID 是目标运行时空间标识。
	SpaceID string
	// BindingID is the backend binding identifier.
	// BindingID 是后端绑定标识。
	BindingID string
	// TableName is the target table name.
	// TableName 是目标表名。
	TableName string
}

// LanceDBDropTableResponse returns one LanceDB table drop result.
// LanceDBDropTableResponse 返回一次 LanceDB 删表结果。
type LanceDBDropTableResponse struct {
	// Message is the controller response message.
	// Message 是 controller 响应消息。
	Message string
}

// sqliteBindingKey identifies one desired SQLite binding.
// sqliteBindingKey 标识一个期望 SQLite 绑定。
type sqliteBindingKey struct {
	// SpaceID is the runtime space identifier.
	// SpaceID 是运行时空间标识符。
	SpaceID string
	// BindingID is the backend binding identifier.
	// BindingID 是后端绑定标识符。
	BindingID string
}

// lancedbBindingKey identifies one desired LanceDB binding.
// lancedbBindingKey 标识一个期望 LanceDB 绑定。
type lancedbBindingKey struct {
	// SpaceID is the runtime space identifier.
	// SpaceID 是运行时空间标识符。
	SpaceID string
	// BindingID is the backend binding identifier.
	// BindingID 是后端绑定标识符。
	BindingID string
}

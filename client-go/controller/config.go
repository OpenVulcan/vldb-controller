package controller

import "time"

// ProcessMode controls the controller process lifecycle.
// ProcessMode 控制 controller 进程生命周期。
type ProcessMode string

const (
	// ProcessModeService keeps the controller alive until external shutdown.
	// ProcessModeService 让 controller 保持常驻直到外部关闭。
	ProcessModeService ProcessMode = "service"
	// ProcessModeManaged allows the controller to stop itself when idle.
	// ProcessModeManaged 允许 controller 在空闲时自行退出。
	ProcessModeManaged ProcessMode = "managed"
)

// Config contains controller endpoint discovery and lifecycle settings.
// Config 包含 controller 端点发现与生命周期设置。
type Config struct {
	// Endpoint is the target controller endpoint.
	// Endpoint 是目标 controller 端点。
	Endpoint string
	// AutoSpawn controls whether automatic startup is allowed.
	// AutoSpawn 控制是否允许自动启动。
	AutoSpawn bool
	// SpawnExecutable is the optional controller executable path.
	// SpawnExecutable 是可选的 controller 可执行文件路径。
	SpawnExecutable string
	// SpawnProcessMode is the process mode used for automatic startup.
	// SpawnProcessMode 是自动启动使用的进程模式。
	SpawnProcessMode ProcessMode
	// MinimumUptime is passed to spawned controllers.
	// MinimumUptime 会传递给被启动的 controller。
	MinimumUptime time.Duration
	// IdleTimeout is passed to spawned controllers.
	// IdleTimeout 会传递给被启动的 controller。
	IdleTimeout time.Duration
	// DefaultLeaseTTL is passed to spawned controllers and used by registration defaults.
	// DefaultLeaseTTL 会传递给被启动的 controller 并作为注册默认租约。
	DefaultLeaseTTL time.Duration
	// ConnectTimeout is the per-attempt gRPC connection timeout.
	// ConnectTimeout 是单次 gRPC 连接超时。
	ConnectTimeout time.Duration
	// StartupTimeout is the total wait window after startup.
	// StartupTimeout 是启动后的总等待窗口。
	StartupTimeout time.Duration
	// StartupRetryInterval is the retry interval while waiting for readiness.
	// StartupRetryInterval 是等待就绪时的重试间隔。
	StartupRetryInterval time.Duration
	// LeaseRenewInterval is the background lease renewal interval.
	// LeaseRenewInterval 是后台租约续期间隔。
	LeaseRenewInterval time.Duration
}

// DefaultConfig returns the default shared-controller client configuration.
// DefaultConfig 返回默认共享 controller 客户端配置。
func DefaultConfig() Config {
	return Config{
		Endpoint:             "http://127.0.0.1:19801",
		AutoSpawn:            true,
		SpawnProcessMode:     ProcessModeManaged,
		MinimumUptime:        300 * time.Second,
		IdleTimeout:          900 * time.Second,
		DefaultLeaseTTL:      120 * time.Second,
		ConnectTimeout:       5 * time.Second,
		StartupTimeout:       15 * time.Second,
		StartupRetryInterval: 250 * time.Millisecond,
		LeaseRenewInterval:   30 * time.Second,
	}
}

// normalized fills zero-valued configuration fields with defaults.
// normalized 使用默认值填充零值配置字段。
func (c Config) normalized() Config {
	defaults := DefaultConfig()
	if c.Endpoint == "" {
		c.Endpoint = defaults.Endpoint
	}
	if c.SpawnExecutable == "" {
		c.SpawnExecutable = "vldb-controller"
	}
	if c.SpawnProcessMode == "" {
		c.SpawnProcessMode = defaults.SpawnProcessMode
	}
	if c.MinimumUptime == 0 {
		c.MinimumUptime = defaults.MinimumUptime
	}
	if c.IdleTimeout == 0 {
		c.IdleTimeout = defaults.IdleTimeout
	}
	if c.DefaultLeaseTTL == 0 {
		c.DefaultLeaseTTL = defaults.DefaultLeaseTTL
	}
	if c.ConnectTimeout == 0 {
		c.ConnectTimeout = defaults.ConnectTimeout
	}
	if c.StartupTimeout == 0 {
		c.StartupTimeout = defaults.StartupTimeout
	}
	if c.StartupRetryInterval == 0 {
		c.StartupRetryInterval = defaults.StartupRetryInterval
	}
	if c.LeaseRenewInterval == 0 {
		c.LeaseRenewInterval = defaults.LeaseRenewInterval
	}
	return c
}

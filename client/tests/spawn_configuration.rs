//! Real auto-spawn tests using an isolated fixture instead of a database controller.
//! 使用隔离夹具而非数据库控制器的真实自动启动测试。

use std::process::Command;
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
use std::time::{Duration, Instant};
use vldb_controller_client::{ClientRegistration, ControllerClient, ControllerClientConfig};

#[cfg(windows)]
#[link(name = "kernel32")]
unsafe extern "system" {
    /// Return this process's console window handle, or null when it has no console window.
    /// 返回本进程的控制台窗口句柄；没有控制台窗口时返回空指针。
    fn GetConsoleWindow() -> *mut std::ffi::c_void;
}

/// Write the actual child console observation to the explicit fixture path; ordinary test runs do nothing.
/// 将实际子进程控制台观察写入显式夹具路径；普通测试运行不执行操作。
#[test]
fn spawn_probe_fixture() {
    // The parent configures this path only on the child command, never in global environment state.
    // 父进程仅在子命令上配置此路径，绝不配置全局环境状态。
    let Some(path) = std::env::var_os("VLDB_SPAWN_PROBE") else {
        return;
    };
    #[cfg(windows)]
    {
        // SAFETY: This read-only query observes only the current process and takes no pointers.
        // 安全性：此只读查询仅观察当前进程，不接收指针。
        assert!(unsafe { GetConsoleWindow() }.is_null());
    }
    // Publish the complete observation atomically so readers never see a newly created empty file.
    // 原子发布完整观察，避免读取者看到刚创建的空文件。
    let path = std::path::PathBuf::from(path);
    // Temporary sibling belongs to this single fixture and becomes visible only after writing completes.
    // 同级临时文件属于本单个夹具，仅在写入完成后对观察者可见。
    let temporary = path.with_extension("pending");
    std::fs::write(&temporary, "hidden").unwrap();
    std::fs::rename(temporary, path).unwrap();
}

/// Build stable fixture registration from `name`; returns a complete old-style public struct literal.
/// 根据 `name` 构建稳定夹具注册；返回完整旧式公开结构体字面量。
fn registration(name: &str) -> ClientRegistration {
    ClientRegistration {
        client_name: name.into(),
        host_kind: "test".into(),
        process_id: std::process::id(),
        process_name: "spawn-configuration".into(),
        lease_ttl_secs: Some(30),
    }
}

/// Prove actual auto-spawn calls the shared callback on each fresh startup attempt and hides Windows consoles.
/// 证明真实自动启动在每次新启动尝试时调用共享回调，并隐藏 Windows 控制台。
#[tokio::test]
async fn configured_auto_spawn_survives_client_cloning_and_retry() {
    // Reserve an ephemeral loopback port solely to choose an unused local endpoint for this fixture.
    // 预留临时回环端口，仅用于为本夹具选择未使用的本地端点。
    let reservation = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    // The endpoint cannot refer to any configured production database service.
    // 此端点不会引用任何已配置的生产数据库服务。
    let endpoint = reservation.local_addr().unwrap().to_string();
    drop(reservation);
    // The unchanged public configuration remains constructible without any added required fields.
    // 不变的公开配置仍可在没有新增必填字段的情况下构造。
    let config = ControllerClientConfig {
        endpoint,
        auto_spawn: true,
        spawn_executable: Some(std::env::current_exe().unwrap().to_str().unwrap().into()),
        spawn_process_mode: vldb_controller_client::ControllerProcessMode::Managed,
        minimum_uptime_secs: 0,
        idle_timeout_secs: 0,
        default_lease_ttl_secs: 30,
        connect_timeout_secs: 1,
        startup_timeout_secs: 0,
        startup_retry_interval_ms: 50,
        lease_renew_interval_secs: 30,
    };
    // Round-trip JSON proves no process-customization field is required by the existing wire configuration.
    // JSON 往返证明现有线配置不需要任何进程定制字段。
    let serialized = serde_json::to_string(&config).unwrap();
    assert_eq!(
        serde_json::from_str::<ControllerClientConfig>(&serialized).unwrap(),
        config
    );
    // Legacy construction remains available and performs no startup action.
    // 旧构造方式仍可使用且不执行启动操作。
    let legacy = ControllerClient::new(config.clone(), registration("legacy"));
    drop(legacy);
    // Unique observation directory is removed only after both fixture children publish complete observations.
    // 唯一观察目录仅在两个夹具子进程发布完整观察后移除。
    let directory = std::env::temp_dir().join(format!(
        "vldb-spawn-config-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir(&directory).unwrap();
    // Counts are shared with the callback and all client clones without mutable global state.
    // 计数与回调及所有客户端克隆共享，不使用可变全局状态。
    let calls = Arc::new(AtomicUsize::new(0));
    // The closure owns its observation counter and directory while the outer test retains assertions.
    // 闭包拥有观察计数器及目录，外部测试保留断言。
    let observed = Arc::clone(&calls);
    // This owned path remains valid for every later auto-spawn, including those from a clone.
    // 此自有路径在每次后续自动启动期间均有效，包括来自克隆实例的启动。
    let child_directory = directory.clone();
    // Hook invocation is deferred until the normal startup path actually creates a process.
    // 回调调用推迟到正常启动路径实际创建进程时。
    let client = ControllerClient::new_with_spawn_configurator(
        config,
        registration("configured"),
        move |command| {
            assert!(command.get_args().any(|arg| arg == "--bind"));
            assert!(
                command
                    .get_args()
                    .any(|arg| arg == "--default-lease-ttl-secs")
            );
            // The fixture replaces only the final executable invocation after verifying SDK defaults were applied.
            // 夹具在验证 SDK 默认配置已应用后，仅替换最终可执行调用。
            let index = observed.fetch_add(1, Ordering::SeqCst);
            *command = Command::new(std::env::current_exe().unwrap());
            command.args(["--exact", "spawn_probe_fixture", "--nocapture"]);
            command.env("VLDB_SPAWN_PROBE", child_directory.join(index.to_string()));
            command
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null());
            #[cfg(windows)]
            {
                use std::os::windows::process::CommandExt;
                // CREATE_NO_WINDOW is the documented Windows process creation flag used by desktop hosts.
                // CREATE_NO_WINDOW 是桌面宿主使用的 Windows 文档规定进程创建标志。
                command.creation_flags(0x0800_0000);
            }
        },
    );
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    for (index, owner) in [client.clone(), client].iter().enumerate() {
        assert!(
            owner.connect().await.is_err(),
            "fixture deliberately provides no gRPC service"
        );
        assert_eq!(calls.load(Ordering::SeqCst), index + 1);
        // The bounded wait observes child completion without guessing a fixed launch delay.
        // 有界等待观察子进程完成，不猜测固定启动延迟。
        let deadline = Instant::now() + Duration::from_secs(10);
        while !directory.join(index.to_string()).is_file() {
            assert!(
                Instant::now() < deadline,
                "auto-spawn fixture did not publish its console observation"
            );
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        assert_eq!(
            std::fs::read_to_string(directory.join(index.to_string())).unwrap(),
            "hidden"
        );
    }
    std::fs::remove_dir_all(directory).unwrap();
}

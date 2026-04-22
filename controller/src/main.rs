use std::sync::Arc;

use tonic::transport::Server;
use vldb_controller_client::rpc::controller_service_server::ControllerServiceServer;
use vldb_controller_client::types::ControllerProcessMode;

mod cli;
mod core;
mod server;

use crate::cli::parse_server_config_from_env;
use crate::core::runtime::VldbControllerRuntime;
use crate::server::ControllerGrpcService;

/// Start the controller service process with the parameter-driven runtime configuration.
/// 使用参数驱动运行时配置启动控制器服务进程。
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync + 'static>> {
    let server_config = parse_server_config_from_env()?;
    let bind_addr = server_config.bind_addr.parse()?;
    let runtime = Arc::new(VldbControllerRuntime::new(server_config.clone()));
    let service = ControllerGrpcService::new(Arc::clone(&runtime));
    let shutdown_rx = if server_config.runtime.process_mode == ControllerProcessMode::Managed {
        let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();
        tokio::spawn(ControllerGrpcService::run_runtime_maintenance_loop(
            Arc::clone(&runtime),
            Some(shutdown_tx),
        ));
        Some(shutdown_rx)
    } else {
        tokio::spawn(ControllerGrpcService::run_runtime_maintenance_loop(
            Arc::clone(&runtime),
            None,
        ));
        None
    };

    Server::builder()
        .add_service(ControllerServiceServer::new(service))
        .serve_with_shutdown(bind_addr, async move {
            if let Some(shutdown_rx) = shutdown_rx {
                tokio::select! {
                    _ = tokio::signal::ctrl_c() => {}
                    _ = shutdown_rx => {}
                }
            } else {
                let _ = tokio::signal::ctrl_c().await;
            }
        })
        .await?;

    Ok(())
}

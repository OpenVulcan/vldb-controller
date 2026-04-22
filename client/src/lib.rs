/// Generated protobuf bindings and gRPC client/server contracts.
/// 生成的 protobuf 绑定与 gRPC 客户端/服务端协议。
pub mod rpc {
    tonic::include_proto!("vldb.controller.v1");
}

/// Shared lightweight controller contract types exposed to hosts and the service crate.
/// 向宿主与服务端 crate 暴露的轻量控制器契约类型。
pub mod types;

/// Host-side controller client proxy for discovery, spawning, and lease maintenance.
/// 宿主侧用于发现、唤起与租约维护的控制器客户端代理。
pub mod client;

pub use client::{ControllerClient, ControllerClientConfig};
pub use types::{
    BoxError, ClientLeaseSnapshot, ClientRegistration, ControllerLanceDbColumnDef,
    ControllerLanceDbColumnType, ControllerLanceDbCreateTableResult, ControllerLanceDbDeleteResult,
    ControllerLanceDbDropTableResult, ControllerLanceDbEnableRequest, ControllerLanceDbInputFormat,
    ControllerLanceDbOutputFormat, ControllerLanceDbSearchResult, ControllerLanceDbUpsertResult,
    ControllerProcessMode, ControllerRuntimeConfig, ControllerServerConfig,
    ControllerSqliteCustomWordEntry, ControllerSqliteDictionaryMutationResult,
    ControllerSqliteEnableRequest, ControllerSqliteEnsureFtsIndexResult,
    ControllerSqliteExecuteBatchResult, ControllerSqliteExecuteResult,
    ControllerSqliteFtsMutationResult, ControllerSqliteListCustomWordsResult,
    ControllerSqliteQueryResult, ControllerSqliteQueryStreamMetrics,
    ControllerSqliteQueryStreamOpenResult, ControllerSqliteRebuildFtsIndexResult,
    ControllerSqliteSearchFtsHit, ControllerSqliteSearchFtsResult, ControllerSqliteTokenizeResult,
    ControllerSqliteTokenizerMode, ControllerSqliteValue, ControllerStatusSnapshot,
    SpaceBackendStatus, SpaceKind, SpaceRegistration, SpaceSnapshot, VldbControllerError,
};

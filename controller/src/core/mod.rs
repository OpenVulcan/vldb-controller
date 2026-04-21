/// SQLite backend wrapper built on top of `vldb-sqlite`.
/// 基于 `vldb-sqlite` 构建的 SQLite 后端封装。
pub mod sqlite;

/// LanceDB backend wrapper built on top of `vldb-lancedb`.
/// 基于 `vldb-lancedb` 构建的 LanceDB 后端封装。
pub mod lancedb;

/// Space-aware runtime registry, lease tracking, and shutdown policy.
/// 面向空间的运行时注册表、租约跟踪与关闭策略。
pub mod runtime;

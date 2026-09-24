// Verify the exact SQLite dependency embedded by the controller before native release packaging.
// 在原生发行打包前验证控制器实际内嵌的 SQLite 依赖。
use std::io;
use vldb_sqlite::runtime::SqliteRuntime;

/// A second runtime must fail while the first owns the database, and succeed after all owners drop.
/// 首个运行时持有数据库时，第二个运行时必须失败；全部持有者释放后必须可以成功打开。
/// Uses no parameters and an isolated temporary database, returning only after ownership checks pass.
/// 不接收参数，使用隔离临时数据库，仅在所有权检查通过后结束。
#[test]
fn embedded_sqlite_rejects_competing_owner() {
    let unique = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!(
        "vldb-controller-lock-{}-{unique}",
        std::process::id()
    ));
    std::fs::create_dir(&root).unwrap();
    let database = root.join("source.db");
    let path = database.to_str().unwrap();
    let first = SqliteRuntime::new();
    let owner = first.open_database(path).unwrap();
    let second = SqliteRuntime::new();
    let error = second
        .open_database(path)
        .expect_err("the embedded SQLite dependency must reject a competing owner");
    assert_eq!(
        error.downcast_ref::<io::Error>().unwrap().kind(),
        io::ErrorKind::WouldBlock
    );

    // Runtime registries retain their own references, so both the handle and runtime must be released.
    // 运行时注册表也保留引用，因此需要同时释放数据库句柄和运行时。
    drop(owner);
    drop(first);
    let next_owner = second.open_database(path).unwrap();
    drop(next_owner);
    drop(second);
    std::fs::remove_dir_all(&root).unwrap();
}

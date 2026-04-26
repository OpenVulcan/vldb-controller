/// Compile the controller protobuf contract during the Cargo build.
/// 在 Cargo 构建期间编译控制器 protobuf 协议。
fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Resolve protocol files from the package directory so crates.io tarballs are self-contained.
    // 从包目录解析协议文件，确保 crates.io 发布包可独立构建。
    let manifest_dir = std::path::PathBuf::from(std::env::var("CARGO_MANIFEST_DIR")?);
    // Keep the protobuf root inside this crate instead of depending on workspace-relative paths.
    // 将 protobuf 根目录保留在当前 crate 内，避免依赖 workspace 相对路径。
    let proto_root = manifest_dir.join("proto");
    // Point tonic-prost-build at the vendored controller contract shipped with this crate.
    // 指向随当前 crate 一起发布的控制器协议契约文件。
    let proto_file = proto_root.join("v1").join("controller.proto");

    let protoc_path = protoc_bin_vendored::protoc_bin_path()?;
    // Keep the generated output deterministic across developer machines and CI runners.
    // 在开发机与 CI 运行器之间保持生成结果的确定性。
    unsafe {
        std::env::set_var("PROTOC", protoc_path);
    }

    tonic_prost_build::configure()
        .build_server(true)
        .build_client(true)
        .compile_protos(&[proto_file], &[proto_root])?;

    Ok(())
}

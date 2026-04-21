/// Compile the controller protobuf contract during the Cargo build.
/// 在 Cargo 构建期间编译控制器 protobuf 协议。
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let protoc_path = protoc_bin_vendored::protoc_bin_path()?;
    // Keep the generated output deterministic across developer machines and CI runners.
    // 在开发机与 CI 运行器之间保持生成结果的确定性。
    unsafe {
        std::env::set_var("PROTOC", protoc_path);
    }

    tonic_prost_build::configure()
        .build_server(true)
        .build_client(true)
        .compile_protos(&["../proto/v1/controller.proto"], &["../proto"])?;

    Ok(())
}

use std::error::Error;

use vldb_controller_client::types::{ControllerProcessMode, ControllerServerConfig};

/// Shared error type used by CLI parsing helpers.
/// CLI 解析辅助函数复用的共享错误类型。
pub type BoxError = Box<dyn Error + Send + Sync + 'static>;

/// Parse one full server configuration from process arguments.
/// 从进程参数解析一份完整服务配置。
pub fn parse_server_config_from_env() -> Result<ControllerServerConfig, BoxError> {
    parse_server_config(std::env::args().skip(1).collect())
}

/// Parse one full server configuration from the supplied raw arguments.
/// 从给定原始参数解析一份完整服务配置。
pub fn parse_server_config(raw_args: Vec<String>) -> Result<ControllerServerConfig, BoxError> {
    let mut config = ControllerServerConfig::default();
    let mut args = raw_args.into_iter();

    while let Some(argument) = args.next() {
        match argument.as_str() {
            "--bind" => {
                config.bind_addr = next_value(&mut args, "--bind")?;
            }
            "--mode" => {
                config.runtime.process_mode =
                    parse_process_mode(&next_value(&mut args, "--mode")?)?;
            }
            "--minimum-uptime-secs" => {
                config.runtime.minimum_uptime_secs = parse_u64(
                    &next_value(&mut args, "--minimum-uptime-secs")?,
                    "--minimum-uptime-secs",
                )?;
            }
            "--idle-timeout-secs" => {
                config.runtime.idle_timeout_secs = parse_u64(
                    &next_value(&mut args, "--idle-timeout-secs")?,
                    "--idle-timeout-secs",
                )?;
            }
            "--default-lease-ttl-secs" => {
                config.runtime.default_lease_ttl_secs = parse_u64(
                    &next_value(&mut args, "--default-lease-ttl-secs")?,
                    "--default-lease-ttl-secs",
                )?;
            }
            "--help" | "-h" => {
                return Err(invalid_input(help_text()));
            }
            other => {
                return Err(invalid_input(format!(
                    "unsupported argument `{other}`; use --help to inspect the supported startup parameters"
                )));
            }
        }
    }

    validate_server_config(&config)?;
    Ok(config)
}

/// Return the built-in help text for the parameter-driven startup model.
/// 返回参数驱动启动模型的内置帮助文本。
pub fn help_text() -> &'static str {
    "vldb-controller\n\
    \n\
    Supported arguments:\n\
      --bind <HOST:PORT>\n\
      --mode <service|managed>\n\
      --minimum-uptime-secs <SECONDS>\n\
      --idle-timeout-secs <SECONDS>\n\
      --default-lease-ttl-secs <SECONDS>\n"
}

/// Parse one process mode string.
/// 解析一个进程模式字符串。
fn parse_process_mode(value: &str) -> Result<ControllerProcessMode, BoxError> {
    match value.trim().to_ascii_lowercase().as_str() {
        "service" => Ok(ControllerProcessMode::Service),
        "managed" => Ok(ControllerProcessMode::Managed),
        _ => Err(invalid_input(format!(
            "unsupported process mode `{value}`; expected `service` or `managed`"
        ))),
    }
}

/// Parse one unsigned integer argument.
/// 解析一个无符号整数参数。
fn parse_u64(value: &str, flag: &str) -> Result<u64, BoxError> {
    value.parse::<u64>().map_err(|error| {
        invalid_input(format!(
            "{flag} expects an unsigned integer value, but received `{value}`: {error}"
        ))
    })
}

/// Return the next argument value for one option flag.
/// 返回某个选项标志的下一个参数值。
fn next_value(args: &mut impl Iterator<Item = String>, flag: &str) -> Result<String, BoxError> {
    args.next()
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| invalid_input(format!("{flag} requires one non-empty value")))
}

/// Validate the resolved server configuration before startup.
/// 在启动前校验解析完成的服务配置。
fn validate_server_config(config: &ControllerServerConfig) -> Result<(), BoxError> {
    if config.bind_addr.trim().is_empty() {
        return Err(invalid_input("bind address must not be empty"));
    }
    if config.runtime.default_lease_ttl_secs == 0 {
        return Err(invalid_input("default lease ttl must be greater than zero"));
    }
    Ok(())
}

/// Build one invalid-input error used by CLI parsing.
/// 构造一个供 CLI 解析使用的无效输入错误。
fn invalid_input(message: impl Into<String>) -> BoxError {
    Box::new(std::io::Error::new(
        std::io::ErrorKind::InvalidInput,
        message.into(),
    ))
}

#[cfg(test)]
mod tests {
    use super::parse_server_config;
    use vldb_controller_client::types::ControllerProcessMode;

    #[test]
    fn parser_accepts_custom_shared_or_isolated_endpoints() {
        let config = parse_server_config(vec![
            "--bind".to_string(),
            "127.0.0.1:19811".to_string(),
            "--mode".to_string(),
            "service".to_string(),
            "--minimum-uptime-secs".to_string(),
            "600".to_string(),
            "--idle-timeout-secs".to_string(),
            "1200".to_string(),
            "--default-lease-ttl-secs".to_string(),
            "180".to_string(),
        ])
        .expect("configuration should parse");

        assert_eq!(config.bind_addr, "127.0.0.1:19811");
        assert_eq!(config.runtime.process_mode, ControllerProcessMode::Service);
        assert_eq!(config.runtime.minimum_uptime_secs, 600);
        assert_eq!(config.runtime.idle_timeout_secs, 1200);
        assert_eq!(config.runtime.default_lease_ttl_secs, 180);
    }
}

//! FluxDown 官方客户端本地代理的应用边界。
//!
//! 本 crate 承载账户、云同步、设备协同与 UI Gateway；下载执行和下载任务事实属于
//! `fluxdown_daemon`。

use fluxdown_protocol::{ServiceHello, ServiceRole};

/// 官方客户端本地代理进程名。
pub const SERVICE_NAME: &str = "fluxdown-agent";

/// 返回本地代理用于协议协商的稳定身份。
#[must_use]
pub fn service_hello() -> ServiceHello {
    ServiceHello::new(ServiceRole::Agent, SERVICE_NAME, env!("CARGO_PKG_VERSION"))
}

#[cfg(test)]
mod tests {
    use fluxdown_protocol::{PROTOCOL_VERSION, ServiceRole};

    use super::{SERVICE_NAME, service_hello};

    #[test]
    fn identifies_as_cloud_agent() {
        let hello = service_hello();

        assert_eq!(hello.role, ServiceRole::Agent);
        assert_eq!(hello.service_name, SERVICE_NAME);
        assert_eq!(hello.protocol_version, PROTOCOL_VERSION);
    }
}

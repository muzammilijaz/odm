use crate::error::{EngineError, Result};

/// Global proxy configuration: either go direct, use an explicit HTTP/SOCKS5
/// proxy, or defer to whatever the OS reports (WinINet/WPAD on Windows,
/// standard `*_PROXY` env vars elsewhere).
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub enum ProxyConfig {
    #[default]
    Direct,
    System,
    Custom {
        /// e.g. "http://host:port" or "socks5://host:port"
        url: String,
        username: Option<String>,
        password: Option<String>,
    },
}

impl ProxyConfig {
    pub(crate) fn apply(&self, builder: reqwest::ClientBuilder) -> Result<reqwest::ClientBuilder> {
        match self {
            ProxyConfig::Direct => Ok(builder.no_proxy()),
            ProxyConfig::System => Ok(builder), // reqwest reads system/env proxy settings by default
            ProxyConfig::Custom {
                url,
                username,
                password,
            } => {
                let mut proxy = reqwest::Proxy::all(url)
                    .map_err(|e| EngineError::InvalidProxy(e.to_string()))?;
                if let (Some(user), Some(pass)) = (username, password) {
                    proxy = proxy.basic_auth(user, pass);
                }
                Ok(builder.proxy(proxy))
            }
        }
    }
}

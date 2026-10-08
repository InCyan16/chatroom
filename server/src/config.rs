use serde::{Deserialize, Serialize};

#[derive(Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub bind: String,
    pub origins: Vec<String>,
    pub production: bool,
    pub trust: std::net::IpAddr,
    pub base_url: String,
    pub max_users: usize,
    pub max_rooms: usize,
    pub max_messages: usize,
}
impl Default for Config {
    fn default() -> Self {
        Self {
            bind: "127.0.0.1:3000".into(),
            production: false,
            trust: std::net::IpAddr::V4(std::net::Ipv4Addr::LOCALHOST),
            base_url: "/".into(),
            max_users: 64,
            max_rooms: 64,
            max_messages: 1000,
            origins: vec![
                "http://localhost:5173".into(),
                "http://127.0.0.1:5173".into(),
                "http://localhost:3000".into(),
                "http://127.0.0.1:3000".into(),
            ],
        }
    }
}
impl Config {
    pub fn display(&self) -> Result<String, String> {
        serde_json::to_string_pretty(self).map_err(|_| "Could not display configuration.".into())
    }
    pub fn load(data: &std::path::Path) -> Result<Self, String> {
        let path = data.join("config.json");
        let saved: Option<Self> = if path.exists() {
            Some(
                serde_json::from_slice(&std::fs::read(&path).map_err(|e| e.to_string())?)
                    .map_err(|e| format!("Invalid data/config.json: {e}"))?,
            )
        } else {
            None
        };
        let production =
            !cfg!(debug_assertions) || std::env::var("CHAT_PRODUCTION").as_deref() == Ok("1");
        let production = production || saved.as_ref().is_some_and(|c| c.production);
        let origins = if production {
            let origin = std::env::var("CHAT_ORIGIN")
                .ok()
                .or_else(|| {
                    saved
                        .as_ref()
                        .filter(|c| c.production)
                        .and_then(|c| c.origins.first().cloned())
                })
                .ok_or("First production start requires CHAT_ORIGIN=https://your-host")?;
            let uri: axum::http::Uri = origin.parse().map_err(|_| "Invalid CHAT_ORIGIN")?;
            if uri.scheme_str() != Some("https")
                || uri.authority().is_none()
                || uri.path_and_query().is_some_and(|p| p.as_str() != "/")
                || origin.ends_with('/')
            {
                return Err(
                    "CHAT_ORIGIN must be an HTTPS origin without a trailing slash or path.".into(),
                );
            }
            vec![origin]
        } else {
            saved
                .as_ref()
                .map(|c| c.origins.clone())
                .unwrap_or_else(|| Self::default().origins)
        };
        let config = Self {
            production,
            origins,
            max_users: saved.as_ref().map_or(64, |c| c.max_users),
            max_rooms: saved.as_ref().map_or(64, |c| c.max_rooms),
            max_messages: saved.as_ref().map_or(1000, |c| c.max_messages),
            base_url: normalize_base_url(
                &std::env::var("CHAT_BASE_URL")
                    .ok()
                    .or_else(|| saved.as_ref().map(|c| c.base_url.clone()))
                    .unwrap_or_else(|| "/".into()),
            )?,
            trust: std::env::var("CHAT_TRUST")
                .ok()
                .map(|value| {
                    value
                        .parse()
                        .map_err(|_| "CHAT_TRUST must be a single IP address.")
                })
                .transpose()?
                .or_else(|| saved.as_ref().map(|c| c.trust))
                .unwrap_or_else(|| Self::default().trust),
            bind: std::env::var("CHAT_BIND")
                .ok()
                .or_else(|| saved.map(|c| c.bind))
                .unwrap_or_else(|| "127.0.0.1:3000".into()),
        };
        if config.max_users == 0 || config.max_rooms == 0 || config.max_messages == 0 {
            return Err("max_users, max_rooms and max_messages must be positive integers.".into());
        }
        config
            .bind
            .parse::<std::net::SocketAddr>()
            .map_err(|_| "bind must be an IP address and port, such as 127.0.0.1:3000.")?;
        let temp = data.join("config.json.tmp");
        std::fs::write(&temp, serde_json::to_vec_pretty(&config).unwrap())
            .map_err(|e| e.to_string())?;
        std::fs::rename(temp, path).map_err(|e| e.to_string())?;
        Ok(config)
    }
}
fn normalize_base_url(value: &str) -> Result<String, String> {
    if value == "/" {
        return Ok(value.into());
    }
    if !value.starts_with('/')
        || value.trim_end_matches('/').split('/').skip(1).any(|part| {
            part.is_empty()
                || !part
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
        })
        || value.ends_with("//")
    {
        return Err(
            "base_url must be a path such as / or /commonroom/ (letters, digits, - and _).".into(),
        );
    }
    Ok(format!("{}/", value.trim_end_matches('/')))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn zero_limits_are_rejected_before_saving() {
        let dir = tempfile::tempdir().unwrap();
        for field in ["max_users", "max_rooms", "max_messages"] {
            std::fs::write(dir.path().join("config.json"), format!("{{\"{field}\":0}}")).unwrap();
            assert!(Config::load(dir.path()).is_err());
        }
    }
    #[test]
    fn existing_configs_default_to_a_local_proxy() {
        let config: Config = serde_json::from_str(
            r#"{"bind":"127.0.0.1:3000","production":false,"origins":["http://localhost:3000"]}"#,
        )
        .unwrap();
        assert_eq!(config.trust.to_string(), "127.0.0.1");
        assert_eq!(config.max_messages, 1000);
        let partial: Config = serde_json::from_str(r#"{"production":false}"#).unwrap();
        assert_eq!(partial.bind, "127.0.0.1:3000");
        assert!(serde_json::from_str::<Config>(r#"{"trust":"not-an-ip"}"#).is_err());
    }
    #[test]
    fn configuration_moves_with_data_folder() {
        let source = tempfile::tempdir().unwrap();
        let destination = tempfile::tempdir().unwrap();
        let original = Config {
            bind: "127.0.0.1:3456".into(),
            origins: vec!["https://chat.example.com".into()],
            production: true,
            trust: "10.0.0.2".parse().unwrap(),
            base_url: "/commonroom/".into(),
            max_users: 8,
            max_rooms: 12,
            max_messages: 7,
        };
        std::fs::write(
            source.path().join("config.json"),
            serde_json::to_vec(&original).unwrap(),
        )
        .unwrap();
        std::fs::copy(
            source.path().join("config.json"),
            destination.path().join("config.json"),
        )
        .unwrap();
        let restored = Config::load(destination.path()).unwrap();
        assert!(restored.production);
        assert_eq!(restored.bind, original.bind);
        assert_eq!(restored.origins, original.origins);
        assert_eq!(restored.trust, original.trust);
        assert_eq!(restored.base_url, original.base_url);
        assert_eq!(restored.max_users, 8);
        assert_eq!(restored.max_rooms, 12);
        assert_eq!(restored.max_messages, 7);
    }
    #[test]
    fn base_url_is_normalized_and_safe_for_html_and_cookies() {
        assert_eq!(normalize_base_url("/").unwrap(), "/");
        assert_eq!(normalize_base_url("/commonroom").unwrap(), "/commonroom/");
        assert_eq!(normalize_base_url("/apps/chat/").unwrap(), "/apps/chat/");
        for invalid in [
            "",
            "//",
            "//evil",
            "/a//b",
            "/a/../b",
            "/a?x=1",
            "/a\"",
            "https://host/chat",
            "/a; Path=/",
        ] {
            assert!(normalize_base_url(invalid).is_err(), "{invalid}");
        }
    }
}

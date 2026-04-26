use std::env;

use anyhow::{Context, Result, bail};
use log::LevelFilter;

#[derive(Debug, Clone)]
pub struct AppConfig {
    pub mqtt_broker_url: String,
    pub mqtt_username: Option<String>,
    pub mqtt_password: Option<String>,
    pub mqtt_check_cert: bool,
}

impl AppConfig {
    pub fn from_env() -> Result<Self> {
        let mqtt_broker_url = env::var("MQTT_BROKER_URL")
            .context("MQTT_BROKER_URL must be configured before starting Deye Dummycloud")?;

        if !mqtt_broker_url.starts_with("mqtt://") && !mqtt_broker_url.starts_with("mqtts://") {
            bail!("MQTT_BROKER_URL must start with mqtt:// or mqtts://");
        }

        let mqtt_username = non_empty_env("MQTT_USERNAME");
        let mqtt_password = non_empty_env("MQTT_PASSWORD");

        if mqtt_password.is_some() && mqtt_username.is_none() {
            bail!("MQTT_USERNAME must be configured when MQTT_PASSWORD is configured");
        }

        let mqtt_check_cert = env::var("MQTT_CHECK_CERT")
            .map(|value| !value.eq_ignore_ascii_case("false"))
            .unwrap_or(true);

        Ok(Self {
            mqtt_broker_url,
            mqtt_username,
            mqtt_password,
            mqtt_check_cert,
        })
    }
}

pub fn log_level_from_env() -> Result<LevelFilter> {
    let value = env::var("LOGLEVEL").unwrap_or_else(|_| "info".to_owned());
    parse_log_level(&value)
}

fn parse_log_level(value: &str) -> Result<LevelFilter> {
    match value.to_ascii_lowercase().as_str() {
        "trace" => Ok(LevelFilter::Trace),
        "debug" => Ok(LevelFilter::Debug),
        "info" => Ok(LevelFilter::Info),
        "warn" => Ok(LevelFilter::Warn),
        "error" => Ok(LevelFilter::Error),
        _ => bail!("invalid LOGLEVEL '{value}', valid values are trace, debug, info, warn, error"),
    }
}

fn non_empty_env(name: &str) -> Option<String> {
    env::var(name).ok().filter(|value| !value.is_empty())
}

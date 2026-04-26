mod config;
mod mqtt;
mod protocol;
mod server;

use std::io::Write;

use anyhow::Result;

use crate::config::{AppConfig, log_level_from_env};
use crate::mqtt::MqttPublisher;
use crate::server::DummyCloudServer;

#[tokio::main]
async fn main() -> Result<()> {
    init_logging()?;

    let config = AppConfig::from_env()?;
    let publisher = MqttPublisher::connect(config).await?;

    DummyCloudServer::new(publisher).run().await
}

fn init_logging() -> Result<()> {
    let level = log_level_from_env()?;

    env_logger::Builder::new()
        .filter_level(level)
        .format(|buf, record| {
            writeln!(
                buf,
                "[{}] [{}] {}",
                buf.timestamp(),
                record.level(),
                record.args()
            )
        })
        .init();

    Ok(())
}

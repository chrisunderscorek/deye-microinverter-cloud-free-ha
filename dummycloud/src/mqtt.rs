use std::collections::HashMap;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use log::{debug, error, info, warn};
use rumqttc::{AsyncClient, Event, Incoming, MqttOptions, QoS, TlsConfiguration, Transport};
use serde_json::{Value, json};
use tokio::sync::Mutex;
use tokio::time::sleep;
use url::Url;

use crate::config::AppConfig;
use crate::protocol::DataPayload;

const TOPIC_PREFIX: &str = "deye-dummycloud";
const AUTOCONF_INTERVAL: Duration = Duration::from_secs(4 * 60 * 60);

pub struct MqttPublisher {
    client: AsyncClient,
    autoconf_timestamps: Mutex<HashMap<String, Instant>>,
}

impl MqttPublisher {
    pub async fn connect(config: AppConfig) -> Result<Self> {
        let broker_url = Url::parse(&config.mqtt_broker_url).with_context(|| {
            format!("failed to parse MQTT broker URL {}", config.mqtt_broker_url)
        })?;
        let scheme = broker_url.scheme();
        let default_port = match scheme {
            "mqtt" => 1883,
            "mqtts" => 8883,
            _ => anyhow::bail!("MQTT_BROKER_URL must start with mqtt:// or mqtts://"),
        };
        let host = broker_url
            .host_str()
            .context("MQTT_BROKER_URL must include a broker host")?;
        let port = broker_url.port().unwrap_or(default_port);

        let mut options = MqttOptions::new(client_id(), host, port);

        options.set_keep_alive(Duration::from_secs(30));

        if let Some(username) = config.mqtt_username {
            options.set_credentials(username, config.mqtt_password.unwrap_or_default());
        }

        if scheme == "mqtts" {
            if config.mqtt_check_cert {
                options.set_transport(Transport::tls_with_config(TlsConfiguration::Native));
            } else {
                warn!("MQTT certificate validation is disabled for this connection");
                let connector = native_tls::TlsConnector::builder()
                    .danger_accept_invalid_certs(true)
                    .danger_accept_invalid_hostnames(true)
                    .build()
                    .context("failed to build TLS connector")?;
                options.set_transport(Transport::tls_with_config(
                    TlsConfiguration::NativeConnector(connector),
                ));
            }
        }

        let (client, mut eventloop) = AsyncClient::new(options, 32);

        tokio::spawn(async move {
            loop {
                match eventloop.poll().await {
                    Ok(Event::Incoming(Incoming::ConnAck(_))) => {
                        info!("Connected to MQTT broker");
                    }
                    Ok(event) => {
                        debug!("MQTT event: {event:?}");
                    }
                    Err(err) => {
                        error!("MQTT error: {err}");
                        sleep(Duration::from_secs(5)).await;
                    }
                }
            }
        });

        Ok(Self {
            client,
            autoconf_timestamps: Mutex::new(HashMap::new()),
        })
    }

    pub async fn handle_data(
        &self,
        remote_address: &str,
        logger_serial: u32,
        data: &DataPayload,
    ) -> Result<()> {
        if data.inverter_meta.mppt_count == 0 {
            debug!("Ignoring historic or unsupported data packet from {remote_address}");
            return Ok(());
        }

        let serial = logger_serial.to_string();
        self.ensure_autoconf(remote_address, &serial, data.inverter_meta.mppt_count)
            .await?;

        let base_topic = format!("{TOPIC_PREFIX}/{serial}");
        let mppt_count = data.inverter_meta.mppt_count.min(data.pv.len() as u8);

        for i in 1..=mppt_count {
            let pv = &data.pv[(i - 1) as usize];
            self.publish(format!("{base_topic}/pv/{i}/v"), number(pv.v), false)
                .await?;
            self.publish(format!("{base_topic}/pv/{i}/i"), number(pv.i), false)
                .await?;
            self.publish(format!("{base_topic}/pv/{i}/w"), number(pv.w), false)
                .await?;
            self.publish(
                format!("{base_topic}/pv/{i}/kWh_today"),
                number(pv.kwh_today),
                true,
            )
            .await?;

            if pv.kwh_total > 0.0 {
                self.publish(
                    format!("{base_topic}/pv/{i}/kWh_total"),
                    number(pv.kwh_total),
                    true,
                )
                .await?;
            }
        }

        self.publish(
            format!("{base_topic}/grid/active_power_w"),
            data.grid.active_power_w.to_string(),
            false,
        )
        .await?;
        self.publish(
            format!("{base_topic}/grid/kWh_today"),
            number(data.grid.kwh_today),
            true,
        )
        .await?;

        if data.grid.kwh_total > 0.0 {
            self.publish(
                format!("{base_topic}/grid/kWh_total"),
                number(data.grid.kwh_total),
                true,
            )
            .await?;
        }

        self.publish(format!("{base_topic}/grid/v"), number(data.grid.v), false)
            .await?;
        self.publish(format!("{base_topic}/grid/hz"), number(data.grid.hz), false)
            .await?;
        self.publish(
            format!("{base_topic}/inverter/radiator_temperature"),
            number(data.inverter.radiator_temp_celsius),
            false,
        )
        .await?;

        let total_dc_power: f64 = data.pv.iter().map(|pv| pv.w).sum();
        let ac_power = data.grid.active_power_w as f64;
        let efficiency = if total_dc_power > 0.0 && total_dc_power > ac_power {
            format!("{:.2}", ac_power / total_dc_power * 100.0)
        } else {
            "null".to_owned()
        };

        self.publish(
            format!("{base_topic}/inverter/efficiency"),
            efficiency,
            false,
        )
        .await?;

        Ok(())
    }

    async fn ensure_autoconf(
        &self,
        remote_address: &str,
        logger_serial: &str,
        mppt_count: u8,
    ) -> Result<()> {
        let now = Instant::now();
        {
            let timestamps = self.autoconf_timestamps.lock().await;
            if let Some(last_publish) = timestamps.get(logger_serial)
                && now.duration_since(*last_publish) <= AUTOCONF_INTERVAL
            {
                return Ok(());
            }
        }

        let base_topic = format!("{TOPIC_PREFIX}/{logger_serial}");
        let device = json!({
            "manufacturer": "Deye",
            "model": "Microinverter",
            "name": format!("Deye Microinverter {logger_serial}"),
            "configuration_url": format!("http://{remote_address}/index_cn.html"),
            "identifiers": [
                format!("deye_dummycloud_{logger_serial}")
            ]
        });

        for i in 1..=mppt_count {
            self.publish_json(
                format!("homeassistant/sensor/deye_dummycloud_{logger_serial}/{logger_serial}_pv{i}_v/config"),
                sensor_payload(
                    &base_topic,
                    &format!("pv/{i}/v"),
                    &format!("PV {i} Voltage"),
                    Some("V"),
                    Some("voltage"),
                    Some("measurement"),
                    &format!("deye_dummycloud_{logger_serial}_pv_{i}_v"),
                    Some(360),
                    Some(i < 3),
                    None,
                    None,
                    &device,
                ),
                true,
            )
            .await?;
            self.publish_json(
                format!("homeassistant/sensor/deye_dummycloud_{logger_serial}/{logger_serial}_pv{i}_i/config"),
                sensor_payload(
                    &base_topic,
                    &format!("pv/{i}/i"),
                    &format!("PV {i} Current"),
                    Some("A"),
                    Some("current"),
                    Some("measurement"),
                    &format!("deye_dummycloud_{logger_serial}_pv_{i}_i"),
                    Some(360),
                    Some(i < 3),
                    None,
                    None,
                    &device,
                ),
                true,
            )
            .await?;
            self.publish_json(
                format!("homeassistant/sensor/deye_dummycloud_{logger_serial}/{logger_serial}_pv{i}_w/config"),
                sensor_payload(
                    &base_topic,
                    &format!("pv/{i}/w"),
                    &format!("PV {i} Power"),
                    Some("W"),
                    Some("power"),
                    Some("measurement"),
                    &format!("deye_dummycloud_{logger_serial}_pv_{i}_w"),
                    Some(360),
                    Some(i < 3),
                    None,
                    None,
                    &device,
                ),
                true,
            )
            .await?;
            self.publish_json(
                format!("homeassistant/sensor/deye_dummycloud_{logger_serial}/{logger_serial}_pv{i}_kWh_today/config"),
                sensor_payload(
                    &base_topic,
                    &format!("pv/{i}/kWh_today"),
                    &format!("PV {i} Energy Today"),
                    Some("kWh"),
                    Some("energy"),
                    Some("total_increasing"),
                    &format!("deye_dummycloud_{logger_serial}_pv_{i}_kWh_today"),
                    None,
                    Some(i < 3),
                    None,
                    None,
                    &device,
                ),
                true,
            )
            .await?;
            self.publish_json(
                format!("homeassistant/sensor/deye_dummycloud_{logger_serial}/{logger_serial}_pv{i}_kWh_total/config"),
                sensor_payload(
                    &base_topic,
                    &format!("pv/{i}/kWh_total"),
                    &format!("PV {i} Energy Total"),
                    Some("kWh"),
                    Some("energy"),
                    Some("total_increasing"),
                    &format!("deye_dummycloud_{logger_serial}_pv_{i}_kWh_total"),
                    None,
                    Some(i < 3),
                    None,
                    None,
                    &device,
                ),
                true,
            )
            .await?;
        }

        self.publish_json(
            format!("homeassistant/sensor/deye_dummycloud_{logger_serial}/{logger_serial}_grid_active_power_w/config"),
            sensor_payload(
                &base_topic,
                "grid/active_power_w",
                "Grid Power (Active)",
                Some("W"),
                Some("power"),
                Some("measurement"),
                &format!("deye_dummycloud_{logger_serial}_grid_active_power_w"),
                Some(360),
                None,
                None,
                None,
                &device,
            ),
            true,
        )
        .await?;
        self.publish_json(
            format!("homeassistant/sensor/deye_dummycloud_{logger_serial}/{logger_serial}_grid_kWh_today/config"),
            sensor_payload(
                &base_topic,
                "grid/kWh_today",
                "Grid Energy Today",
                Some("kWh"),
                Some("energy"),
                Some("total_increasing"),
                &format!("deye_dummycloud_{logger_serial}_grid_energy_today"),
                None,
                None,
                None,
                None,
                &device,
            ),
            true,
        )
        .await?;
        self.publish_json(
            format!("homeassistant/sensor/deye_dummycloud_{logger_serial}/{logger_serial}_grid_kWh_total/config"),
            sensor_payload(
                &base_topic,
                "grid/kWh_total",
                "Grid Energy Total",
                Some("kWh"),
                Some("energy"),
                Some("total_increasing"),
                &format!("deye_dummycloud_{logger_serial}_grid_energy_total"),
                None,
                None,
                None,
                None,
                &device,
            ),
            true,
        )
        .await?;
        self.publish_json(
            format!(
                "homeassistant/sensor/deye_dummycloud_{logger_serial}/{logger_serial}_grid_v/config"
            ),
            sensor_payload(
                &base_topic,
                "grid/v",
                "Grid Voltage",
                Some("V"),
                Some("voltage"),
                Some("measurement"),
                &format!("deye_dummycloud_{logger_serial}_grid_v"),
                Some(360),
                None,
                None,
                None,
                &device,
            ),
            true,
        )
        .await?;
        self.publish_json(
            format!("homeassistant/sensor/deye_dummycloud_{logger_serial}/{logger_serial}_grid_hz/config"),
            sensor_payload(
                &base_topic,
                "grid/hz",
                "Grid Frequency",
                Some("Hz"),
                Some("frequency"),
                Some("measurement"),
                &format!("deye_dummycloud_{logger_serial}_grid_hz"),
                Some(360),
                None,
                None,
                None,
                &device,
            ),
            true,
        )
        .await?;
        self.publish_json(
            format!(
                "homeassistant/sensor/deye_dummycloud_{logger_serial}/{logger_serial}_inverter_radiator_temperature/config"
            ),
            sensor_payload(
                &base_topic,
                "inverter/radiator_temperature",
                "Radiator Temperature",
                Some("\u{00b0}C"),
                Some("temperature"),
                Some("measurement"),
                &format!("deye_dummycloud_{logger_serial}_inverter_radiator_temperature"),
                Some(360),
                None,
                None,
                None,
                &device,
            ),
            true,
        )
        .await?;
        self.publish_json(
            format!("homeassistant/sensor/deye_dummycloud_{logger_serial}/{logger_serial}_inverter_efficiency/config"),
            sensor_payload(
                &base_topic,
                "inverter/efficiency",
                "Inverter Efficiency",
                Some("%"),
                None,
                Some("measurement"),
                &format!("deye_dummycloud_{logger_serial}_inverter_efficiency"),
                Some(360),
                None,
                Some("diagnostic"),
                Some("mdi:cog-transfer"),
                &device,
            ),
            true,
        )
        .await?;

        let mut timestamps = self.autoconf_timestamps.lock().await;
        timestamps.insert(logger_serial.to_owned(), now);

        Ok(())
    }

    async fn publish_json(&self, topic: String, payload: Value, retain: bool) -> Result<()> {
        self.publish(topic, serde_json::to_string(&payload)?, retain)
            .await
    }

    async fn publish(&self, topic: String, payload: String, retain: bool) -> Result<()> {
        self.client
            .publish(topic, QoS::AtMostOnce, retain, payload)
            .await
            .context("failed to publish MQTT message")
    }
}

#[allow(clippy::too_many_arguments)]
fn sensor_payload(
    base_topic: &str,
    topic_suffix: &str,
    name: &str,
    unit_of_measurement: Option<&str>,
    device_class: Option<&str>,
    state_class: Option<&str>,
    object_and_unique_id: &str,
    expire_after: Option<u16>,
    enabled_by_default: Option<bool>,
    entity_category: Option<&str>,
    icon: Option<&str>,
    device: &Value,
) -> Value {
    let mut payload = json!({
        "state_topic": format!("{base_topic}/{topic_suffix}"),
        "name": name,
        "object_id": object_and_unique_id,
        "unique_id": object_and_unique_id,
        "device": device,
    });

    let object = payload
        .as_object_mut()
        .expect("sensor payload is an object");

    if let Some(unit_of_measurement) = unit_of_measurement {
        object.insert("unit_of_measurement".to_owned(), json!(unit_of_measurement));
    }
    if let Some(device_class) = device_class {
        object.insert("device_class".to_owned(), json!(device_class));
    }
    if let Some(state_class) = state_class {
        object.insert("state_class".to_owned(), json!(state_class));
    }
    if let Some(expire_after) = expire_after {
        object.insert("expire_after".to_owned(), json!(expire_after));
    }
    if let Some(enabled_by_default) = enabled_by_default {
        object.insert("enabled_by_default".to_owned(), json!(enabled_by_default));
    }
    if let Some(entity_category) = entity_category {
        object.insert("entity_category".to_owned(), json!(entity_category));
    }
    if let Some(icon) = icon {
        object.insert("icon".to_owned(), json!(icon));
    }
    if topic_suffix == "inverter/efficiency" {
        object.insert("value_template".to_owned(), json!("{{ value_json }}"));
    }

    payload
}

fn number(value: f64) -> String {
    value.to_string()
}

fn client_id() -> String {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .subsec_nanos();
    format!("deye_dummycloud_{:07x}", nanos & 0x0fff_ffff)
}

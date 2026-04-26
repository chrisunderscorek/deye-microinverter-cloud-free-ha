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
use crate::protocol::{DataPayload, LoggerPayload, ReportPayload, WifiPayload};

const TOPIC_PREFIX: &str = "deye-dummycloud";
const AUTOCONF_INTERVAL: Duration = Duration::from_secs(4 * 60 * 60);

#[derive(Debug, Clone, Default)]
struct LoggerMetadata {
    ip_address: Option<String>,
    mac_address: Option<String>,
    firmware_version: Option<String>,
    hardware_version: Option<String>,
    wifi_ssid: Option<String>,
}

impl LoggerMetadata {
    fn from_handshake(remote_address: &str, logger: &LoggerPayload) -> Self {
        Self {
            ip_address: non_empty(&logger.ip).or_else(|| non_empty(remote_address)),
            mac_address: logger.mac.clone(),
            firmware_version: non_empty(&logger.fw_ver),
            hardware_version: non_empty(&logger.ver),
            wifi_ssid: non_empty(&logger.ssid),
        }
    }
}

pub struct MqttPublisher {
    client: AsyncClient,
    autoconf_timestamps: Mutex<HashMap<String, Instant>>,
    logger_metadata: Mutex<HashMap<String, LoggerMetadata>>,
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
            logger_metadata: Mutex::new(HashMap::new()),
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

    pub async fn handle_logger(
        &self,
        remote_address: &str,
        logger_serial: u32,
        logger: &LoggerPayload,
    ) -> Result<()> {
        let serial = logger_serial.to_string();
        let metadata = LoggerMetadata::from_handshake(remote_address, logger);

        {
            let mut logger_metadata = self.logger_metadata.lock().await;
            logger_metadata.insert(serial.clone(), metadata.clone());
        }

        let base_topic = format!("{TOPIC_PREFIX}/{serial}");
        self.publish(
            format!("{base_topic}/logger/serial_number"),
            serial.clone(),
            true,
        )
        .await?;

        if let Some(ip_address) = metadata.ip_address.as_deref() {
            self.publish(
                format!("{base_topic}/logger/ip_address"),
                ip_address.to_owned(),
                true,
            )
            .await?;
        }
        if let Some(mac_address) = metadata.mac_address.as_deref() {
            self.publish(
                format!("{base_topic}/logger/mac_address"),
                mac_address.to_owned(),
                true,
            )
            .await?;
        }
        if let Some(firmware_version) = metadata.firmware_version.as_deref() {
            self.publish(
                format!("{base_topic}/logger/firmware_version"),
                firmware_version.to_owned(),
                true,
            )
            .await?;
        }
        if let Some(hardware_version) = metadata.hardware_version.as_deref() {
            self.publish(
                format!("{base_topic}/logger/hardware_version"),
                hardware_version.to_owned(),
                true,
            )
            .await?;
        }
        if let Some(wifi_ssid) = metadata.wifi_ssid.as_deref() {
            self.publish(
                format!("{base_topic}/logger/wifi_ssid"),
                wifi_ssid.to_owned(),
                true,
            )
            .await?;
        }

        Ok(())
    }

    pub async fn handle_report(&self, logger_serial: u32, report: &ReportPayload) -> Result<()> {
        let base_topic = format!("{TOPIC_PREFIX}/{logger_serial}");
        let report_time = report.reconstructed_timestamp_seconds();

        self.publish(
            format!("{base_topic}/logger/uptime_seconds"),
            report.uptime_seconds.to_string(),
            true,
        )
        .await?;
        self.publish(
            format!("{base_topic}/logger/report_time"),
            unix_epoch_to_utc_iso8601(report_time),
            true,
        )
        .await?;

        if report_time >= report.uptime_seconds as u64 {
            let last_reboot = report_time - report.uptime_seconds as u64;
            self.publish(
                format!("{base_topic}/logger/last_reboot"),
                unix_epoch_to_utc_iso8601(last_reboot),
                true,
            )
            .await?;
        }

        Ok(())
    }

    pub async fn handle_wifi(
        &self,
        logger_serial: u32,
        wifi: &WifiPayload,
        expected_ssid: Option<&str>,
    ) -> Result<()> {
        let Some(expected_ssid) = expected_ssid else {
            debug!("Skipping WIFI signal publish without a known SSID");
            return Ok(());
        };

        if expected_ssid.is_empty() || wifi.text_value != expected_ssid {
            debug!(
                "Skipping WIFI signal publish for non-SSID status text with len {}",
                wifi.text_value.len()
            );
            return Ok(());
        }

        let Some(signal_quality_percent) = wifi.signal_quality_percent else {
            debug!("Skipping WIFI signal publish with out-of-range signal quality");
            return Ok(());
        };

        let base_topic = format!("{TOPIC_PREFIX}/{logger_serial}");
        self.publish(
            format!("{base_topic}/logger/wifi_signal"),
            signal_quality_percent.to_string(),
            true,
        )
        .await
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
            if let Some(last_publish) = timestamps.get(logger_serial) {
                if now.duration_since(*last_publish) <= AUTOCONF_INTERVAL {
                    return Ok(());
                }
            }
        }

        let base_topic = format!("{TOPIC_PREFIX}/{logger_serial}");
        let logger_metadata = {
            let logger_metadata = self.logger_metadata.lock().await;
            logger_metadata.get(logger_serial).cloned()
        };
        let device = device_payload(logger_serial, remote_address, logger_metadata.as_ref());

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

        self.publish_json(
            format!("homeassistant/text_sensor/deye_dummycloud_{logger_serial}/{logger_serial}_logger_serial_number/config"),
            text_sensor_payload(
                &base_topic,
                "logger/serial_number",
                "Logger Serial Number",
                &format!("deye_dummycloud_{logger_serial}_logger_serial_number"),
                Some("diagnostic"),
                Some("mdi:barcode"),
                &device,
            ),
            true,
        )
        .await?;
        self.publish_json(
            format!("homeassistant/text_sensor/deye_dummycloud_{logger_serial}/{logger_serial}_logger_ip_address/config"),
            text_sensor_payload(
                &base_topic,
                "logger/ip_address",
                "Logger IP Address",
                &format!("deye_dummycloud_{logger_serial}_logger_ip_address"),
                Some("diagnostic"),
                Some("mdi:ip-network"),
                &device,
            ),
            true,
        )
        .await?;
        self.publish_json(
            format!("homeassistant/text_sensor/deye_dummycloud_{logger_serial}/{logger_serial}_logger_mac_address/config"),
            text_sensor_payload(
                &base_topic,
                "logger/mac_address",
                "Logger MAC Address",
                &format!("deye_dummycloud_{logger_serial}_logger_mac_address"),
                Some("diagnostic"),
                Some("mdi:network-outline"),
                &device,
            ),
            true,
        )
        .await?;
        self.publish_json(
            format!("homeassistant/text_sensor/deye_dummycloud_{logger_serial}/{logger_serial}_logger_firmware_version/config"),
            text_sensor_payload(
                &base_topic,
                "logger/firmware_version",
                "Logger Firmware Version",
                &format!("deye_dummycloud_{logger_serial}_logger_firmware_version"),
                Some("diagnostic"),
                Some("mdi:chip"),
                &device,
            ),
            true,
        )
        .await?;
        self.publish_json(
            format!("homeassistant/text_sensor/deye_dummycloud_{logger_serial}/{logger_serial}_logger_hardware_version/config"),
            text_sensor_payload(
                &base_topic,
                "logger/hardware_version",
                "Logger Hardware Version",
                &format!("deye_dummycloud_{logger_serial}_logger_hardware_version"),
                Some("diagnostic"),
                Some("mdi:developer-board"),
                &device,
            ),
            true,
        )
        .await?;
        self.publish_json(
            format!("homeassistant/sensor/deye_dummycloud_{logger_serial}/{logger_serial}_logger_uptime_seconds/config"),
            sensor_payload(
                &base_topic,
                "logger/uptime_seconds",
                "Logger Uptime",
                Some("s"),
                Some("duration"),
                Some("measurement"),
                &format!("deye_dummycloud_{logger_serial}_logger_uptime_seconds"),
                None,
                None,
                Some("diagnostic"),
                Some("mdi:timer-outline"),
                &device,
            ),
            true,
        )
        .await?;
        self.publish_json(
            format!("homeassistant/sensor/deye_dummycloud_{logger_serial}/{logger_serial}_logger_report_time/config"),
            sensor_payload(
                &base_topic,
                "logger/report_time",
                "Logger Report Time",
                None,
                Some("timestamp"),
                None,
                &format!("deye_dummycloud_{logger_serial}_logger_report_time"),
                None,
                None,
                Some("diagnostic"),
                Some("mdi:clock-outline"),
                &device,
            ),
            true,
        )
        .await?;
        self.publish_json(
            format!("homeassistant/sensor/deye_dummycloud_{logger_serial}/{logger_serial}_logger_last_reboot/config"),
            sensor_payload(
                &base_topic,
                "logger/last_reboot",
                "Logger Last Reboot",
                None,
                Some("timestamp"),
                None,
                &format!("deye_dummycloud_{logger_serial}_logger_last_reboot"),
                None,
                None,
                Some("diagnostic"),
                Some("mdi:restart"),
                &device,
            ),
            true,
        )
        .await?;
        self.publish_json(
            format!("homeassistant/sensor/deye_dummycloud_{logger_serial}/{logger_serial}_logger_wifi_signal/config"),
            sensor_payload(
                &base_topic,
                "logger/wifi_signal",
                "Logger WiFi Signal",
                Some("%"),
                None,
                Some("measurement"),
                &format!("deye_dummycloud_{logger_serial}_logger_wifi_signal"),
                None,
                None,
                Some("diagnostic"),
                Some("mdi:wifi"),
                &device,
            ),
            true,
        )
        .await?;
        self.publish_json(
            format!("homeassistant/text_sensor/deye_dummycloud_{logger_serial}/{logger_serial}_logger_wifi_ssid/config"),
            text_sensor_payload(
                &base_topic,
                "logger/wifi_ssid",
                "Logger WiFi SSID",
                &format!("deye_dummycloud_{logger_serial}_logger_wifi_ssid"),
                Some("diagnostic"),
                Some("mdi:wifi-settings"),
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

fn device_payload(
    logger_serial: &str,
    remote_address: &str,
    metadata: Option<&LoggerMetadata>,
) -> Value {
    let mut payload = json!({
        "manufacturer": "Deye",
        "model": "Microinverter",
        "name": format!("Deye Microinverter {logger_serial}"),
        "configuration_url": format!("http://{remote_address}/index_cn.html"),
        "identifiers": [
            format!("deye_dummycloud_{logger_serial}")
        ],
        "serial_number": logger_serial,
    });

    let object = payload
        .as_object_mut()
        .expect("device payload is an object");

    if let Some(metadata) = metadata {
        if let Some(firmware_version) = metadata.firmware_version.as_deref() {
            object.insert("sw_version".to_owned(), json!(firmware_version));
        }
        if let Some(hardware_version) = metadata.hardware_version.as_deref() {
            object.insert("hw_version".to_owned(), json!(hardware_version));
        }
        if let Some(mac_address) = metadata.mac_address.as_deref() {
            object.insert("connections".to_owned(), json!([["mac", mac_address]]));
        }
    }

    payload
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

fn text_sensor_payload(
    base_topic: &str,
    topic_suffix: &str,
    name: &str,
    object_and_unique_id: &str,
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
        .expect("text sensor payload is an object");

    if let Some(entity_category) = entity_category {
        object.insert("entity_category".to_owned(), json!(entity_category));
    }
    if let Some(icon) = icon {
        object.insert("icon".to_owned(), json!(icon));
    }

    payload
}

fn number(value: f64) -> String {
    value.to_string()
}

fn non_empty(value: &str) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_owned())
    }
}

fn unix_epoch_to_utc_iso8601(seconds: u64) -> String {
    let days = (seconds / 86_400) as i64;
    let seconds_of_day = seconds % 86_400;
    let (year, month, day) = civil_from_unix_days(days);
    let hour = seconds_of_day / 3_600;
    let minute = (seconds_of_day % 3_600) / 60;
    let second = seconds_of_day % 60;

    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}Z")
}

fn civil_from_unix_days(days_since_unix_epoch: i64) -> (i32, u32, u32) {
    let z = days_since_unix_epoch + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let day_of_era = z - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_index = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_index + 2) / 5 + 1;
    let month = month_index + if month_index < 10 { 3 } else { -9 };
    let year = year + if month <= 2 { 1 } else { 0 };

    (year as i32, month as u32, day as u32)
}

fn client_id() -> String {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .subsec_nanos();
    format!("deye_dummycloud_{:07x}", nanos & 0x0fff_ffff)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn creates_diagnostic_text_sensor_payload() {
        let device = json!({"identifiers": ["deye_dummycloud_123"]});
        let payload = text_sensor_payload(
            "deye-dummycloud/123",
            "logger/wifi_ssid",
            "Logger WiFi SSID",
            "deye_dummycloud_123_logger_wifi_ssid",
            Some("diagnostic"),
            Some("mdi:wifi-settings"),
            &device,
        );

        assert_eq!(
            payload["state_topic"],
            "deye-dummycloud/123/logger/wifi_ssid"
        );
        assert_eq!(payload["entity_category"], "diagnostic");
        assert_eq!(payload["icon"], "mdi:wifi-settings");
        assert_eq!(payload["unique_id"], "deye_dummycloud_123_logger_wifi_ssid");
    }

    #[test]
    fn adds_logger_metadata_to_device_payload() {
        let metadata = LoggerMetadata {
            ip_address: Some("192.0.2.15".to_owned()),
            mac_address: Some("AA:BB:CC:DD:EE:FF".to_owned()),
            firmware_version: Some("MW3_16U_5406_1.53".to_owned()),
            hardware_version: Some("V1.1.00.0F".to_owned()),
            wifi_ssid: Some("test-net".to_owned()),
        };
        let payload = device_payload("1234567890", "192.0.2.15", Some(&metadata));

        assert_eq!(payload["serial_number"], "1234567890");
        assert_eq!(
            payload["configuration_url"],
            "http://192.0.2.15/index_cn.html"
        );
        assert_eq!(payload["sw_version"], "MW3_16U_5406_1.53");
        assert_eq!(payload["hw_version"], "V1.1.00.0F");
        assert_eq!(payload["connections"][0][0], "mac");
        assert_eq!(payload["connections"][0][1], "AA:BB:CC:DD:EE:FF");
    }

    #[test]
    fn formats_unix_epoch_as_utc_iso8601() {
        assert_eq!(unix_epoch_to_utc_iso8601(0), "1970-01-01T00:00:00Z");
        assert_eq!(
            unix_epoch_to_utc_iso8601(1_700_000_000),
            "2023-11-14T22:13:20Z"
        );
    }
}

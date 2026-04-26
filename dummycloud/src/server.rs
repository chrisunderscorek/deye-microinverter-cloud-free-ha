use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use log::{debug, error, info, trace, warn};
use tokio::fs::{File, create_dir_all};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

use crate::config::StreamDumpConfig;
use crate::mqtt::MqttPublisher;
use crate::protocol::{
    PORT, RequestType, build_time_response, packet_total_len, parse_data_payload,
    parse_logger_payload, parse_packet,
};

const MAX_PACKET_LEN: usize = 4096;

pub struct DummyCloudServer {
    publisher: Arc<MqttPublisher>,
    stream_dump: Option<Arc<StreamDumpConfig>>,
}

impl DummyCloudServer {
    pub fn new(publisher: MqttPublisher, stream_dump: Option<StreamDumpConfig>) -> Self {
        Self {
            publisher: Arc::new(publisher),
            stream_dump: stream_dump.map(Arc::new),
        }
    }

    pub async fn run(self) -> Result<()> {
        let listener = TcpListener::bind(("0.0.0.0", PORT))
            .await
            .with_context(|| format!("failed to bind TCP port {PORT}"))?;
        info!("Starting deye-dummycloud on port {PORT}");

        loop {
            let (socket, peer_addr) = listener.accept().await?;
            info!("New connection from {}", peer_addr.ip());

            let publisher = Arc::clone(&self.publisher);
            let stream_dump = self.stream_dump.clone();
            tokio::spawn(async move {
                if let Err(err) = handle_connection(socket, peer_addr, publisher, stream_dump).await
                {
                    error!("Error on dummycloud socket for {}: {err:#}", peer_addr.ip());
                }
            });
        }
    }
}

async fn handle_connection(
    mut socket: TcpStream,
    peer_addr: SocketAddr,
    publisher: Arc<MqttPublisher>,
    stream_dump: Option<Arc<StreamDumpConfig>>,
) -> Result<()> {
    let remote_address = peer_addr.ip().to_string();
    let mut read_buf = [0u8; 2048];
    let mut pending = Vec::new();
    let mut dumper = StreamDumper::open(stream_dump.as_deref(), peer_addr).await;

    loop {
        let bytes_read = socket.read(&mut read_buf).await?;

        if bytes_read == 0 {
            info!("Ending connection with {remote_address}");
            return Ok(());
        }

        let chunk = &read_buf[..bytes_read];
        trace!(
            "Data received from client {remote_address}: {}",
            String::from_utf8_lossy(chunk)
        );
        trace!("Data {remote_address}: {}", hex(chunk));

        if let Some(dumper) = dumper.as_mut() {
            dumper.record_client_chunk(chunk).await;
        }

        pending.extend_from_slice(chunk);
        process_pending_packets(
            &mut pending,
            &mut socket,
            &remote_address,
            &publisher,
            &mut dumper,
        )
        .await?;
    }
}

async fn process_pending_packets(
    pending: &mut Vec<u8>,
    socket: &mut TcpStream,
    remote_address: &str,
    publisher: &MqttPublisher,
    dumper: &mut Option<StreamDumper>,
) -> Result<()> {
    loop {
        let Some(total_len) = next_packet_len(pending) else {
            return Ok(());
        };

        if pending.len() < total_len {
            return Ok(());
        }

        let packet_bytes = pending.drain(..total_len).collect::<Vec<_>>();
        process_packet(&packet_bytes, socket, remote_address, publisher, dumper).await?;
    }
}

fn next_packet_len(pending: &mut Vec<u8>) -> Option<usize> {
    loop {
        let result = packet_total_len(pending)?;

        match result {
            Ok(total_len) if total_len <= MAX_PACKET_LEN => return Some(total_len),
            Ok(total_len) => {
                warn!("Discarding oversized packet with {total_len} bytes");
                pending.clear();
                return None;
            }
            Err(err) => {
                warn!("Discarding byte while resyncing packet stream: {err}");
                pending.remove(0);
            }
        }
    }
}

async fn process_packet(
    packet_bytes: &[u8],
    socket: &mut TcpStream,
    remote_address: &str,
    publisher: &MqttPublisher,
    dumper: &mut Option<StreamDumper>,
) -> Result<()> {
    let packet = match parse_packet(packet_bytes) {
        Ok(packet) => packet,
        Err(err) => {
            error!("Error while parsing packet from {remote_address}: {err:#}");
            if let Some(dumper) = dumper.as_mut() {
                dumper
                    .record_parse_error(packet_bytes, &err.to_string())
                    .await;
            }
            return Ok(());
        }
    };

    let request_type = RequestType::from_byte(packet.header.msg_type);
    if let Some(dumper) = dumper.as_mut() {
        dumper
            .record_packet(packet.header.logger_serial, request_type, packet_bytes)
            .await;
    }

    match request_type {
        RequestType::Unknown(value) => warn!("Received packet of unknown type 0x{value:x}"),
        known => debug!("Received packet of type {:?}", known.name()),
    }

    match request_type {
        RequestType::Handshake => match parse_logger_payload(&packet) {
            Ok(payload) => debug!(
                "Handshake packet data from {remote_address}: fw_ver={}, ip={}, ver={}, ssid={}",
                payload.fw_ver, payload.ip, payload.ver, payload.ssid
            ),
            Err(err) => debug!("Could not parse handshake payload from {remote_address}: {err:#}"),
        },
        RequestType::Data => match parse_data_payload(&packet) {
            Ok(Some(data)) => {
                debug!("DATA packet data from {remote_address}: {data:?}");
                if let Err(err) = publisher
                    .handle_data(remote_address, packet.header.logger_serial, &data)
                    .await
                {
                    error!("Failed to publish data packet from {remote_address}: {err:#}");
                }
            }
            Ok(None) => debug!("Discarded unsupported data packet from {remote_address}"),
            Err(err) => {
                error!("Error while parsing data packet from {remote_address}: {err:#}");
                return Ok(());
            }
        },
        RequestType::Wifi
        | RequestType::Heartbeat
        | RequestType::Report
        | RequestType::Unknown(_) => {}
    }

    let response = build_time_response(&packet);
    trace!("Response {}", hex(&response));
    if let Some(dumper) = dumper.as_mut() {
        dumper.record_response(&response).await;
    }
    socket.write_all(&response).await?;

    Ok(())
}

struct StreamDumper {
    raw: File,
    log: File,
}

impl StreamDumper {
    async fn open(config: Option<&StreamDumpConfig>, peer_addr: SocketAddr) -> Option<Self> {
        let config = config?;

        if let Err(err) = create_dir_all(&config.dir).await {
            warn!(
                "Could not create stream dump directory {:?}: {err}",
                config.dir
            );
            return None;
        }

        let prefix = dump_file_prefix(&config.dir, peer_addr);
        let raw_path = prefix.with_extension("bin");
        let log_path = prefix.with_extension("log");

        let raw = match File::create(&raw_path).await {
            Ok(file) => file,
            Err(err) => {
                warn!("Could not create raw stream dump {:?}: {err}", raw_path);
                return None;
            }
        };
        let mut log = match File::create(&log_path).await {
            Ok(file) => file,
            Err(err) => {
                warn!("Could not create stream dump log {:?}: {err}", log_path);
                return None;
            }
        };

        if let Err(err) = log
            .write_all(format!("stream dump for {peer_addr}\n").as_bytes())
            .await
        {
            warn!("Could not write stream dump header {:?}: {err}", log_path);
            return None;
        }

        info!(
            "Dumping client stream for {peer_addr} to {:?} and {:?}",
            raw_path, log_path
        );

        Some(Self { raw, log })
    }

    async fn record_client_chunk(&mut self, chunk: &[u8]) {
        if let Err(err) = self.raw.write_all(chunk).await {
            warn!("Could not write raw client stream dump: {err}");
        }

        self.write_log_line(&format!(
            "client_to_dummycloud chunk len={} hex={}",
            chunk.len(),
            hex(chunk)
        ))
        .await;
    }

    async fn record_packet(
        &mut self,
        logger_serial: u32,
        request_type: RequestType,
        packet: &[u8],
    ) {
        self.write_log_line(&format!(
            "packet type={} logger_serial={} len={} hex={}",
            request_type.name(),
            logger_serial,
            packet.len(),
            hex(packet)
        ))
        .await;
    }

    async fn record_response(&mut self, response: &[u8]) {
        self.write_log_line(&format!(
            "dummycloud_to_client response len={} hex={}",
            response.len(),
            hex(response)
        ))
        .await;
    }

    async fn record_parse_error(&mut self, packet: &[u8], error: &str) {
        self.write_log_line(&format!(
            "parse_error error={error:?} len={} hex={}",
            packet.len(),
            hex(packet)
        ))
        .await;
    }

    async fn write_log_line(&mut self, line: &str) {
        if let Err(err) = self.log.write_all(format!("{line}\n").as_bytes()).await {
            warn!("Could not write stream dump log: {err}");
        }
    }
}

fn dump_file_prefix(dir: &Path, peer_addr: SocketAddr) -> PathBuf {
    let timestamp_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    let peer = peer_addr.to_string().replace([':', '.'], "-");

    dir.join(format!("deye-dummycloud-{timestamp_ms}-{peer}"))
}

fn hex(buf: &[u8]) -> String {
    buf.iter().map(|byte| format!("{byte:02x}")).collect()
}

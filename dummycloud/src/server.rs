use std::net::SocketAddr;
use std::sync::Arc;

use anyhow::{Context, Result};
use log::{debug, error, info, trace, warn};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

use crate::mqtt::MqttPublisher;
use crate::protocol::{
    PORT, RequestType, build_time_response, packet_total_len, parse_data_payload,
    parse_logger_payload, parse_packet,
};

const MAX_PACKET_LEN: usize = 4096;

pub struct DummyCloudServer {
    publisher: Arc<MqttPublisher>,
}

impl DummyCloudServer {
    pub fn new(publisher: MqttPublisher) -> Self {
        Self {
            publisher: Arc::new(publisher),
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
            tokio::spawn(async move {
                if let Err(err) = handle_connection(socket, peer_addr, publisher).await {
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
) -> Result<()> {
    let remote_address = peer_addr.ip().to_string();
    let mut read_buf = [0u8; 2048];
    let mut pending = Vec::new();

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

        pending.extend_from_slice(chunk);
        process_pending_packets(&mut pending, &mut socket, &remote_address, &publisher).await?;
    }
}

async fn process_pending_packets(
    pending: &mut Vec<u8>,
    socket: &mut TcpStream,
    remote_address: &str,
    publisher: &MqttPublisher,
) -> Result<()> {
    loop {
        let Some(total_len) = next_packet_len(pending) else {
            return Ok(());
        };

        if pending.len() < total_len {
            return Ok(());
        }

        let packet_bytes = pending.drain(..total_len).collect::<Vec<_>>();
        process_packet(&packet_bytes, socket, remote_address, publisher).await?;
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
) -> Result<()> {
    let packet = match parse_packet(packet_bytes) {
        Ok(packet) => packet,
        Err(err) => {
            error!("Error while parsing packet from {remote_address}: {err:#}");
            return Ok(());
        }
    };

    let request_type = RequestType::from_byte(packet.header.msg_type);

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
        RequestType::Wifi | RequestType::Heartbeat | RequestType::Unknown(_) => {}
    }

    let response = build_time_response(&packet);
    trace!("Response {}", hex(&response));
    socket.write_all(&response).await?;

    Ok(())
}

fn hex(buf: &[u8]) -> String {
    buf.iter().map(|byte| format!("{byte:02x}")).collect()
}

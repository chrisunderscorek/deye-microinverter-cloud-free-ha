use anyhow::{Context, Result, bail};

pub const PORT: u16 = 10000;
const HEADER_LEN: usize = 11;
const FOOTER_LEN: usize = 2;

#[derive(Debug, Clone)]
pub struct Header {
    pub unknown1: u8,
    pub msg_type: u8,
    pub msg_id_response: u8,
    pub msg_id_request: u8,
    pub logger_serial: u32,
}

#[derive(Debug, Clone)]
pub struct Packet {
    pub header: Header,
    pub payload: Vec<u8>,
}

#[derive(Debug, Clone)]
pub struct LoggerPayload {
    pub fw_ver: String,
    pub ip: String,
    pub ver: String,
    pub ssid: String,
}

#[derive(Debug, Clone)]
pub struct DataPayload {
    pub pv: Vec<PvData>,
    pub grid: GridData,
    pub inverter: InverterData,
    pub inverter_meta: InverterMeta,
}

#[derive(Debug, Clone)]
pub struct PvData {
    pub v: f64,
    pub i: f64,
    pub w: f64,
    pub kwh_today: f64,
    pub kwh_total: f64,
}

#[derive(Debug, Clone)]
pub struct GridData {
    pub active_power_w: u32,
    pub kwh_today: f64,
    pub kwh_total: f64,
    pub v: f64,
    pub hz: f64,
}

#[derive(Debug, Clone)]
pub struct InverterData {
    pub radiator_temp_celsius: f64,
}

#[derive(Debug, Clone)]
pub struct InverterMeta {
    pub mppt_count: u8,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct ReportPayload {
    pub version: u8,
    pub timestamp_offset_seconds: u32,
    pub uptime_seconds: u32,
    pub base_timestamp_seconds: u32,
    pub unknown_status: [u8; 3],
    pub reserved: Vec<u8>,
}

impl ReportPayload {
    pub fn reconstructed_timestamp_seconds(&self) -> u64 {
        self.base_timestamp_seconds as u64 + self.timestamp_offset_seconds as u64
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct WifiPayload {
    pub flags: u8,
    pub timestamp_offset_seconds: u32,
    pub uptime_seconds: u32,
    pub base_timestamp_seconds: u32,
    pub unknown_status: [u8; 2],
    pub text_value: String,
    pub signal_quality_percent: Option<u8>,
    pub link_status: u8,
}

impl WifiPayload {
    pub fn reconstructed_timestamp_seconds(&self) -> Option<u64> {
        if self.base_timestamp_seconds == 0 {
            return None;
        }

        Some(self.base_timestamp_seconds as u64 + self.timestamp_offset_seconds as u64)
    }
}

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub enum RequestType {
    Handshake,
    Data,
    Wifi,
    Heartbeat,
    Report,
    Unknown(u8),
}

impl RequestType {
    pub fn from_byte(value: u8) -> Self {
        match value {
            0x41 => Self::Handshake,
            0x42 => Self::Data,
            0x43 => Self::Wifi,
            0x47 => Self::Heartbeat,
            0x48 => Self::Report,
            other => Self::Unknown(other),
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Handshake => "HANDSHAKE",
            Self::Data => "DATA",
            Self::Wifi => "WIFI",
            Self::Heartbeat => "HEARTBEAT",
            Self::Report => "REPORT",
            Self::Unknown(_) => "UNKNOWN",
        }
    }
}

pub fn packet_total_len(buf: &[u8]) -> Option<Result<usize>> {
    if buf.len() < HEADER_LEN {
        return None;
    }

    if buf[0] != 0xa5 {
        return Some(Err(anyhow::anyhow!("invalid header magic: {}", buf[0])));
    }

    let payload_len = u16::from_le_bytes([buf[1], buf[2]]) as usize;
    Some(Ok(HEADER_LEN + payload_len + FOOTER_LEN))
}

pub fn parse_packet(buf: &[u8]) -> Result<Packet> {
    if buf.len() < HEADER_LEN + FOOTER_LEN {
        bail!("packet too short: {} bytes", buf.len());
    }

    let header = parse_header(buf)?;
    parse_footer(buf)?;

    Ok(Packet {
        header,
        payload: buf[HEADER_LEN..buf.len() - FOOTER_LEN].to_vec(),
    })
}

pub fn parse_logger_payload(packet: &Packet) -> Result<LoggerPayload> {
    let payload = &packet.payload;
    require_len(payload, 210)?;

    Ok(LoggerPayload {
        fw_ver: ascii_until_null(&payload[19..60]),
        ip: ascii_until_null(&payload[65..82]),
        ver: ascii_until_null(&payload[89..130]),
        ssid: ascii_until_null(&payload[172..210]),
    })
}

pub fn parse_data_payload(packet: &Packet) -> Result<Option<DataPayload>> {
    require_len(&packet.payload, 2)?;

    match packet.payload[1] {
        0x08 => parse_microinverter_payload(packet).map(Some),
        _ => Ok(None),
    }
}

pub fn parse_report_payload(packet: &Packet) -> Result<ReportPayload> {
    let payload = &packet.payload;
    require_len(payload, 16)?;

    Ok(ReportPayload {
        version: payload[0],
        timestamp_offset_seconds: read_u32_le(payload, 1)?,
        uptime_seconds: read_u32_le(payload, 5)?,
        base_timestamp_seconds: read_u32_le(payload, 9)?,
        unknown_status: [payload[13], payload[14], payload[15]],
        reserved: payload[16..].to_vec(),
    })
}

pub fn parse_wifi_payload(packet: &Packet) -> Result<WifiPayload> {
    let payload = &packet.payload;
    require_len(payload, 47)?;

    let signal_quality = payload[45];
    let signal_quality_percent = if signal_quality <= 100 {
        Some(signal_quality)
    } else {
        None
    };

    Ok(WifiPayload {
        flags: payload[0],
        timestamp_offset_seconds: read_u32_le(payload, 1)?,
        uptime_seconds: read_u32_le(payload, 5)?,
        base_timestamp_seconds: read_u32_le(payload, 9)?,
        unknown_status: [payload[13], payload[14]],
        text_value: ascii_until_null(&payload[15..45]),
        signal_quality_percent,
        link_status: payload[46],
    })
}

pub fn build_time_response(packet: &Packet) -> Vec<u8> {
    let mut response = vec![0u8; 23];

    response[0] = 0xa5;
    write_u16_le(&mut response, 1, 10);
    response[3] = packet.header.unknown1;
    response[4] = packet.header.msg_type.wrapping_sub(0x30);
    response[5] = packet.header.msg_id_response.wrapping_add(1);
    response[6] = packet.header.msg_id_request;
    write_u32_le(&mut response, 7, packet.header.logger_serial);

    response[11] = packet.payload.first().copied().unwrap_or_default();
    response[12] = 0x01;

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as u32;
    write_u32_le(&mut response, 13, now);
    write_u32_le(&mut response, 17, 0);

    let checksum_index = response.len() - 2;
    let magic_index = response.len() - 1;
    response[checksum_index] = checksum(&response);
    response[magic_index] = 0x15;

    response
}

pub fn checksum(msg: &[u8]) -> u8 {
    msg[1..msg.len() - 2]
        .iter()
        .fold(0u8, |sum, byte| sum.wrapping_add(*byte))
}

fn parse_header(buf: &[u8]) -> Result<Header> {
    let payload_length = read_u16_le(buf, 1)?;
    let expected_len = HEADER_LEN + payload_length as usize + FOOTER_LEN;

    if buf[0] != 0xa5 {
        bail!("invalid header magic: {}", buf[0]);
    }

    if expected_len != buf.len() {
        bail!("payload length from header doesn't match packet length");
    }

    Ok(Header {
        unknown1: buf[3],
        msg_type: buf[4],
        msg_id_response: buf[5],
        msg_id_request: buf[6],
        logger_serial: read_u32_le(buf, 7)?,
    })
}

fn parse_footer(buf: &[u8]) -> Result<()> {
    let magic = buf[buf.len() - 1];

    if magic != 0x15 {
        bail!("invalid footer magic: {magic}");
    }

    Ok(())
}

fn parse_microinverter_payload(packet: &Packet) -> Result<DataPayload> {
    let payload = &packet.payload;
    require_len(payload, 251)?;

    if payload[0] & 0b1000_0000 != 0 {
        return Ok(DataPayload {
            pv: Vec::new(),
            grid: GridData {
                active_power_w: 0,
                kwh_today: 0.0,
                kwh_total: 0.0,
                v: 0.0,
                hz: 0.0,
            },
            inverter: InverterData {
                radiator_temp_celsius: 0.0,
            },
            inverter_meta: InverterMeta { mppt_count: 0 },
        });
    }

    let pv = vec![
        parse_pv(payload, 85, 87, 136, 145)?,
        parse_pv(payload, 89, 91, 138, 149)?,
        parse_pv(payload, 93, 95, 140, 153)?,
        parse_pv(payload, 97, 99, 142, 157)?,
    ];

    Ok(DataPayload {
        pv,
        grid: GridData {
            active_power_w: read_u32_le(payload, 59)?,
            kwh_today: read_u32_le(payload, 33)? as f64 / 100.0,
            kwh_total: read_u32_le(payload, 37)? as f64 / 10.0,
            v: read_u16_le(payload, 45)? as f64 / 10.0,
            hz: read_u16_le(payload, 57)? as f64 / 100.0,
        },
        inverter: InverterData {
            radiator_temp_celsius: read_i16_le(payload, 63)? as f64 / 100.0,
        },
        inverter_meta: InverterMeta {
            mppt_count: payload[131],
        },
    })
}

fn parse_pv(
    payload: &[u8],
    v_offset: usize,
    i_offset: usize,
    today_offset: usize,
    total_offset: usize,
) -> Result<PvData> {
    let v = read_u16_le(payload, v_offset)? as f64 / 10.0;
    let i = read_u16_le(payload, i_offset)? as f64 / 10.0;
    let w = round_two(v * i);

    Ok(PvData {
        v,
        i,
        w,
        kwh_today: read_u16_le(payload, today_offset)? as f64 / 10.0,
        kwh_total: read_u16_be(payload, total_offset)? as f64 / 10.0,
    })
}

fn round_two(value: f64) -> f64 {
    (value * 100.0).round() / 100.0
}

fn require_len(buf: &[u8], len: usize) -> Result<()> {
    if buf.len() < len {
        bail!(
            "payload too short: got {} bytes, need at least {len}",
            buf.len()
        );
    }

    Ok(())
}

fn ascii_until_null(buf: &[u8]) -> String {
    let end = buf.iter().position(|byte| *byte == 0).unwrap_or(buf.len());
    String::from_utf8_lossy(&buf[..end]).to_string()
}

fn read_u16_le(buf: &[u8], offset: usize) -> Result<u16> {
    let bytes = buf
        .get(offset..offset + 2)
        .with_context(|| format!("missing u16le at offset {offset}"))?;
    Ok(u16::from_le_bytes([bytes[0], bytes[1]]))
}

fn read_i16_le(buf: &[u8], offset: usize) -> Result<i16> {
    let bytes = buf
        .get(offset..offset + 2)
        .with_context(|| format!("missing i16le at offset {offset}"))?;
    Ok(i16::from_le_bytes([bytes[0], bytes[1]]))
}

fn read_u16_be(buf: &[u8], offset: usize) -> Result<u16> {
    let bytes = buf
        .get(offset..offset + 2)
        .with_context(|| format!("missing u16be at offset {offset}"))?;
    Ok(u16::from_be_bytes([bytes[0], bytes[1]]))
}

fn read_u32_le(buf: &[u8], offset: usize) -> Result<u32> {
    let bytes = buf
        .get(offset..offset + 4)
        .with_context(|| format!("missing u32le at offset {offset}"))?;
    Ok(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
}

fn write_u16_le(buf: &mut [u8], offset: usize, value: u16) {
    buf[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
}

fn write_u32_le(buf: &mut [u8], offset: usize, value: u32) {
    buf[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_packet_and_builds_time_response() {
        let packet = packet_with_type(0x41, &[0x08, 0x01]);
        let parsed = parse_packet(&packet).expect("packet parses");
        let response = build_time_response(&parsed);

        assert_eq!(response.len(), 23);
        assert_eq!(response[0], 0xa5);
        assert_eq!(response[4], 0x11);
        assert_eq!(response[11], 0x08);
        assert_eq!(response[12], 0x01);
        assert_eq!(response[21], checksum(&response));
        assert_eq!(response[22], 0x15);
    }

    #[test]
    fn maps_known_request_types() {
        assert_eq!(RequestType::from_byte(0x41), RequestType::Handshake);
        assert_eq!(RequestType::from_byte(0x42), RequestType::Data);
        assert_eq!(RequestType::from_byte(0x43), RequestType::Wifi);
        assert_eq!(RequestType::from_byte(0x47), RequestType::Heartbeat);
        assert_eq!(RequestType::from_byte(0x48), RequestType::Report);
        assert_eq!(RequestType::from_byte(0xff), RequestType::Unknown(0xff));
    }

    #[test]
    fn parses_logger_payload_from_handshake_frame() {
        let mut payload = vec![0u8; 210];
        write_ascii(&mut payload, 19, "MW3_16U_5406_1.53");
        write_ascii(&mut payload, 65, "192.0.2.15");
        write_ascii(&mut payload, 89, "V1.1.00.0F");
        write_ascii(&mut payload, 172, "test-ssid");

        let packet = parse_packet(&packet_with_type(0x41, &payload)).expect("packet parses");
        let parsed = parse_logger_payload(&packet).expect("logger payload parses");

        assert_eq!(parsed.fw_ver, "MW3_16U_5406_1.53");
        assert_eq!(parsed.ip, "192.0.2.15");
        assert_eq!(parsed.ver, "V1.1.00.0F");
        assert_eq!(parsed.ssid, "test-ssid");
    }

    #[test]
    fn parses_microinverter_data_payload() {
        let mut payload = vec![0u8; 251];
        payload[0] = 0x01;
        payload[1] = 0x08;

        write_u32_le(&mut payload, 33, 456);
        write_u32_le(&mut payload, 37, 789);
        write_u16_le(&mut payload, 45, 2305);
        write_u16_le(&mut payload, 57, 5001);
        write_u32_le(&mut payload, 59, 123);
        write_i16_le(&mut payload, 63, 2800);

        write_u16_le(&mut payload, 85, 312);
        write_u16_le(&mut payload, 87, 2);
        write_u16_le(&mut payload, 89, 321);
        write_u16_le(&mut payload, 91, 10);
        write_u16_le(&mut payload, 136, 16);
        write_u16_le(&mut payload, 138, 19);
        write_u16_be(&mut payload, 145, 8019);
        write_u16_be(&mut payload, 149, 8741);
        payload[131] = 2;

        let packet = parse_packet(&packet_with_type(0x42, &payload)).expect("packet parses");
        let parsed = parse_data_payload(&packet)
            .expect("data payload parses")
            .expect("microinverter payload");

        assert_eq!(parsed.inverter_meta.mppt_count, 2);
        assert_eq!(parsed.grid.active_power_w, 123);
        assert_close(parsed.grid.kwh_today, 4.56);
        assert_close(parsed.grid.kwh_total, 78.9);
        assert_close(parsed.grid.v, 230.5);
        assert_close(parsed.grid.hz, 50.01);
        assert_close(parsed.inverter.radiator_temp_celsius, 28.0);
        assert_close(parsed.pv[0].v, 31.2);
        assert_close(parsed.pv[0].i, 0.2);
        assert_close(parsed.pv[0].w, 6.24);
        assert_close(parsed.pv[0].kwh_today, 1.6);
        assert_close(parsed.pv[0].kwh_total, 801.9);
        assert_close(parsed.pv[1].v, 32.1);
        assert_close(parsed.pv[1].i, 1.0);
        assert_close(parsed.pv[1].w, 32.1);
        assert_close(parsed.pv[1].kwh_today, 1.9);
        assert_close(parsed.pv[1].kwh_total, 874.1);
    }

    #[test]
    fn parses_wifi_payload() {
        let mut payload = vec![0u8; 47];
        payload[0] = 0x81;
        write_u32_le(&mut payload, 1, 1000);
        write_u32_le(&mut payload, 5, 42);
        write_u32_le(&mut payload, 9, 1_700_000_000);
        payload[13..15].copy_from_slice(&[0x10, 0x00]);
        write_ascii(&mut payload, 15, "test-net");
        payload[45] = 70;
        payload[46] = 1;

        let packet = parse_packet(&packet_with_type(0x43, &payload)).expect("packet parses");
        let parsed = parse_wifi_payload(&packet).expect("wifi payload parses");

        assert_eq!(parsed.flags, 0x81);
        assert_eq!(parsed.timestamp_offset_seconds, 1000);
        assert_eq!(parsed.uptime_seconds, 42);
        assert_eq!(parsed.base_timestamp_seconds, 1_700_000_000);
        assert_eq!(
            parsed.reconstructed_timestamp_seconds(),
            Some(1_700_001_000)
        );
        assert_eq!(parsed.unknown_status, [0x10, 0x00]);
        assert_eq!(parsed.text_value, "test-net");
        assert_eq!(parsed.signal_quality_percent, Some(70));
        assert_eq!(parsed.link_status, 1);
    }

    #[test]
    fn ignores_out_of_range_wifi_signal_quality() {
        let mut payload = vec![0u8; 47];
        payload[45] = 255;

        let packet = parse_packet(&packet_with_type(0x43, &payload)).expect("packet parses");
        let parsed = parse_wifi_payload(&packet).expect("wifi payload parses");

        assert_eq!(parsed.signal_quality_percent, None);
    }

    #[test]
    fn parses_report_payload_and_keeps_time_response_shape() {
        let mut payload = vec![0xff; 60];
        payload[0] = 0x01;
        write_u32_le(&mut payload, 1, 1000);
        write_u32_le(&mut payload, 5, 42);
        write_u32_le(&mut payload, 9, 1_700_000_000);
        payload[13..16].copy_from_slice(&[0x01, 0x05, 0x2c]);

        let packet = parse_packet(&packet_with_type(0x48, &payload)).expect("packet parses");
        let parsed = parse_report_payload(&packet).expect("report payload parses");
        let response = build_time_response(&packet);

        assert_eq!(
            RequestType::from_byte(packet.header.msg_type),
            RequestType::Report
        );
        assert_eq!(parsed.version, 1);
        assert_eq!(parsed.timestamp_offset_seconds, 1000);
        assert_eq!(parsed.uptime_seconds, 42);
        assert_eq!(parsed.base_timestamp_seconds, 1_700_000_000);
        assert_eq!(parsed.reconstructed_timestamp_seconds(), 1_700_001_000);
        assert_eq!(parsed.unknown_status, [0x01, 0x05, 0x2c]);
        assert!(parsed.reserved.iter().all(|byte| *byte == 0xff));
        assert_eq!(response[4], 0x18);
        assert_eq!(response[11], 0x01);
        assert_eq!(response[21], checksum(&response));
    }

    #[test]
    fn rejects_wrong_footer_magic() {
        let mut packet = packet_with_type(0x41, &[0x08, 0x01]);
        let last = packet.len() - 1;
        packet[last] = 0xff;

        assert!(parse_packet(&packet).is_err());
    }

    fn packet_with_type(msg_type: u8, payload: &[u8]) -> Vec<u8> {
        let mut packet = vec![0u8; HEADER_LEN + payload.len() + FOOTER_LEN];
        packet[0] = 0xa5;
        write_u16_le(&mut packet, 1, payload.len() as u16);
        packet[3] = 0x00;
        packet[4] = msg_type;
        packet[5] = 0x02;
        packet[6] = 0x03;
        write_u32_le(&mut packet, 7, 1234567890);
        packet[HEADER_LEN..HEADER_LEN + payload.len()].copy_from_slice(payload);
        let checksum_index = packet.len() - 2;
        packet[checksum_index] = checksum(&packet);
        let magic_index = packet.len() - 1;
        packet[magic_index] = 0x15;
        packet
    }

    fn write_ascii(buf: &mut [u8], offset: usize, value: &str) {
        let bytes = value.as_bytes();
        buf[offset..offset + bytes.len()].copy_from_slice(bytes);
    }

    fn write_i16_le(buf: &mut [u8], offset: usize, value: i16) {
        buf[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
    }

    fn write_u16_be(buf: &mut [u8], offset: usize, value: u16) {
        buf[offset..offset + 2].copy_from_slice(&value.to_be_bytes());
    }

    fn assert_close(actual: f64, expected: f64) {
        assert!(
            (actual - expected).abs() < 0.001,
            "expected {expected}, got {actual}"
        );
    }
}

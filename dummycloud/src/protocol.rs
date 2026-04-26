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

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub enum RequestType {
    Handshake,
    Data,
    Wifi,
    Heartbeat,
    Unknown(u8),
}

impl RequestType {
    pub fn from_byte(value: u8) -> Self {
        match value {
            0x41 => Self::Handshake,
            0x42 => Self::Data,
            0x43 => Self::Wifi,
            0x47 => Self::Heartbeat,
            other => Self::Unknown(other),
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Handshake => "HANDSHAKE",
            Self::Data => "DATA",
            Self::Wifi => "WIFI",
            Self::Heartbeat => "HEARTBEAT",
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
        let packet = packet_with_payload(&[0x08, 0x01]);
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
    fn rejects_wrong_footer_magic() {
        let mut packet = packet_with_payload(&[0x08, 0x01]);
        let last = packet.len() - 1;
        packet[last] = 0xff;

        assert!(parse_packet(&packet).is_err());
    }

    fn packet_with_payload(payload: &[u8]) -> Vec<u8> {
        let mut packet = vec![0u8; HEADER_LEN + payload.len() + FOOTER_LEN];
        packet[0] = 0xa5;
        write_u16_le(&mut packet, 1, payload.len() as u16);
        packet[3] = 0x00;
        packet[4] = 0x41;
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
}

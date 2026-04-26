# Deye Dummycloud v2

[![Builder](https://github.com/chrisunderscorek/deye-microinverter-cloud-free-ha/actions/workflows/builder.yaml/badge.svg?branch=master)](https://github.com/chrisunderscorek/deye-microinverter-cloud-free-ha/actions/workflows/builder.yaml)
[![GHCR image](https://img.shields.io/badge/GHCR-deye--dummycloud2--ha-2496ED?logo=docker&logoColor=white)](https://github.com/chrisunderscorek/deye-microinverter-cloud-free-ha/pkgs/container/deye-dummycloud2-ha)
[![Platforms](https://img.shields.io/badge/platform-linux%2Famd64%20%7C%20linux%2Farm64-2496ED?logo=linux&logoColor=white)](https://github.com/chrisunderscorek/deye-microinverter-cloud-free-ha/pkgs/container/deye-dummycloud2-ha)

Deye Dummycloud v2 is a Rust reimplementation of the local Deye Solarman cloud replacement; it has a lighter runtime, lower memory overhead, and no Node.js dependency.

It accepts Deye logger TCP connections on port `10000`, answers them with Solarman time responses, publishes inverter data to MQTT, and creates Home Assistant MQTT discovery entities.

## Home Assistant OS app

This folder is a Home Assistant OS app package. It is not a Home Assistant integration or a patched Home Assistant
component; it runs the Rust dummycloud service as an HAOS-managed app and exposes port `10000` for the inverter.
Prebuilt GHCR images are published for `aarch64`/`arm64` and `amd64`.

The original Node.js implementation remains available in [dummycloud](../dummycloud). Both apps use TCP port `10000`,
so only one should run at a time.

## Usage

The dummycloud is configured using environment variables to be container-friendly to use.

- `LOGLEVEL` (defaults to `info`)
- `MQTT_BROKER_URL` (defaults to `mqtt://core-mosquitto:1883` for Home Assistant OS. Should look like `mqtt://foo.bar`)
- `MQTT_USERNAME` (no default, optional.)
- `MQTT_PASSWORD` (no default, optional.)
- `MQTT_CHECK_CERT` set to `false` for using `mqtts` with self signed certificate (defaults to `true`)
- `DUMP_CLIENT_STREAM` (optional) set to `true` or a directory path to dump raw inverter TCP streams for protocol analysis.

## Local development

The Rust service can also be built and run directly on macOS for local testing; the Home Assistant app images target Linux `amd64` and `arm64`.

Published GHCR image sizes after the v2.0.2 build:

| Image | `linux/amd64` | `linux/arm64` |
| --- | ---: | ---: |
| `ghcr.io/chrisunderscorek/deye-dummycloud-ha:1.1.5` | 38.4 MiB | 37.6 MiB |
| `ghcr.io/chrisunderscorek/deye-dummycloud2-ha:2.0.2` | 21.8 MiB | 22.3 MiB |

The GHCR values are the compressed config and layer sizes reported by the OCI manifests. Local macOS/Colima builds of
v2.0.2 reported 22.2 MiB for `linux/amd64` via `docker image inspect`.

## MQTT diagnostics

Besides PV, grid, and inverter telemetry, Deye Dummycloud v2 publishes Home Assistant diagnostic entities for logger status:

- `logger/wifi_ssid`
- `logger/wifi_signal`
- `logger/serial_number`
- `logger/ip_address`
- `logger/mac_address`
- `logger/firmware_version`
- `logger/hardware_version`
- `logger/uptime_seconds`
- `logger/report_time`
- `logger/last_reboot`

The Wi-Fi signal value is derived from observed `0x43` WIFI packets, matches the signal quality from the Deye web UI,
and is published only when the status text matches the SSID from the logger handshake. The last reboot time is derived
from the observed `0x48` REPORT packet timestamp minus the report uptime counter.

The Home Assistant MQTT device payload includes the logger serial number, firmware/hardware versions, and the MAC address
advertised in the handshake when available. The device name remains `Deye Microinverter <logger serial>`.

## Home Assistant OS Deployment

For Home Assistant OS, install `Deye Dummycloud v2` as a Home Assistant app from this repository.

1. Open Home Assistant and go to `Settings` -> `Apps`.
2. Open `Install app`.
3. Open the repository menu and choose `Repositories` or `Add repository`.
4. Add this repository URL:

```text
https://github.com/chrisunderscorek/deye-microinverter-cloud-free-ha
```

5. Install the `Deye Dummycloud v2` app.
6. Configure the MQTT broker URL and optional credentials.
7. Start the app and keep TCP port `10000` exposed.
8. Using the `/config_hide.html` of the inverter web interface, point `Server A Setting` and `Optional Server Setting` to the HAOS host IP address and port `10000`.

For the local Mosquitto app running on the same HAOS host, use this MQTT broker URL:

```text
mqtt://core-mosquitto:1883
```

Keep the firewall rules preventing the inverter from phoning home in place for good measure.

The app uses TCP port `10000` for inverter connections and opens outbound TCP connections to the configured MQTT broker.
When `mqtts://` is used, certificate validation happens inside that same MQTT/TLS connection; no additional certificate
check port is required.

# Deye Dummycloud v2 Home Assistant App

This Home Assistant app runs the Rust dummycloud service on HAOS and exposes TCP port `10000` so Deye microinverters can connect to it as their cloud endpoint.

## Configuration

- `LOGLEVEL`: defaults to `info`; valid values are `trace`, `debug`, `info`, `warn`, and `error`.
- `MQTT_BROKER_URL`: required MQTT broker URL, for example `mqtt://core-mosquitto` or `mqtt://mqtt.example.local`.
- `MQTT_USERNAME`: optional MQTT username.
- `MQTT_PASSWORD`: optional MQTT password. Set `MQTT_USERNAME` as well when using this.
- `MQTT_CHECK_CERT`: defaults to `true`; set to `false` when using `mqtts` with a self-signed certificate.

## HAOS Setup

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

For the local Mosquitto app running on the same HAOS host, use this MQTT broker URL:

```text
mqtt://core-mosquitto:1883
```

Prebuilt app images are published for `aarch64`/`arm64` and `amd64`.

## Local development

The Rust service can also be built and run directly on macOS for local testing. The published Home Assistant app images
target Linux `amd64` and `arm64`.

Published GHCR image sizes after the v2.0.1 build:

| Image | `linux/amd64` | `linux/arm64` |
| --- | ---: | ---: |
| `ghcr.io/chrisunderscorek/deye-dummycloud-ha:1.1.5` | 38.4 MiB | 37.6 MiB |
| `ghcr.io/chrisunderscorek/deye-dummycloud2-ha:2.0.1` | 21.4 MiB | 21.8 MiB |

The GHCR values are the compressed config and layer sizes reported by the OCI manifests. Local macOS/Colima builds of
v2.0.1 reported 21.8 MiB for `linux/amd64` and 22.2 MiB for `linux/arm64` via `docker image inspect`.

## Networking

The app exposes TCP port `10000` for inverter connections and opens outbound TCP connections to the configured MQTT
broker. When `mqtts://` is used, certificate validation happens inside that same MQTT/TLS connection; no additional
certificate check port is required.

## Inverter Setup

Using the `/config_hide.html` of the inverter webinterface, point `Server A Setting` and `Optional Server Setting` to the HAOS host this app is running on, using port `10000`.
Keep the firewall rules preventing the inverter from phoning home in place for good measure.

## Logger diagnostics

Deye Dummycloud v2 publishes Home Assistant MQTT diagnostics for logger Wi-Fi SSID, signal quality, serial number, IP
address, MAC address, firmware version, hardware version, uptime, report time, and derived last reboot time.

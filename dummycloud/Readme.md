# Deye-dummycloud

This is a small service that mocks the deye solarman cloud and publishes all the data to an MQTT broker.
It also takes care of Home Assistant autodiscovery leading to things just working.

![dummycloud_demo.png](../img/dummycloud_demo.png)

## Home Assistant OS app

This folder is also a Home Assistant OS app package for dummycloud. It is not a Home Assistant integration or a patched
Home Assistant component; it runs the dummycloud service as an HAOS-managed app and exposes port `10000` for the inverter.
Prebuilt GHCR images are published for `aarch64`/`arm64` and `amd64`.

For installation steps, see [Home Assistant OS Deployment](#home-assistant-os-deployment).

## Usage

The dummycloud is configured using environment variables to be container-friendly to use.

- `LOGLEVEL` (defaults to `info`)
- `MQTT_BROKER_URL` (no default. Should look like `mqtt://foo.bar`)
- `MQTT_USERNAME` (no default, optional.)
- `MQTT_PASSWORD` (no default, optional.)
- `MQTT_CHECK_CERT` set to `false` for using `mqtts` with self signed certificate (defaults to `true`)
- `DUMP_CLIENT_STREAM` (optional) set to `true` or a directory path to dump raw inverter TCP streams for protocol analysis.

## Inverter Setup

Using the `/config_hide.html` of the inverter webinterface, simply point `Server A Setting` and `Optional Server Setting` to the host this is running on.
I'd still keep the firewall rules preventing the inverter from phoning home in place for good measure.

## MQTT diagnostics

Besides PV, grid, and inverter telemetry, dummycloud publishes Home Assistant diagnostic entities for logger status:

- `logger/wifi_ssid`
- `logger/wifi_signal`
- `logger/uptime_seconds`
- `logger/report_time`
- `logger/last_reboot`

The Wi-Fi signal value is derived from observed `0x43` WIFI packets, matches the signal quality from the Deye web UI,
and is published only when the status text matches the SSID from the logger handshake. The last reboot time is derived
from the observed `0x48` REPORT packet timestamp minus the report uptime counter.

## Home Assistant OS Deployment

For Home Assistant OS, install dummycloud as a Home Assistant app from this repository. This is the preferred deployment
path for HAOS because it uses the Home Assistant app UI for configuration and publishes prebuilt GHCR images for
`aarch64`/`arm64` and `amd64`.

1. Open Home Assistant and go to `Settings` -> `Apps`.
2. Open `Install app`.
3. Open the repository menu and choose `Repositories` or `Add repository`.
4. Add this repository URL:

```text
https://github.com/chrisunderscorek/deye-microinverter-cloud-free-ha
```

5. Install the `Deye Dummycloud` app.
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

## Standalone Docker Deployment

Outside Home Assistant OS, dummycloud can still be run directly with Node.js or Docker. The example below is the classic
standalone Docker Compose setup from the upstream project. For the canonical upstream documentation, see
[Hypfer/deye-microinverter-cloud-free](https://github.com/Hypfer/deye-microinverter-cloud-free/tree/master/dummycloud).

A standalone `docker-compose.yml` entry could for example look like this:

```yml
  deye-dummycloud:
    build:
      context: ./deye-microinverter-cloud-free/dummycloud/
      dockerfile: Dockerfile
    container_name: "deye-dummycloud"
    restart: always
    environment:
      - "LOGLEVEL=info"
      - "MQTT_BROKER_URL=mqtt://foobar.example"
      # User those variables if the MQTT broker requires username and password
      # - "MQTT_USERNAME=example-user"
      # - "MQTT_PASSWORD=example-password"
    ports:
      - "10000:10000"
```

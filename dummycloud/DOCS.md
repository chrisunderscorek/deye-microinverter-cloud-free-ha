# Deye Dummycloud Home Assistant App

This Home Assistant app runs the dummycloud service on HAOS and exposes TCP port `10000` so Deye microinverters can connect to it as their cloud endpoint.

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

5. Install the `Deye Dummycloud` app.
6. Configure the MQTT broker URL and optional credentials.
7. Start the app and keep TCP port `10000` exposed.

For the local Mosquitto app running on the same HAOS host, use this MQTT broker URL:

```text
mqtt://core-mosquitto:1883
```

Prebuilt app images are published for `aarch64`/`arm64` and `amd64`.

## Networking

The app exposes TCP port `10000` for inverter connections and opens outbound TCP connections to the configured MQTT
broker. When `mqtts://` is used, certificate validation happens inside that same MQTT/TLS connection; no additional
certificate check port is required.

## Inverter Setup

Using the `/config_hide.html` of the inverter webinterface, simply point `Server A Setting` and `Optional Server Setting` to the HAOS host this app is running on, using port `10000`.
I'd still keep the firewall rules preventing the inverter from phoning home in place for good measure.

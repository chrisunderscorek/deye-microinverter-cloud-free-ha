# Deye Dummycloud Home Assistant App

This Home Assistant app runs the dummycloud service on HAOS and exposes TCP port `10000` so Deye microinverters can connect to it as their cloud endpoint.

## Configuration

- `LOGLEVEL`: defaults to `info`; valid values are `trace`, `debug`, `info`, `warn`, and `error`.
- `MQTT_BROKER_URL`: required MQTT broker URL, for example `mqtt://core-mosquitto` or `mqtt://192.168.178.100`.
- `MQTT_USERNAME`: optional MQTT username.
- `MQTT_PASSWORD`: optional MQTT password. Set `MQTT_USERNAME` as well when using this.
- `MQTT_CHECK_CERT`: defaults to `true`; set to `false` when using `mqtts` with a self-signed certificate.

## HAOS setup

1. Add this repository as a Home Assistant app repository.
2. Install the `Deye Dummycloud` app.
3. Configure the MQTT broker URL and optional credentials.
4. Start the app and keep TCP port `10000` exposed.
5. Point the inverter cloud server setting to the HAOS host IP address.

For the HAOS host at `192.168.178.100`, configure the inverter cloud server host as `192.168.178.100` and port `10000`.

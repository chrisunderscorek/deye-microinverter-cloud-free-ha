# Deye Dummycloud Home Assistant App

This Home Assistant app runs the dummycloud service on HAOS and exposes TCP port `10000` so Deye microinverters can connect to it as their cloud endpoint.

## Configuration

- `LOGLEVEL`: defaults to `info`; valid values are `trace`, `debug`, `info`, `warn`, and `error`.
- `MQTT_BROKER_URL`: required MQTT broker URL, for example `mqtt://core-mosquitto` or `mqtt://192.168.178.100`.
- `MQTT_USERNAME`: optional MQTT username.
- `MQTT_PASSWORD`: optional MQTT password. Set `MQTT_USERNAME` as well when using this.
- `MQTT_CHECK_CERT`: defaults to `true`; set to `false` when using `mqtts` with a self-signed certificate.

## HAOS setup

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
8. Point the inverter cloud server setting to the HAOS host IP address.

For the local Mosquitto app running on the same HAOS host, use this MQTT broker URL:

```text
mqtt://core-mosquitto:1883
```

For the HAOS host at `192.168.178.100`, configure the inverter cloud server host as `192.168.178.100` and port `10000`.

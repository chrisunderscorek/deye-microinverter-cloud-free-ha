# Deye-dummycloud

This is a small Node.js service that mocks the deye solarman cloud and publishes all the data to an MQTT broker.
It also takes care of Home Assistant autodiscovery leading to things just working.

![dummycloud_demo.png](../img/dummycloud_demo.png)

## Home Assistant OS app

This folder is also a Home Assistant OS app package for dummycloud. It is not a Home Assistant integration or a patched
Home Assistant component; it runs the dummycloud service as an HAOS-managed app and exposes port `10000` for the inverter.

To install it on HAOS:

1. Open Home Assistant and go to `Settings` -> `Apps`.
2. Open `Install app`.
3. Open the repository menu and choose `Repositories` or `Add repository`.
4. Add this repository URL:

```text
https://github.com/chrisunderscorek/deye-microinverter-cloud-free-ha
```

5. Install the `Deye Dummycloud` app.
6. Configure `MQTT_BROKER_URL`. For the local Mosquitto app on the same HAOS host, use:

```text
mqtt://core-mosquitto:1883
```

Set optional `MQTT_USERNAME` and `MQTT_PASSWORD` only when your MQTT broker requires authentication. After starting the
app, point the inverter cloud server host to the HAOS IP address and port `10000`.

## Usage

The dummycloud is configured using environment variables to be container-friendly to use.

- `LOGLEVEL` (defaults to `info`)
- `MQTT_BROKER_URL` (no default. Should look like `mqtt://foo.bar`)
- `MQTT_USERNAME` (no default, optional.)
- `MQTT_PASSWORD` (no default, optional.)
- `MQTT_CHECK_CERT` set to `false` for using `mqtts` with self signed certificate (defaults to `true`)

## Inverter Setup

Using the `/config_hide.html` of the inverter webinterface, simply point `Server A Setting` and `Optional Server Setting` to the host this is running on.
I'd still keep the firewall rules preventing the inverter from phoning home in place for good measure.

## Deployment

The dummycloud can be started using `npm run start`. Next to this readme, there's also a dockerfile provided.
For more HAOS app details, see [DOCS.md](./DOCS.md).

A `docker-compose.yml` entry could for example look like this:

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

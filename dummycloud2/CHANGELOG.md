# Changelog

## 2.0.0

- Initial Home Assistant OS app package for Deye Dummycloud v2.
- Reimplement the dummycloud service in Rust while keeping the existing MQTT configuration names.
- Publish Home Assistant MQTT discovery for PV, grid, inverter, and logger diagnostic entities.
- Add logger diagnostics for Wi-Fi SSID, signal quality, serial number, IP address, MAC address, firmware version, uptime, report time, and derived last reboot time.
- Publish multi-architecture images for `aarch64` and `amd64` to GHCR.
- Install the Alpine build tooling required for native TLS dependencies in the Docker builder image.
- Pin the Rust Docker builder image and keep the crate on Rust 2021 edition for wider toolchain compatibility.
- Install static OpenSSL libraries needed for Rust's musl target in the Docker builder image.

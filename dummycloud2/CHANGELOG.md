# Changelog

## 2.0.4

- Let Home Assistant OS load the bundled AppArmor profile from `apparmor.txt`, matching the legacy app package.

## 2.0.3

- Use the boolean AppArmor metadata value expected by the Home Assistant OS 2026.4 Supervisor validator.
- Restore the v2 readme filename used by the existing Home Assistant OS app package.

## 2.0.2

- Include Home Assistant app metadata files in the runtime image for easier inspection and fallback tooling.
- Store the runtime image logo as a symlink to the icon file.
- Rename the v2 readme file to `README.md` to match Home Assistant app repository conventions.

## 2.0.1

- Enable the custom AppArmor profile explicitly in the Home Assistant app metadata.

## 2.0.0

- Initial Home Assistant OS app package for Deye Dummycloud v2.
- Reimplement the dummycloud service in Rust while keeping the existing MQTT configuration names.
- Publish Home Assistant MQTT discovery for PV, grid, inverter, and logger diagnostic entities.
- Add logger diagnostics for Wi-Fi SSID, signal quality, serial number, IP address, MAC address, firmware version, uptime, report time, and derived last reboot time.
- Publish multi-architecture images for `aarch64` and `amd64` to GHCR.
- Install the Alpine build tooling required for native TLS dependencies in the Docker builder image.
- Pin the Rust Docker builder image and keep the crate on Rust 2021 edition for wider toolchain compatibility.
- Install static OpenSSL libraries needed for Rust's musl target in the Docker builder image.

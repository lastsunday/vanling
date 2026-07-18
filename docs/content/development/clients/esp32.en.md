+++
title = "ESP32"
weight = 301
source_file_hash = "8d4afdbafb3a51b785c2e35656698c5af9bb8dbe"
translated_at = "2026-07-18"
+++

# ESP32

## Clone Code

```shell
git clone git@github.com:78/xiaozhi-esp32.git
```

## Install ESP-IDF

> <https://docs.espressif.com/projects/esp-idf/en/v6.0.2/esp32/get-started/macos-setup.html>

## Development

### Configure Environment and Flash Device

- esp32-s3

```shell
source ~/.espressif/tools/activate_idf_v6.0.2.sh
idf.py set-target esp32-s3
idf.py menuconfig
idf.py build
idf.py -p PORT flash
# macos
idf.py -p /dev/cu.usbserial-14410 flash
# linux
sudo chmod 777 /dev/ttyACM0
idf.py -p /dev/ttyACM0 flash
```

### Other Common Commands

- Get Port

```shell
ls /dev/cu.*
```

- Debug Monitor

```shell
idf.py monitor
idf.py -p PORT flash monitor

```

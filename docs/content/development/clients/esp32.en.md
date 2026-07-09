+++
title = "ESP32"
weight = 301
[extra]
source_file_hash = "ec0ae0c0c6bfbba58c9ab563acb816bd0daf4504"
translated_at = "2026-06-28T18:00:00Z"
+++

# ESP32

## Clone Code

```shell
git clone git@github.com:78/xiaozhi-esp32.git
```

## Install ESP-IDF

> <https://docs.espressif.com/projects/esp-idf/en/v5.5.2/esp32/get-started/linux-macos-setup.html>

## Development

### Configure Environment & Flash Device

- esp32-s3

```shell
. $HOME/esp/esp-idf/export.sh
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

- List ports

```shell
ls /dev/cu.*
```

- Debug monitor

```shell
idf.py monitor
idf.py -p PORT flash monitor

```

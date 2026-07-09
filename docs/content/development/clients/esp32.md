+++
title = "ESP32"
weight = 301
+++

# ESP32

## 克隆代码

```shell
git clone git@github.com:78/xiaozhi-esp32.git
```

## 安装 ESP-IDF

> <https://docs.espressif.com/projects/esp-idf/zh_CN/v5.5.2/esp32/get-started/linux-macos-setup.html>

## 开发

### 配置环境与烧录设备

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

### 其他常用命令

- 获取端口

```shell
ls /dev/cu.*
```

- 调试监视器

```shell
idf.py monitor
idf.py -p PORT flash monitor

```

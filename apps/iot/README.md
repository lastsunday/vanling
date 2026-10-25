# apps/iot

Vanling 的 ESP32 固件：S3 是产品靶，C6 是冒烟靶。

```sh
moon run iot:check-s3   # 改完先查（只 type-check，不链接）
moon run iot:flash-s3   # 编 + 烧产品靶；两块板都插着时带 IOT_PORT=/dev/cu.usbmodem…
```

命令、端口与复位、编译期变量、症状速查：[开发文档 / 速查](../../docs/content/roadmap/cheatsheet.md)。
分层与组合见 [architecture](../../docs/content/development/iot/architecture.md)，安装与产物见 [flashing](../../docs/content/development/iot/flashing.md)。

+++
title = "Quick Start"
weight = 100
[extra]
source_file_hash = "ca69f5aa9d66cfe5be4a970c797969c20cf82621"
translated_at = "2026-07-30T00:00:00Z"
+++

# Quick Start

## Development

#### Server

```shell
moon run server-ui:build
moon run server:downloader-install
moon run server:run

# with CUDA, in place of the last line (moon cannot forward --features):
cd apps/server && cargo run -p vanling-server --features cuda -- \
  -O 'address=0.0.0.0' -O 'data_dir=../../data' --config=application.toml
```

- Homepage <http://127.0.0.1:3000>
- Admin panel <http://127.0.0.1:3000/login>
  - Default credentials: root/Change_Me
- API docs <http://127.0.0.1:3000/docs>
- Device test page <http://127.0.0.1:3000/test/device/test_page.html>
- Client configuration
  - OTA address
    <http://127.0.0.1:3000/api/ota/>
  - WebSocket address
    <ws://127.0.0.1:3000/vanling/v1/>

#### Admin Panel

```shell
pnpm i
moon run server-ui:dev
```

#### App (Flutter)

**_TODO_**

## Build

**_TODO_**

## Usage

**_TODO_**

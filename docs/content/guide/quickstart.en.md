+++
title = "Quick Start"
weight = 100
[extra]
source_file_hash = "bfc4b9739d583df4b7d613e57a790f743d096394"
translated_at = "2026-07-30T00:00:00Z"
+++

# Quick Start

## Development

#### Server

```shell
pnpm i
moon run server-ui:build
vanling-server download
# using cuda: moon run server:run --features cuda
moon run server:run
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

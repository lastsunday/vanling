+++
title = "快速开始"
weight = 100
+++

# 快速开始

## 开发

#### 服务端

```shell
pnpm i
moon run server-ui:build
chobits-server download
# using cuda: moon run server:run --features cuda
moon run server:run
```

- 首页 <http://127.0.0.1:3000>
- 管理后台 <http://127.0.0.1:3000/login>
  - 默认账号：root/Change_Me
- API 文档 <http://127.0.0.1:3000/docs>
- 设备测试页 <http://127.0.0.1:3000/test/device/test_page.html>
- 客户端配置
  - OTA 地址
    <http://127.0.0.1:3000/api/ota/>
  - WebSocket 地址
    <ws://127.0.0.1:3000/chobits/v1/>

#### 管理后台

```shell
pnpm i
moon run server-ui:dev
```

#### App（Flutter）

**_TODO_**

## 构建

**_TODO_**

## 使用

**_TODO_**

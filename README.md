# Chobits

> [!WARNING]
> This project is being developed,all the things is not stable.

[![build-server](https://github.com/lastsunday/chobits/actions/workflows/build-server.yml/badge.svg)](https://github.com/lastsunday/chobits/actions/workflows/build-server.yml)[![docker](https://img.shields.io/github/v/release/lastsunday/chobits?logo=docker)](https://github.com/lastsunday/chobits/releases)

## Purpose

To learn the rust programming language,voice interaction and large language model.

To make an self contained chatbot(self host all component,eg: llm,tts etc..), like [xiaozhi-esp32](https://github.com/78/xiaozhi-esp32) with self host server.

## Features

- [x] Connection: websocket
- [x] Voice interaction: VAD,ASR,TTS
- [x] Chat: LLM
- [x] MCP: self host/remote server mcp,device mcp
- [ ] Backend
  1. home page(WIP)
  1. admin console(WIP)
  1. simulation deivce in web(WIP)
- [ ] Deploy: bin(WIP),docker(WIP)
- [ ] Compatible devices
  1. [xiaozhi-esp32](https://github.com/78/xiaozhi-esp32)(WIP)
  1. chobits(cross platform app,create by flutter)(WIP)

## Documentation

You can find user guide documentation on [here](./docs/src/guide/README.md).

You can find user development documentation on [here](./docs/src/development/README.md).

## Quick start

### Development

#### apps/server

```shell
pnpm i
moon run server-ui:build
chobits-server download
# using cuda: moon run server:run --features cuda
moon run server:run
```

- Access home page <http://127.0.0.1:3000>
- Access admin console <http://127.0.0.1:3000/login>
  - default account: root/Change_Me
- Access api documentation <http://127.0.0.1:3000/docs>
- Access device test page <http://127.0.0.1:3000/test/device/test_page.html>
- Client setting
  - ota url
    <http://127.0.0.1:3000/api/ota/>
  - ws url
    <ws://127.0.0.1:3000/chobits/v1/>

#### apps/server-ui

```shell
pnpm i
moon run server-ui:dev
```

#### apps/app

**_TODO_**

### Building

**_TODO_**

### Using

**_TODO_**

## Contributing

Expected workflow is: Fork -> Patch -> Push -> Pull Request

> [!NOTE]
>
> 1. **YOU MUST READ THE [CONTRIBUTORS GUIDE](CONTRIBUTING.md) BEFORE STARTING TO WORK ON A PULL REQUEST.**
> 2. If you have found a vulnerability in the project, please write privately to **<lastsunday@yeah.net>**. Thanks!

## FAQ

See the [FAQ](./docs/src/faq.md) file

## License

This project is licensed under the MIT License.
See the [LICENSE](./LICENSE) file
for the full license text.

## Further information

<details>
<summary>Looking for an overview of the interface? Check it out!</summary>

### Login/Register Page

**_TODO_**

### User Dashboard

**_TODO_**

</details>

## Thanks

<https://github.com/78/xiaozhi-esp32>

<https://github.com/xinnan-tech/xiaozhi-esp32-server>

<https://github.com/joey-zhou/xiaozhi-esp32-server-java>

+++
title = "xiaozhi-esp32-server"
weight = 601
+++

# xiaozhi-esp32-server

> <https://github.com/xinnan-tech/xiaozhi-esp32-server>

## code key

```
main/xiaozhi-server/core/websocket_server.py
  _handle_connection
  handler.handle_connection
    main/xiaozhi-server/core/connection.py
      handle_connection(self, ws)
        //认证
        self.auth.authenticate(self.headers)
        //路由消息
        await self._route_message(message)
          handleTextMessage
            main/xiaozhi-server/core/handle/textHandle.py
              handleTextMessage
                json == int
                  await conn.websocket.send(message)
                json["type"] == "hello"
                json["type"] == "abort"
                json["type"] == "listen"
                  "mode" in json
                    conn.client_listen_mode = msg_json["mode"]

                  json["state"] == "start"
                  json["state"] == "stop"
                    await handleAudioMessage(conn, b"")
                  json["state"] == "detect"

```

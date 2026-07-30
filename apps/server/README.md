# vanling-server

## Development

### Test

```shell
cargo test --workspace
```

## Framework

1. Web framework
   1. axum
2. Database

   1. PostgreSQL

      > <https://www.postgresql.org/docs/17/index.html>

      1. DataType <https://www.postgresql.org/docs/17/datatype.html>
      2. Docker Images <https://hub.docker.com/r/bitnami/postgresql>
         1. username = postgres
         2. password = changeme

3. Database access Framework

   1. SeaOrm

      > <https://github.com/SeaQL/sea-orm> > <https://www.sea-ql.org/sea-orm-cookbook/>

      ```shell
          sea-orm-cli migrate up
          sea-orm-cli generate entity -o src/data/entity
      ```

4. Test

   1. REST API
   2. Database
      1. Testcontainers pg
   3. Logic
   4. BDD
      1. <https://cucumber.io/>
      2. <https://github.com/cucumber-rs/cucumber>
         > <https://cucumber-rs.github.io/cucumber/main/quickstart.html>
         1. <https://cucumber-rs.github.io/cucumber/main/writing/languages.html>

5. Log
   1.

### Setup flow

1. web framework
1. router
1. logger(tracing)
1. configuration(config)
1. database(sea-orm)
1. error(thiserror)
1. request/response tracing(info_span,xid)
1. layer(timeout,body_limit,cors)
1. request,response
   1. ApiResponse
   2. ApiParam
   3. ApiPageParam
   4. ApiPageResult
1. validator(validator,axum-valid)
1. json(serde_json,serde-aux(deserialize_with))
1. custom valid structure(query,path,json)
1. custom valid message
1. auth(jwt,jsonwebtoken)
1. user schema(password(bcrypt,id(xid)))
1. auth api(login,user info)
1. web app
1. include web app to server(rust-embed)
1. build release(profile.releases)
1. upload assets to github, docker image to docker hub
1. openapi + ui(scalar)
1. i18n for error(rust-i18n)
1. ws(axum ws,futures-util)
1. timezone(jiff)
1. tts(sherpa-rs(KokoroTts))
1. opus(opus)
1. vad(sherpa-rs(Vad))
1. asr(sherpa-rs(XAsrRecognizer))

### Websocket handle flow

1. http api/ota -> ws vanling/v1
2. ws.on_upgrade
   1. socket split write and read
      1. read
         1. message convert to frame with message
         2. handler handle frame with message
         3. handler output data use by sender
      1. write
         1. wrapper to sender
         1. sender has some method
            1. send json
            2. send tts
            3. send tts with text

### App

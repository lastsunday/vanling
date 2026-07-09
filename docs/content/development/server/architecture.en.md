+++
title = "Core Architecture"
weight = 200
[extra]
source_file_hash = "8f75a98219a5e973dc6bedc4b2ea024cb38d7b7b"
translated_at = "2026-07-07T00:00:00Z"
+++

# Core Architecture

## Session Overview

- Managed using session + round: a session represents one connection, and a round represents each turn of conversation.
- A session is an abstract connection, not bound to any specific protocol (WebSocket, etc.). It uses `accept_frame` and `output_frame` as the entry and exit points for frame information, and abstracts frame structures such as Hello, Listen, LLM, TTS, etc.
- To facilitate frame collection and continuous start/interruption of conversations, a dual-round model is used: shadow round and running round.

```txt
ws -> input proxy -> session -> output proxy -> ws

input proxy <- record frame
output proxy <- record frame
```

### Session

#### Entry and Exit

```txt
accept_frame

output_frame
  output_controller
```

TODO

#### Abstract Frame

TODO

#### State Machine

- on_idle
  new_round

- on_ready
  new_round
  stop_round

- on_listening

- on_speaking

TODO

### Round

TODO

### Output Controller

TODO

### Activity Time

TODO

### Interruption

TODO

### Frame Collection

TODO

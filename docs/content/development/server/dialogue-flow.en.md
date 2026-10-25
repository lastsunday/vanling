+++
title = "Dialogue Flow"
weight = 204
[extra]
source_file_hash = "e70166d91de3aff0d2d285fee6044331fb62a235"
translated_at = "2026-10-25T00:00:00Z"
+++

# Dialogue Flow

```mermaid
flowchart TB
  subgraph Device
    direction TB
    DeviceSession[Device Session] --> DeviceMCPServer[Device MCP Server]
    DeviceMCPServer .-> DeviceSession
  end
  WebSocket
    subgraph Server
    direction LR
    ServerSession[Server Session]
    ServerMCPHost[Server MCP Host]
    ServerMCPClient[Server MCP Client]
    ServerMCPServer[Server MCP Server]
    RemoteServerMCPServer[Remote Server MCP Server]
    VAD
    ASR
    LLM
    TTS

    ServerSession --> ServerMCPHost
    ServerMCPHost --> ServerMCPClient
    ServerMCPClient --> ServerMCPServer
    ServerMCPServer .-> ServerMCPClient
    ServerMCPClient --> RemoteServerMCPServer
    RemoteServerMCPServer .-> ServerMCPClient
    ServerMCPClient .-> ServerMCPHost
    ServerMCPHost .-> ServerSession

    ServerSession --> VAD
    VAD --> ASR
    ASR --> LLM
    LLM --> ServerMCPHost
    ServerMCPHost .-> LLM
    LLM --> TTS
    TTS .-> ServerSession
  end
  subgraph Transport
    WebSocket
  end

  DeviceSession <--> WebSocket
  WebSocket <--> ServerSession
```

### Handshake Phase

```mermaid
sequenceDiagram
    autonumber
    Device Session ->> Server Session: 1. websocket connect request
    Server Session -->> Device Session: 2. websocket connect response
    Device Session ->> Server Session: 3. hello message request
    Server Session -->> Device Session: 4. hello message response
    alt Hello message response has mcp = true
        Server Session ->> Device Session: 5. mcp initialize message request
        Device Session -->> Server Session: 6. mcp initialize message response
        Server Session ->> Device Session: 7. mcp tools list message request
        Device Session -->> Server Session: 8. mcp tools list message response
        loop Tools list message response has next cursor
            Server Session ->> Device Session: 7. mcp tools list message request
            Device Session -->> Server Session: 8. mcp tools list message response
        end
    end
```

### Communication Phase

```mermaid
sequenceDiagram
    autonumber
    participant DeviceSession as Device Session
    participant ServerSession as Server Session
    DeviceSession ->> ServerSession: audio data
    DeviceSession ->> ServerSession: listen(detect) message
    ServerSession -->> DeviceSession: stt message
    DeviceSession ->> ServerSession: listen(start) message
    loop
      DeviceSession ->> ServerSession: audio data
      break when no voice timeout
        ServerSession ->> DeviceSession: disconnect
      end
      par
        ServerSession ->> ServerSession: vad handle
        opt if voice silence timeout
          ServerSession ->> ServerSession: send main handle stop single
        end
      and
        opt if voice silence timeout
          note right of ServerSession: when recv main handle stop single to exit following logic
          ServerSession ->> ServerSession: asr handle
          ServerSession ->> ServerSession: llm handle
          loop if last llm messages is tools call response
            ServerSession ->> ServerSession: mcp handle
            ServerSession ->> ServerSession: llm handle
          end
          loop
            ServerSession -->> DeviceSession: llm message
            ServerSession -->> DeviceSession: tts(start) message
            ServerSession -->> DeviceSession: tts(sentence start) message
            ServerSession -->> DeviceSession: audio data
            ServerSession -->> DeviceSession: tts(sentence end) message
            ServerSession -->> DeviceSession: tts(stop) message
          end
        end
      end
    end
```

### Listen Modes

The listen mode is decided by the **`mode` field on the `listen(start)` frame** (`auto`/`manual`/`realtime`, mapped to `barge_in` + `is_voice_break_detect`), not by a global Hello field. All three modes share the same node chain (`opus→vad→asr→turn→ling→tts`).

#### Auto

The device sends audio continuously; the server decides the utterance is over via silence confirmation, then runs ASR + LLM + TTS. The default for hands-free conversation.

#### Manual

The server **never answers early**: the device controls when audio starts and stops, receiving on `listen(start)` and committing only on `listen(stop)`. While the button is held the ASR node keeps feeding frames to pre-decode but emits no partials and performs no silence confirmation, so a mid-hold pause cannot truncate the utterance and recognition lands almost instantly on release (the decode was spread across the hold). With no speech it prompts once via `EmptyKind::Manual`; see [Empty-Input Behavior](#empty-input-behavior). Mechanics are in [VAD and Turn Detection](@/development/server/vad.en.md).

#### Realtime

Low-latency mode: VAD onset immediately starts LLM inference and TTS streaming without waiting for silence confirmation. Suited to real-time devices like the ESP32.

### Empty-Input Behavior {#empty-input-behavior}

Empty input (no valid speech) is classified by the hub (Session) and prompted at graded levels; the generation layer only renders wording — a "hub decides, NLG renders" split: the Session classifies, counts and decides, while `Ling` / Echo only words the `Act`. Classification uses mode + previous turn:

| kind | Criterion | Behavior |
| --- | --- | --- |
| `Manual` | push-to-talk: `ListenStop` with no speech detected (`!is_voice_break_detect` and `!speech_active`) | one prompt per keypress ("didn't catch that, please repeat"), then back to listening |
| `Wake` | empty input on the first listen after the wake word (`Input{mode:Wake}`) | guiding prompt ("what can I help you with?"), escalating, back to idle after 3 |
| `AutoSpoke` | hands-free auto: VAD triggered but ASR text empty (`asr_node` emits `EmptyInput`) | "didn't catch that, please repeat" → give an actionable example → graceful close |
| `Silence` | hands-free auto/realtime total silence: VAD never triggered, no ASR stream (`Nothing`) | gentle guide with no blame, escalating, back to idle after 3 |
| `Continuing` | empty input while listening again after a reply (realtime `Speaking→Listening`) | silent wait, no repeated prompting, back to idle on timeout |

**Counting (Rule of three)**: the Session keeps `empty_count`, incrementing on every `EmptyTurn` and resetting on real input (`TurnComplete`); it converges after at most 3. `Manual` is the exception: event-driven, it prompts once per voiceless keypress and then resets, not limited to 3.

**Decision**: after observing `(After, EmptyInput)` / `(After, EmptyTurn)` the Session emits one of three actions — `Prompt{kind, count}` re-injected at the chain head (nodes pass the unrecognised event through, `Ling` renders the wording), `Silence` with no expression injected (silent wait / back to idle), or `GiveUp` back to `Idle`. A real LLM composes its prompt from the Act; the Echo model returns a graded fixed sentence by kind/count.

The `Manual` prompt has exactly one source: in manual mode `finish_stream` returns `Nothing` when VAD detected no speech, so `EmptyKind::Manual` drives it and no double-trigger with the in-chain `EmptyInput` is possible. ASR-side outcomes and thresholds are in [VAD and Turn Detection](@/development/server/vad.en.md).

### MCP Handle

```mermaid
sequenceDiagram
    autonumber
    participant DeviceSession as Device Session
    participant ServerSession as Server Session
    participant ServerMCPServer as Server MCP Server
    alt if call server tool (checked first)
      ServerSession ->> ServerMCPServer: mcp tools call http request
      ServerMCPServer -->> ServerSession: mcp tools call http response
    else if call device tool (fallback)
      ServerSession ->> DeviceSession: mcp tools call message request
      DeviceSession -->> ServerSession: mcp tools call message response
    end
```

Detailed protocol field definitions can be found in [WebSocket Protocol](@/development/server/websocket-protocol.en.md).

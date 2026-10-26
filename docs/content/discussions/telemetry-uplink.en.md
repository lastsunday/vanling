+++
title = "Telemetry Uplink (Publication)"
weight = 40
[extra]
source_file_hash = "8e41a684a0232fc1a8d7051b79fc3ff9a2ce511d"
translated_at = "2026-10-26T07:56:17Z"
+++

# Telemetry Uplink (Publication)

The board publishes its own state. **The transport can change; the data model stays one.** This document is a discussion draft: **nothing is implemented and there is not a single BLE measurement**; the only hardware figures quoted come from the [Wi-Fi implementation record](@/records/iot/wifi.md).

## Scope

Three things must be kept apart, and this document covers only the third:

| Layer | Decided by | Turning it off means |
| --- | --- | --- |
| Sampling | driver / demand model | no data locally |
| Local use | product logic | not drawn on the panel, not fed into the audio chain |
| **Publication** | this document | everything local stays the same, it just stops going out |

"How often to sample" belongs to the [demand model in the architecture document](@/development/iot/architecture.md), not here. This document only answers how state that already exists gets out.

A second boundary: what is published is a **state mirror**, not an event stream. No ordering, no transactions, no delivery confirmation; the last copy overwrites the previous one.

## 1. Data model (transport-neutral)

One snapshot, every slot carrying its own identity and scale:

| Field | Width | Meaning |
| --- | --- | --- |
| `sensor_id` | 1 | board-local slot number; the receiver keys its cache on it |
| `type` | 2 | GATT 16-bit UUID, which also fixes the quantity and scale |
| `value` | 4 | fixed point. **Not f32** |
| `boot_id` | 2 | a new one every power-up |
| `uptime` | 3 | units of 100 ms, wraps after 19.4 days |

Plus a **disclosure mask**: bit i set means slot i is in this round's packet.

### Why a mirror rather than a message

A mirror is idempotent, and a channel cannot hold it — stuffing it into a channel forces a deep queue, a change detector on the writer side, and retry on failure. All three exist only because "a channel cannot hold one value". The architecture document already argues this the same way.

### Why fixed point

f32 may decode to different numbers in two implementations, and this packet has no room for version negotiation. This repository has a second reason: the FPU is disabled on this target, so floating point runs in software (see the [echo cancellation record](@/records/iot/aec.md)).

### Why not absolute time

The board has no RTC. `boot_id + uptime` is only enough for ordering and de-duplication; **the receiver stamps absolute time on arrival**. A change of `boot_id` must flush the receiver's cache, because `uptime` from two `boot_id`s is not comparable.

### How narrow values widen

A UUID fixes the type and the scale but not the width. The rule is fixed: **zero-extend unsigned, sign-extend signed**, and the receiver decodes accordingly. Otherwise the same `0x2A6E` decodes to different numbers in two implementations.

## 2. How the transports differ

| | BLE advertising | Zigbee | Thread | Wi-Fi |
| --- | --- | --- | --- | --- |
| Data model | mirror | last attribute value | mirror | mirror |
| Addressing | none (whoever hears it) | device + endpoint + cluster | IPv6 | IP |
| Delivery guarantee | no ACK, repetition | MAC ACK + route retries | IP retries | TCP |
| Rate control | the board | device-side reporting interval | the board | the board |
| Joining | none | permit-join + network key | dataset + commissioner | credentials |

All four land on "mirror", so **the only difference is the delivery guarantee**, and only two sink shapes are needed:

| Shape | Semantics | Used by |
| --- | --- | --- |
| `Mirror` | overwrite, drop on failure, the next round covers it | BLE advertising, Zigbee attribute reports |
| `Deliver` | addressed + acknowledged, back off and retry | Thread / Wi-Fi (over IP) |

Rotation **does not belong to the abstraction layer**: it is a BLE encoding constraint (31 bytes cannot hold everything), hidden inside the BLE sink and never leaking upward.

## 3. Where each transport lands

### BLE advertising

**PDU**: `ADV_NONCONN_IND` (non-connectable, non-scannable). No connection needed, no scan response, all 31 bytes go to payload; the receiver can scan passively.

**Payload budget**: the flags AD takes 3 B and the manufacturer-specific header takes 4 B (1 length + 1 type + 2 company id), so user data is capped at **24 B**; without flags, 27 B.

**One sensor per packet**:

| Field | Bytes |
| --- | --- |
| company_id | 2 |
| version | 1 |
| sensor_id | 1 |
| type (GATT UUID) | 2 |
| flags | 1 |
| boot_id | 2 |
| uptime (100 ms) | 3 |
| value | 4 |
| total | **16** |

Eight of the 24 bytes are spare and stay untouched for now.

**Why a timer is mandatory**: the controller keeps repeating the last advertising data. So "move to the next sensor" can only happen by **rewriting the advertising data slot by slot**; nothing else changes what is on air. This is not an optimisation, it is the premise of the model.

A full round takes:

```
N_on × k × interval
```

`k` is how many advertising events one sensor is repeated for. On a mains-powered board `interval` is 30–50 ms and `k = 3`, so eight slots take 0.7–1.2 s per round.

**Order** rotates the starting point per round and adds jitter, otherwise it phase-locks with any fixed scan interval.

**Only single-value fixed-point GATT characteristics are allowed**:

| UUID | Quantity | Encoding |
| --- | --- | --- |
| `0x2A6E` | temperature | sint16, 0.01 °C |
| `0x2A6F` | humidity | uint16, 0.01 % |
| `0x2A6D` | pressure | uint32, 0.1 Pa |
| `0x2A19` | battery level | uint8, 1 % |

Composite characteristics with flag bitfields (`0x2A37` heart rate, `0x2A5B` CSC measurement) **do not fit this format** — they are not one `value` and will not go into a fixed 4 bytes.

**Three HCI commands**: `LE Set Advertising Parameters` / `LE Set Advertising Data` / `LE Set Advertising Enable`. `esp-radio`'s `BleConnector::write` / `read_async` take raw bytes, so the encoding is about 50 lines by hand and **`trouble-host` is not needed**.

**Ordering rules**:

- Within one boot, **never mix** legacy and extended advertising commands. After mixing, the controller returns `Command Disallowed` and only `HCI_Reset` recovers.
- Enabling must be **idempotent**: re-enabling an already enabled set also returns `Command Disallowed`, so track the state yourself.
- Whether `Set Adv Data` is accepted while enabled — the extended advertising spec allows it explicitly, legacy implementations differ. **Untested**; if rejected, fall back to disable → set → enable at the cost of one interval of silence.
- **HCI events must be read**. A write-only implementation runs fine and then goes silent at some point: Command Complete and buffer-reclaim events pile up and the controller stops accepting commands.

### Zigbee

Attribute reporting is already a mirror: cluster + attribute + last value, driven by the device-side reporting interval. So it shares the same `Snapshot` and the same mask as BLE.

- **It has MAC ACK**, which makes it the easiest `Deliver` there is: no retry logic to write.
- **No rotation needed**: there is no 31-byte budget, so one round reports every attribute.
- The composite-characteristic limit exists in ZCL too (it has richer data types, but our `value` is still a single value).

### Thread / Wi-Fi

Thread provides only the network layer (IPv6) and **no application data model**. So "Thread support" usually does not mean another sink; it means **another network interface**, with the layer above reusing the `Deliver` path that Wi-Fi already uses (MQTT / HTTP).

That differs in kind from BLE and Zigbee: those two each define what one datum looks like, while the IP transports do not.

**Zigbee and Thread are mutually exclusive**: both run on IEEE 802.15.4, there is one radio, and the MAC is hard real-time on fully overlapping channels with no usable arbiter. Switching can only be a boot-time choice (leave the network, then rejoin), never a hot swap; in a router role, switching amounts to tearing the topology down once.

## 4. Where this lands in the framework

Three things only, for now:

```rust
// iot-core: transport-neutral, testable on the host
struct Snapshot { slots: [Slot; N] }
struct Mask(u8);

// one method; the seam is the point
trait Sink { fn publish(&mut self, snap: &Snapshot); }
```

Plus one `Signal<Snapshot>` as the crossing point, following the Plane shape in `render.rs`.

**Do not build** `Plan` / `resolve` / a registry / `Route`. There is one sink today, and those layers would only ever appear in tests.

The criterion is fixed, and it decides when those layers may appear:

> **When the second sink arrives, the change should touch only `bsp` plus one `match`.**

Beyond that means the seam is in the wrong place; inside that means building it today was wasted. This criterion beats any rehearsal, because it leaves "when to abstract" to facts.

**What the intent looks like later**: the goal is "one intent says which road to take", but the order cannot be reversed — an intent's domain is set by the sinks that exist, and with one sink it has one value, which is a placeholder rather than an abstraction. So today's intent is one sentence: "which slots are disclosed". `Route` arrives with the second sink, and only then does it deserve a second variant.

**The disclosure mask is per sink**, not global: BLE is plaintext and anyone can hear it, Wi-Fi goes to our own backend. The same publication can carry different masks on the two paths.

**Failure isolation**: each sink gets its own `join` branch, and the publisher only writes into each one's `Signal`. Do not loop over sinks with `await` — one stuck sink blocks them all.

## 5. The two chips

### S3's BT power-up: one refuted inference

An earlier reading of `esp-radio`'s `init_clocks()` led to the inference that "the BT clock is cleared", and from there to "nothing powers up the BT domain on the S3". **That was wrong**:

- Those three bits in `init_clocks()` are `I2C_CLK` / `UNUSED_BIT12` / `SDIO_HOST`, not BT clocks; the S3's `enable_bt()` is itself a no-op.
- `enable_wifi_power_domain()` on the S3 (`soc_has_bt`) pulses `WIFIBB/FE/WIFIMAC + BTBB_RST/BTMAC_RST/RW_BTMAC_RST/RW_BTMAC_REG_RST/BTBB_REG_RST`.
- `init()` is triggered by `RadioRefGuard::new()` on the refcount's 0→1 transition, and the Wi-Fi driver's `new()` is exactly that 0→1 caller.

So when Wi-Fi starts first, the BT reset has already run. This repository's hand-written `ungate_radio()` covers only the three Wi-Fi bits, but it runs before `init()`, so it most likely leaves nothing missing — **to be confirmed on hardware**.

### Memory: one pool, not two

| Item | Figure | Source |
| --- | --- | --- |
| S3 radio heap total | 48 KiB (16 KiB `dram_seg` + 32 KiB dcache reclaimed) | `chip-esp/src/radio/esp32s3.rs` |
| Left after Wi-Fi runs | **15,820 B** | `records/iot/wifi.md` (measured) |
| BLE controller needs | 20–30 KiB (typical) | **untested** |
| controller task stack | 8 KiB by default | `Config::default()` |

The easy mistake: **those 8 KiB are not a second account alongside the controller's own heap**. `esp-radio`'s chip features pull in both `esp-rtos` and `esp-radio-rtos-driver`; controller tasks take their stack from `InternalMemory` through `esp-rtos`'s `spawn`, and `InternalMemory` only matches regions declared `Internal` in `esp_alloc::HEAP` — the only `Internal` region this repository registers on this part **is the radio's 48 KiB** (the app heap is a separate `LlffHeap` living in PSRAM). So those 15,820 B must hold both the controller's memory and its task stack.

### C6

| | C6 |
| --- | --- |
| controller | `npl` (the S3 uses `btdm`); the two `create_ble_config`s and their memory needs do not substitute for each other |
| BT clock | `bt_periph_module_enable()` really calls `enable_bt(true)`; `ungate_radio()` is empty, and correctly so |
| radio heap | also 48 KiB, but **one block** that does not compete with the main stack |
| headroom | **untested**; the S3's 15,820 B must not be substituted |
| product path | none (no PSRAM, no LCD_CAM, not enough GPIO) |

### `coex` is not a soft feature

`esp-radio`'s `coex` calls `crate::wifi::coex_init` directly inside `ble_init`, so it depends on `wifi` in code. That fixes where it goes:

```toml
bt = ["dep:esp-radio", "esp-radio/ble", ...]                  # compiles alone
lckfb-szpi-esp32s3 = [..., "bt", "wifi", "esp-radio/coex"]    # combination
```

The cost is one **combination entry** in `check-features` — the only item in that script that cannot run "alone". It is not two soft features; it is the hardware fact that two capabilities share one antenna at the same time.

### The 802.15.4 PHY is already here, the protocol stacks are not

`esp-radio` has an `ieee802154` feature, so the radio driver exists; but the Zigbee and Thread stacks are not in that crate and need additional C libraries. So the cost of "adding Zigbee" is not in the radio, it is in the stack, device compatibility, and certification.

## 6. Rejected alternatives

| Alternative | Why rejected |
| --- | --- |
| Packing several sensors into one packet | does not fit everything; and the switch can only be expressed as "bit is 0", hard to tell apart from "not sent" |
| Multiple advertising sets (BLE 5.0) | removes the timer, but controller and receiver support varies and airtime stacks up |
| `trouble-host` / a direct `bt-hci` dependency | only three HCI commands are needed, and `BleConnector` already takes `&[u8]` |
| A queue with fixed-rate sending | advertising has no ACK, so a queue cannot promise delivery; the first entries sent are stale state |
| `Plan` / `resolve` / a registry | with one sink, those layers would only appear in tests |
| Runtime switching between Zigbee and Thread | leaving plus rejoining plus neighbour rebuild is not a hot swap; in a router role it tears the network down |
| Using the MAC as the device ID | phone-side addresses rotate; the device ID belongs in the payload |

## 7. Open questions (all need hardware)

1. **Is there enough on the S3 under coex**: 15,820 B must hold both controller memory and an 8 KiB task stack, against a typical 20–30 KiB need.
2. **What is C6's equivalent headroom.**
3. **Is `LE Set Adv Data` accepted while enabled**: decides whether there is a silent gap.
4. **What is the real jitter of the advertising interval while Wi-Fi scans**: decides whether this broadcast is usable under coex.

## 8. How to continue

Do not jump to coex in one go. Run two passes so the two variables stay separate:

| Probe | Phase | What it measures |
| --- | --- | --- |
| BLE-only | refcount 0→1, `init()` runs | where the radio heap and task stack land, whether a phone sees the advertising |
| Wi-Fi + coex | refcount 2→3, `init()` does not run | how much heap the same advertising costs, interval jitter while scanning |

Comparing the two readings separates "power domain" from "memory" — otherwise a coex crash leaves you unable to tell which one it was.

Run the first on the C6 (clean baseline) and both on the S3 (comparison).

Once there are readings, the measured parts move into `records/iot/`; this file keeps the reasoning and the trade-offs.

+++
title = "Resource Files"
weight = 700
[extra]
source_file_hash = "6e243d6559dd5a56f398b8b85a276225e5fb47d0"
translated_at = "2026-06-28T18:00:00Z"
+++

## File Properties

```
General
Complete name                  : samples_jfk.wav
Format                         : Wave
File size                      : 344 KiB
Duration                       : 11 s 0 ms
Overall bit rate mode          : Constant
Overall bit rate               : 256 kb/s
Writing application            : Lavf59.27.100

Audio
Format                         : PCM
Format settings                : Little / Signed
Codec ID                       : 1
Duration                       : 11 s 0 ms
Bit rate mode                  : Constant
Bit rate                       : 256 kb/s
Channel(s)                     : 1 channel
Sampling rate                  : 16.0 kHz
Bit depth                      : 16 bits
Stream size                    : 344 KiB (100%)
```

## File Properties JSON

```json
{
  "header": {
    "ckID": "RIFF",
    "ckSize": 352070,
    "format": "WAVE"
  },
  "data": [
    {
      "chunk": {
        "chunkId": "fmt%20",
        "chunkSize": 16
      },
      "fmt": {
        "formatTag": "WaveFormatType%3A%3APCM",
        "channels": 1,
        "samplesPerSec": 16000,
        "avgBytesPerSec": 32000,
        "blockAlign": 2
      },
      "pcmExtraData": {
        "bitsPerSample": 16
      }
    },
    {
      "chunk": {
        "chunkId": "LIST",
        "chunkSize": 26
      },
      "list": {
        "type": "INFO",
        "item": [
          {
            "chunkId": "ISFT",
            "chunkSize": 14
          }
        ]
      }
    },
    {
      "chunk": {
        "chunkId": "data",
        "chunkSize": 352000
      }
    }
  ]
}
```

## Format

| Data           | Value   | Calculation  | Name               | Field Size | Endian | Name                  | Notes                          |
| -------------- | ------- | ------------ | ------------------ | ---------- | ------ | --------------------- | ------------------------------ |
| 52 49 46 46    | RIFF    |              | Chunk ID           | 4          | Big    | RIFF Header Signature |                                |
| 46 5F 05 00    | 352070  | HEX 00055F46 | Total chunk size   | 4          | Little | RIFF Chunk Size       |                                |
| 57 41 56 45    | WAVE    |              | File format        | 4          | Big    | WAVE Header Signature |                                |
| 66 6D 74 20    | fmt     |              | Sub-chunk 1 tag    | 4          | Big    | chunkId               |                                |
| 10 00 00 00    | 16      | HEX 00000010 | Sub-chunk 1 size   | 4          | Little | chunkSize             | Unit: bytes                    |
| 01 00          | 1       | HEX 0001     | Audio format       | 2          | Little | formatTag             | 1 (PCM)                        |
| 01 00          | 1       | HEX 0001     | Number of channels | 2          | Little | channels              | 1 (mono) 2 (stereo)            |
| 80 3E 00 00    | 16000   | HEX 00003E80 | Sample rate        | 4          | Little | samplesPerSec         | Samples/second (Hz)            |
| 00 7D 00 00    | 32000   | HEX 00007D00 | Bit rate           | 4          | Little | avgBytesPerSec        | =sample_rate * bit_depth * channels / 8 |
| 02 00          | 2       | HEX 0002     | Block align        | 2          | Little | blockAlign            |                                |
| 10 00          | 16      | HEX 0010     | Bit depth          | 2          | Little | bitsPerSample         | Bit depth                      |
| 64 61 74 61    | data    |              | Sub-chunk tag      | 4          | Big    | chunkId               |                                |
| 00 5F 05 00    | 352000  | HEX 00055F00 | Sub-chunk size     | 4          | Little | chunkSize             | Unit: bytes                    |

### Notes

| Name    | tag  | Notes |
| ------- | ---- | ----- |
| chunkId | fmt  |       |
|         | data |       |
|         | fact |       |
|         | smpl |       |
|         | cue  |       |
|         | LIST |       |
|         | id3  | Other |

## Calculations

```
1. Playback duration = 11 s
   1. Formula 1: data[chunkSize] / avgBytesPerSec
      352000 / 32000
   2. Formula 2: data[chunkSize] * 8 / (samplesPerSec * bitsPerSample * channels)
      352000 * 8 / (16000 * 16 * 1)
2. File size = 352078 B
   1. Formula 1: RIFF Chunk Size + RIFF Header Signature(field size) + RIFF Chunk Size(field size)
      352070 + 4 + 4
```

## Notes

1. Format settings
   Little-Endian: least significant byte stored first.
   Big-Endian: most significant byte stored first.

## Other

1. Hex tool <https://github.com/WerWolv/ImHex>

+++
title = "资源文件"
weight = 700
+++

## 文件属性

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

## 文件属性 json

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

## 格式

| 数据        | 值     | 计算方式     | 名称          | 区块大小 | 序端 | Name                  | 备注                          |
| ----------- | ------ | ------------ | ------------- | -------- | ---- | --------------------- | ----------------------------- |
| 52 49 46 46 | RIFF   |              | 块区编号      | 4        | 大   | RIFF Header Signature |                               |
| 46 5F 05 00 | 352070 | HEX 00055F46 | 总区块大小    | 4        | 小   | RIFF Chunk Size       |                               |
| 57 41 56 45 | WAVE   |              | 文件格式      | 4        | 大   | WAVE Header Signature |                               |
| 66 6D 74 20 | fmt    |              | 子区块 1 标签 | 4        | 大   | chunkId               |                               |
| 10 00 00 00 | 16     | HEX 00000010 | 子区块 1 大小 | 4        | 小   | chunkSize             | 单位：字节                    |
| 01 00       | 1      | HEX 0001     | 音频格式      | 2        | 小   | formatTag             | 1（PCM）                      |
| 01 00       | 1      | HEX 0001     | 声道数量      | 2        | 小   | channels              | 1（单声道）2 立体声           |
| 80 3E 00 00 | 16000  | HEX 00003E80 | 取样频率      | 4        | 小   | samplesPerSec         | 取样点/秒（Hz）               |
| 00 7D 00 00 | 32000  | HEX 00007D00 | 比特率        | 4        | 小   | avgBytesPerSec        | =取样频率*比特深度*声道数量/8 |
| 02 00       | 2      | HEX 0002     | 区块对齐      | 2        | 小   | blockAlign            |                               |
| 010 00      | 16     | HEX 0010     | 比特深度      | 2        | 小   | bitsPerSample         | 位元深度                      |
| 64 61 74 61 | data   |              | 子区块标签    | 4        | 大   | chunkId               |                               |
| 00 5F 05 00 | 352000 | HEX 00055F00 | 子区块大小    | 4        | 小   | chunkSize             | 单位：字节                    |

### 备注

| Name    | tag  | 备注 |
| ------- | ---- | ---- |
| chunkId | fmt  |      |
|         | data |      |
|         | fact |      |
|         | smpl |      |
|         | cue  |      |
|         | LIST |      |
|         | id3  | 其他 |

## 计算

```
1. 播放时间 duration = 11 s
   1. 公式 1：data[chunkSize] / avgBytesPerSec
      352000 / 32000
   2. 公式 2：data[chunkSize] * 8 / (samplesPerSec * bitsPerSample * channels)
      352000 * 8 / (16000 * 16 * 1)
2. 文件大小 file size = 352078 B
   1. 公式 1：RIFF Chunk Size + RIFF Header Signature(区块大小) + RIFF Chunk Size(区块大小)
      352070 + 4 + 4
```

## 备注

1. Format settings
   Little-Endian
   （小端序）：最低有效字节先存储。
   Big-Endian
   （大端序）：最高有效字节先存储。

## 其他

1.Hex 工具 <https://github.com/WerWolv/ImHex>

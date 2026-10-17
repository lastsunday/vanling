+++
title = "检查任务与缓存记录"
weight = 80
sort_by = "weight"
+++

<!-- doc-audience: ai -->

# 检查任务与缓存记录

`apps/iot` 的验证循环：哪些任务真的每次都跑、`moon` 的缓存键由什么组成、C6 的产品检查为什么不再进容器。**AI 产出，未经人类审阅**，见[上级说明](@/records/_index.md)。

每条判断标注依据：**`[测试]`** 有对应单元测试，**`[实测]`** 有实测数字并附复现方式，**`[据称]`** 未复现、仅留痕。

## 一句话

改前的模型是「每次 `moon run` 都把整张矩阵重编一遍」：`check-features` 写着 `cache: false`，`check-c6` 走容器。现在同一棵树重跑是 **0.09 s / 0.10 s**，真正改了文件的那一次仍付全价。**省下的只是「没改也重来」，不是任何一条验证。**

## moon 的缓存键里有什么

`[实测]` `.moon/cache/hashes/<hash>.json` 就是键本身，它逐项列出：`command`/`script`（任务文本 ⇒ 改 `moon.yml` 里的命令会失效）、`args`、`env`（项目级 `env:`）、`inputEnv`、`outputs`、`toolchains`、`version`，以及 `inputs` —— **每个文件一条 sha1**。

- **不写 `inputs` 时，默认是「这个项目下的每个文件」**：`check-host` 的默认键里有 121 条，含 `.gitignore`、`assets/*.mid`、`partitions/*.csv`、`linker/*.x`，另加 `.moon/workspace.yml` 与 `.moon/toolchains.yml`。`apps/iot/moon.yml` **不在**其中（命令文本另在 `script` 项里，所以改命令仍然失效）。
- **写了 `inputs` 是替换，不是叠加。** 只写脚本名会把项目文件整批踢出去。
- 相对路径按项目根解析，`..` 不合法（moon 直接拒绝）；**以 `/` 开头按仓库根解析**。

`[实测]` `'**/*'` 覆盖了默认那 121 条里除 `.moon/*` 之外的全部（点文件、`.mid`、`.csv`、`.x` 一条不缺），另加 `apps/iot/moon.yml` 本身。`moon.yml` 的 `fileGroups.build-inputs` 就是这条：`'**/*'` + `'/scripts/iot-xtensa.sh'`。

### 盲点：经仓库根脚本编译的任务

`check-s3` / `check-features` / 七个 `probe-*-s3` 都经 `scripts/iot-xtensa.sh` 编译，而那个脚本在项目外 ⇒ 不在默认哈希里 ⇒ **改了分派脚本，缓存不失效**：moon 会重放一个旧结论，或者把一份分派脚本已经不再描述的探针 ELF 当作完好的 `outputs` 留在盘上。

`[实测]` 直接验过（拿 `check-host` 当靶子，把脚本临时加进它的 `inputs`）：基线键 `0973f854`；给 `scripts/iot-xtensa.sh` 追加一行注释后任务**真跑**、键变 `577401ca`；还原成逐字节相同的文件后键回到 `0973f854` 且**命中缓存**。⇒ 脚本内容确实进键，且键是内容寻址的。

所以这些任务的 `inputs` 现在显式写出来（`@group(build-inputs)`，`check-features` 另加自己那个脚本）。`check-c6` 不在其中——它已经不经分派脚本。

## `check-features`：`cache: false` 换成内容哈希

矩阵 15 个子集（宿主 6 + 裸机目标 9），每个子集对 cargo 都是一套没见过的 feature 组合。

| 情形     | 改前                  | 改后                                  |
| -------- | --------------------- | ------------------------------------- |
| 什么都没改重跑 | 全价（`cache: false`） | **0.09 s**（重放，输出带 `cached`）   |
| 改了源码 | 全价                  | 全价：**2m20s570ms**                  |
| 只改了脚本 | 全价                | **35 s**（cargo 产物仍新鲜）          |

`[实测]` 单个 S3 子集：产物热时重跑 **2.48 s**，首次 **14.84 s** ⇒ 那 ~15 s 是真的在编译，不是分派开销。`docker run` 起一次（含两个挂载、`-w`、`bash -lc`、`source export-esp.sh`）**0.45 s**。所以那两分钟买的是 cargo 的工作量，缓存能省的只有「没改也重来」那一份。

## `check-c6`：容器 → 宿主 stable

现在直接是宿主命令（`--bin vanling --bin wifi-probe`，两个 feature 与默认值等价），依据三条：

1. `riscv32imac-unknown-none-elf` 有预编译 `core`/`alloc`（`rust-toolchain.toml` 的 `targets` 就装着）⇒ 不需要 esp fork，也不需要 `-Z build-std`；
2. C6 的**固件本来就由宿主 stable 产出**（`build` → `image-c6` 都走宿主）⇒ 容器对「镜像能不能出来」没有额外保证；
3. `[实测]` **1m12s**（容器，先前读数）→ **1.46 s cargo / 1.654 s 任务**（产物热）。

`[实测]` `check-c6` 之后立刻跑 `check` 是 0.19 s（产物新鲜）⇒ 两者共用同一份产物，交替跑不互相打翻。**S3 仍然走容器**：Xtensa 没有预编译 rust-std。

## 被否：8 次 `docker run` 合成 1 次

原计划里有这条，理由是「每次 `docker run` 有固定成本」。**实测推翻了理由**：起一次 0.45 s，不是 5.4 s（5.4 s 是一次冷容器/冷 VM 的读数，重跑同一条命令三次都是 0.45 s 上下）⇒ 合成一次最多省 **3.5 s**，代价是给 `iot-xtensa.sh` 加一套批量协议（stdin 传命令行、失败归属、离线重试语义）。不划算，不做。

## 保留的守卫：C6 子集仍经分派脚本

`iot-xtensa.sh` 里 `ESP_HAL_CONFIG_DATA_CACHE_SIZE` 是 S3-only，无条件 export 会把 C6 构建炸掉（esp-config 把对本芯片不生效的选项当未知项直接失败）。这条 gating 原先由 CI 里的 `check-c6` 守着；它改宿主之后，**只有 `check-features` 的 C6 子集还在把非 S3 目标交给分派脚本**，所以那一个子集特意不改：它对这次检查本身没好处（多起一个容器），但它是这条 gating 在 CI 里唯一的见证。

## 未决

- **`check-s3` 拆产品／探针两条**：收益未量，且会动摇「五个 bin 每次 `check-s3` 都编一遍」这条现成的防腐线（见[本目录首页](@/records/iot/_index.md) 的「探针」节）。
- **`smoke-host` 开了缓存**（15 s → **0.084 s** 重放）：它的产物是一段日志，重放会带 `cached` 标记；要真跑用 `moon run --force`。
- **sccache／`target` 瘦身、Docker VM 内存或 `CARGO_BUILD_JOBS`、测试 profile 的 debuginfo**：三条都只有在真实脏状态（改一个 core 文件 → `cargo test -p iot-core` 51 s）下才量得准，本轮**没量**。
- 机器侧：13 GB 空闲磁盘 / 8 GB 内存，`apps/iot/target` 8.6 G + 根 `target` 3.5 G + sccache 11 G。八套 S3 子集并行需要各自的 target 目录（cargo 对同一 target 目录加锁），磁盘放不下——这是矩阵不能再快的机器侧原因。

+++
title = "Flutter App"
weight = 300
+++

# Flutter App

Flutter 客户端（`apps/app`）目前是**基础框架**，尚未接入 vanling 业务功能。跨端语义见[共享客户端协议](@/development/clients/shared-protocol.md)，页面结构见[页面实现](@/development/clients/pages.md)。

## 当前能力

| 能力 | 状态 |
| --- | --- |
| 主题框架 / 自适应布局（含桌面端判断）/ 文字缩放 / 国际化 | 已具备 |
| 网络层（Dio）/ 数据库（Sqlite）/ 事件总线 / 唯一 ID（nanoid2） | 已具备 |
| 环境配置（开发 / 生产） | 已具备 |
| 认证（Spring-authorization-server） | 已具备 |
| 自动升级 | Android 已具备 |
| 日志 | 轮转日志文件（Web 除外）已具备；导出/上传未实现 |
| 发布 | Android、Web 已具备；iOS、Windows（exe 免安装）、Linux、macOS 未实现 |
| 用户信息 / 集成测试 / CI 代码质量与测试 | 未实现 |

## 环境要求

| 工具 | 说明 |
| --- | --- |
| Xcode | iOS/macOS 构建；装好后 `sudo xcode-select --switch /Applications/Xcode.app` |
| Android Studio | Android SDK 管理，首次启动装 SDK（API 34/35） |
| Chrome | Web 调试，`flutter run -d chrome` |

工具链已包含在 `nix develop` 里，进 shell 后即可 `flutter test` / `flutter analyze` / `flutter build apk`。

首次使用：`nix develop` → `flutter doctor --android-licenses` → `flutter pub get` → `flutter doctor` 确认就绪。

## 发布 Android

1. 创建密钥库

   ```shell
   keytool -genkey -v -keystore .\android-app-keystore.jks -storetype JKS -keyalg RSA -keysize 2048 -validity 10000 -alias android-app
   ```

2. 创建 `[project]/android/key.properties` 引用密钥库

   ```properties
   storePassword=<上一步的密码>
   keyPassword=<上一步的密码>
   keyAlias=android-app
   storeFile=<密钥库文件路径>
   ```

3. 打包（生产环境）

   ```shell
   flutter build apk --dart-define=DART_DEFINE_APP_ENV=prod
   ```

## 编码约定

- JSON 模型改动后跑 `dart run build_runner build`。
- Web 端 Sqlite3 要先装二进制：`dart run sqflite_common_ffi_web:setup`，会在 `web/` 下生成 `sqlite3.wasm` 与 `sqflite_sw.js`；升级 sqlite3 版本后用 `--force` 重跑。
- 提交信息遵循 [Conventional Commits](https://www.conventionalcommits.org/zh-hans/v1.0.0/)。

## 坐标系

服务端按 WGS84 存坐标，客户端用高德地图（GCJ-02），转换走 `coordtransform_dart`。跨端读写坐标必须在这一层换算，不要各自假设。

## 已知问题

`pod install` 慢时先 `cd ios/` 再 `pod install --verbose`；需要代理就导出 `https_proxy` / `http_proxy` / `all_proxy`。

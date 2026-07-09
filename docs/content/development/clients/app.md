+++
title = "客户端"
weight = 300
+++

# 客户端

## 环境要求

### 前置依赖

| 工具 | 说明 | 安装方式 |
|------|------|----------|
| Xcode | iOS/macOS 构建 | App Store 下载，运行 `sudo xcode-select --switch /Applications/Xcode.app` |
| Android Studio | Android SDK 管理 | [官网下载](https://developer.android.com/studio)，首次启动时安装 SDK（API 34/35） |
| Chrome | Web 调试 | 已安装则 `flutter run -d chrome` 可用 |

### Nix 开发环境

进入项目根目录后直接用 `nix develop`，Flutter 工具链已包含在内：

```shell
nix develop
flutter test
flutter analyze
flutter build apk
flutter doctor
```

### 首次使用

1. 安装前置依赖（Xcode、Android Studio、Chrome）
2. 进入 Nix shell：`nix develop`
3. 接受 Android 许可证：`flutter doctor --android-licenses`
4. 安装项目依赖：`flutter pub get`
5. 运行 `flutter doctor` 确认环境就绪

> [!WARNING]
> App 尚未实现 chobits 业务功能，当前仅为基础框架。

## 框架

- [x] 主题框架
- [x] 自适应布局
  - [x] 桌面端判断
- [x] 文字缩放
- [x] 国际化
- [x] 网络层
  - [x] Dio
- [x] 数据库
  - [x] Sqlite
- [x] 工具
  - [x] 唯一 ID
    - [x] nanoid2
- [x] 事件总线
- [x] 自动升级
  - [x] Android
- [ ] 日志
  - [x] 轮转日志文件（不含 Web 环境）
  - [ ] 导出/上传日志文件
- [ ] 发布
  - [x] Android
  - [ ] iOS
  - [ ] Windows
    - [x] exe（免安装）
  - [ ] Linux
  - [ ] macOS
  - [x] Web
- [x] 环境配置
  - [x] 开发环境
  - [x] 生产环境

## 进阶

- [x] 认证
  - [x] Spring-authorization-server
- [ ] 用户信息

## 开发流程

- [x] 变更日志
- [ ] CI
  - [x] 构建
  - [ ] 代码质量
  - [ ] 测试
- [ ] 测试
  - [x] 单元测试（示例）
  - [x] Widget 测试（示例）
  - [ ] 集成测试

## 编码

### Database Versioning

> lib\modules\app\app_store.dart

```dart
//DB init
DbManager.instance().init([ChangeLogV1()]);
Database db = await DbManager.instance().open();
```

### Database Record To View Model

> Example

```dart
class MemoMapper{
    static Future<List<MemoEntity>> selectAll() async {
        List<Map<String, dynamic>> findResult =
            await DbManager.instance().find(MemoEntity.tableName);
        return Future.value(findResult.map((e) => MemoEntity.fromJson(e)).toList());
  }
}

class AppStore{
    Future<List<MemoModel>> getMemoList() async {
        List<MemoEntity> memoEntityList = await MemoMapper.selectAll();
        return Future(() =>
            memoEntityList.map((e) => MemoModel.fromJson(e.toJson())).toList());
      }
}

class _MemoPageState{

    void initState() {
        super.initState();
        Provider.of<AppStore>(context, listen: false).getMemoList().then((value) {
          setState(() {
            memoModelList = value;
          });
        });
      }
}
```

### Pagging(Pull refresh and load more)

> Example

```dart
class _MemoPageState extends State<MemoPage> {
  List<MemoModel> _memoModelList = [];
  late EasyRefreshController _controller;
  late int _pageNum;
  late int _pageSize;
  late int _total;

  @override
  void initState() {
    super.initState();
    resetPageInfo();
    _controller = EasyRefreshController(
      controlFinishRefresh: true,
      controlFinishLoad: true,
    );
  }

  @override
  void dispose() {
    _controller.dispose();
    super.dispose();
  }

  void resetPageInfo() {
    setState(() {
      _pageNum = 1;
      _pageSize = 10;
      _total = 0;
      _memoModelList = [];
    });
  }

  Future<PageResult> _loadData() async {
    var result = await Provider.of<AppStore>(context, listen: false).pageMemo(
        PageParam(
            pageNum: _pageNum, pageSize: _pageSize, orderBy: " datetime desc"));
    if (!mounted) {
      return Future.value(result);
    }
    _memoModelList.addAll(result.rows);
    _total = result.total;
    LogHelper.debug(
        "[Memo] loadData pageNum = $_pageNum,pageSize = $_pageSize,total = $_total");
    return Future.value(result);
  }

  void _refreshList() async {
    resetPageInfo();
    await _loadData();
    setState(() {});
    _controller.finishRefresh();
    _controller.resetFooter();
  }

  void _loadList() async {
    _pageNum++;
    PageResult result = await _loadData();
    setState(() {});
    if (result.hasNext) {
      _controller.finishLoad(IndicatorResult.success);
    } else {
      _controller.finishLoad(IndicatorResult.noMore);
    }
  }

  @override
  Widget build(BuildContext context) {
    return Scaffold(
        body: EasyRefresh(
            refreshOnStart: true,
            controller: _controller,
            header: RefreshHeader(context),
            footer: RefreshFooter(context),
            onRefresh: _refreshList,
            onLoad: _loadList,
            child: CustomScrollView(slivers: [
              SliverGrid(
                delegate: SliverChildBuilderDelegate((context, index) {
                  return _createMemoItem(_memoModelList[index]);
                }, childCount: _memoModelList.length),
                gridDelegate: const SliverGridDelegateWithMaxCrossAxisExtent(
                    maxCrossAxisExtent: 210),
              )
            ])),
        floatingActionButton: _getFloatingActionButton());
  }
}
```

### HttpRequest

```dart
Response result = await HttpClient.instance().get("/");
LogHelper.info(result.body().toString());
```

## JSON 模型生成

```shell
dart run build_runner build
```

## 发布

### Android

> [构建和发布 Android 应用 | Flutter](https://docs.flutter.dev/deployment/android)

1. 创建密钥库

   ```shell
   keytool -genkey -v -keystore .\android-app-keystore.jks -storetype JKS -keyalg RSA -keysize 2048 -validity 10000 -alias android-app
   ```

2. 创建 `[project]/android/key.properties` 并引用密钥库

   ```properties
   storePassword=<上一步的密码>
   keyPassword=<上一步的密码>
   keyAlias=android-app
   storeFile=<密钥库文件路径>
   ```

3. 执行发布命令（以生产环境为例）

   ```shell
   flutter build apk --dart-define=DART_DEFINE_APP_ENV=prod
   ```

## 提交规范

> <https://www.conventionalcommits.org/zh-hans/v1.0.0/>


## Web 端 Sqlite3

> sqflite_common_ffi_web

### 安装二进制文件

需将 [sqlite3.wasm](https://github.com/simolus3/sqlite3.dart/releases) 及相关 worker 文件放入 web 目录：

```bash
dart run sqflite_common_ffi_web:setup
```

会在 web 目录下创建：

- `sqlite3.wasm`
- `sqflite_sw.js`

更新 sqlite3 版本后需使用 `--force` 重新安装：

```bash
dart run sqflite_common_ffi_web:setup --force
```

## 离线地图

1. 使用 QGIS 生成 XYZ 瓦片：工具箱 → 生成 XYZ 瓦片（目录）
2. 生成 `pubspec.yaml` 资源路径

```bash
ls -R assets | grep ':'
```

## 坐标系

1. 服务端使用 WGS84 格式存储坐标
2. 客户端使用高德地图（GCJ-02 坐标系）
3. 使用 [coordtransform_dart](https://pub-web.flutter-io.cn/packages/coordtransform_dart) 进行坐标转换

## 常见问题

Q1. setState() or markNeedsBuild() called during build

> <https://fluttercorner.com/setstate-or-markneedsbuild-called-during-build-a-vertical-renderflex-overflowed/>

解决方案：使用回调函数延迟 setState 调用

```dart
WidgetsBinding.instance.addPostFrameCallback((_){
  // 你的代码
});
```

## 其他

1. pod install 速度慢

```shell
cd ./ios/
# 如有代理，取消下面注释
# export https_proxy=http://127.0.0.1:7890 http_proxy=http://127.0.0.1:7890 all_proxy=socks5://127.0.0.1:7890
pod install --verbose
```

## 相关项目

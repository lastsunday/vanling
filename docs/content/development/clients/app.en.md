+++
title = "App Client"
weight = 300
[extra]
source_file_hash = "52d986968906e788ff51158861be1c854bff54dd"
translated_at = "2026-06-28T18:00:00Z"
+++

# App Client

## Environment Requirements

### Prerequisites

| Tool | Description | Installation |
|------|-------------|-------------|
| Xcode | iOS/macOS build | Download from App Store, run `sudo xcode-select --switch /Applications/Xcode.app` |
| Android Studio | Android SDK management | [Official site](https://developer.android.com/studio), install SDK (API 34/35) on first launch |
| Chrome | Web debugging | If already installed, `flutter run -d chrome` is available |

### Nix Development Environment

Enter the project root and use `nix develop` — the Flutter toolchain is included:

```shell
nix develop
flutter test
flutter analyze
flutter build apk
flutter doctor
```

### First Use

1. Install prerequisites (Xcode, Android Studio, Chrome)
2. Enter Nix shell: `nix develop`
3. Accept Android licenses: `flutter doctor --android-licenses`
4. Install project dependencies: `flutter pub get`
5. Run `flutter doctor` to verify the environment is ready

> [!WARNING]
> The App does not yet implement chobits business functionality; it is currently a basic framework.

## Framework

- [x] Theme framework
- [x] Responsive layout
  - [x] Desktop detection
- [x] Text scaling
- [x] Internationalization
- [x] Network layer
  - [x] Dio
- [x] Database
  - [x] Sqlite
- [x] Utilities
  - [x] Unique ID
    - [x] nanoid2
- [x] Event bus
- [x] Auto-update
  - [x] Android
- [ ] Logging
  - [x] Rotating log files (excluding Web)
  - [ ] Export/upload log files
- [ ] Distribution
  - [x] Android
  - [ ] iOS
  - [ ] Windows
    - [x] exe (portable)
  - [ ] Linux
  - [ ] macOS
  - [x] Web
- [x] Environment configuration
  - [x] Development
  - [x] Production

## Advanced

- [x] Authentication
  - [x] Spring-authorization-server
- [ ] User info

## Development Workflow

- [x] Changelog
- [ ] CI
  - [x] Build
  - [ ] Code quality
  - [ ] Tests
- [ ] Testing
  - [x] Unit tests (example)
  - [x] Widget tests (example)
  - [ ] Integration tests

## Coding

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

## JSON Model Generation

```shell
dart run build_runner build
```

## Distribution

### Android

> [Build and release an Android app | Flutter](https://docs.flutter.dev/deployment/android)

1. Create keystore

   ```shell
   keytool -genkey -v -keystore .\android-app-keystore.jks -storetype JKS -keyalg RSA -keysize 2048 -validity 10000 -alias android-app
   ```

2. Create `[project]/android/key.properties` referencing the keystore

   ```properties
   storePassword=<password from previous step>
   keyPassword=<password from previous step>
   keyAlias=android-app
   storeFile=<keystore file path>
   ```

3. Run the release command (production as example)

   ```shell
   flutter build apk --dart-define=DART_DEFINE_APP_ENV=prod
   ```

## Commit Convention

> <https://www.conventionalcommits.org/en/v1.0.0/>

## Web Sqlite3

> sqflite_common_ffi_web

### Install Binary Files

Place [sqlite3.wasm](https://github.com/simolus3/sqlite3.dart/releases) and related worker files in the web directory:

```bash
dart run sqflite_common_ffi_web:setup
```

This will create in the web directory:

- `sqlite3.wasm`
- `sqflite_sw.js`

After updating the sqlite3 version, reinstall with `--force`:

```bash
dart run sqflite_common_ffi_web:setup --force
```

## Offline Maps

1. Generate XYZ tiles with QGIS: Toolbox → Generate XYZ Tiles (Directory)
2. Generate `pubspec.yaml` asset paths

```bash
ls -R assets | grep ':'
```

## Coordinate System

1. Server stores coordinates in WGS84 format
2. Client uses Amap (GCJ-02 coordinate system)
3. Use [coordtransform_dart](https://pub-web.flutter-io.cn/packages/coordtransform_dart) for coordinate conversion

## FAQ

Q1. setState() or markNeedsBuild() called during build

> <https://fluttercorner.com/setstate-or-markneedsbuild-called-during-build-a-vertical-renderflex-overflowed/>

Solution: Use a callback to delay setState

```dart
WidgetsBinding.instance.addPostFrameCallback((_){
  // your code
});
```

## Miscellaneous

1. pod install is slow

```shell
cd ./ios/
# Uncomment below if using a proxy
# export https_proxy=http://127.0.0.1:7890 http_proxy=http://127.0.0.1:7890 all_proxy=socks5://127.0.0.1:7890
pod install --verbose
```

## Related Projects

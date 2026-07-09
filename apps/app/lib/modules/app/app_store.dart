import 'dart:async';
import 'dart:convert';
import 'dart:io';

import 'package:event_bus/event_bus.dart';
import 'package:flutter/foundation.dart';
import 'package:flutter/material.dart';
import 'package:flutter/scheduler.dart';
import 'package:app/constants.dart';
import 'package:app/core/db/db_manager.dart';
import 'package:app/core/local_storage.dart';
import 'package:logging_appenders/logging_appenders.dart';
import 'package:app/env.dart';
import 'package:app/modules/app/app_setting.dart';
import 'package:app/modules/app/common/task.dart';
import 'package:app/modules/app/db/changelog_v1.dart';
import 'package:app/modules/app/db/changelog_v2.dart';
import 'package:app/modules/app/db/changelog_v3.dart';
// import 'package:app/modules/app/net/my_http_overrides.dart';
import 'package:app/modules/app/store/menu.dart';
import 'package:package_info_plus/package_info_plus.dart';
import 'package:path_provider/path_provider.dart';
import 'package:app/core/log_helper.dart';
import 'package:sqflite/sqflite.dart';

import 'db/changelog_v4.dart';
import 'package:geolocator/geolocator.dart';
// import 'package:geolocator_apple/geolocator_apple.dart';
// import 'package:geolocator_android/geolocator_android.dart';

class AppStore extends ChangeNotifier {
  static late RotatingFileAppender rotatingFileAppender;

  static const itemKeyAppSetting = "itemKeyAppSetting";
  static const itemKeyTtsLocale = "itemKeyTtsLocale";

  bool initValue = false;
  bool initUIValue = false;
  bool _showDatetime = false;
  bool _showOnlineMemo = false;
  Menu menu = Menu.memo;

  static EventBus eventBus = EventBus();
  static var timerCount = 0;
  static List<Task> timerRunningTask = [];

  late LocationSettings locationSettings;
  bool initLocation = false;
  bool isInitWithContext = false;
  Timer? periodicTimer;
  StreamSubscription<Position>? positionStream;

  static late String cachePath;

  static Future<void> initEnv() async {
    FlutterError.onError = (details) {
      FlutterError.presentError(details);
      LogHelper.err(details.toString(), details.stack!);
    };
    // ---
    // move PlatformDispatcher.instance.onError implement to main.dart runZonedGuarded
    // ---
    // PlatformDispatcher.instance.onError = (error, stack) {
    //   eventBus.fire(ErrorToastEvent(e: error));
    //   if (error is ServiceException) {
    //     LogHelper.err(error.serviceMessage, stack);
    //   } else {
    //     LogHelper.err(error.toString(), stack);
    //   }
    //   return true;
    // };
    //init log
    LogHelper.setLevel(Env.config.logLevel);
    if (!kIsWeb) {
      Directory appRootDir = await getApplicationDocumentsDirectory();
      LogHelper.info("appRootDir dir path = ${appRootDir.absolute.path}");
      var packageInfo = await PackageInfo.fromPlatform();
      Directory logDir =
          Directory("${appRootDir.absolute.path}/${packageInfo.appName}/log");
      if (!logDir.existsSync()) {
        logDir.createSync(recursive: true);
        LogHelper.info("log dir create = ${logDir.absolute.path}");
      }
      String logFilePath = "${logDir.path}/app.log";
      rotatingFileAppender = RotatingFileAppender(baseFilePath: logFilePath);
      rotatingFileAppender.attachToLogger(LogHelper.log);
      LogHelper.info("log dir path = ${logDir.absolute.path}");
      LogHelper.info("log file path = $logFilePath");
      //DB init
      Directory dbDir =
          Directory("${appRootDir.absolute.path}/${packageInfo.appName}/db");
      DbManager.instance().init(
          [ChangeLogV1(), ChangeLogV2(), ChangeLogV3(), ChangeLogV4()],
          dbDir.path,
          "app.db");
    } else {
      DbManager.instance().init(
          [ChangeLogV1(), ChangeLogV2(), ChangeLogV3(), ChangeLogV4()],
          "",
          "app.db");
    }
    Database db = await DbManager.instance().open();
    var dbVersion = await db.getVersion();
    LogHelper.info("[DB] current database version is $dbVersion");
    //获取设备编号
    // try {
    //   _deviceId = await PlatformDeviceId.getDeviceId;
    // } on PlatformException {
    //   LogHelper.info("[Platform] Cant't found DeviceId");
    // }
    // LogHelper.info("[Platform] DeviceId=$_deviceId");
    //自签证书配置
    // HttpOverrides.global = MyHttpOverrides();
  }

  void init() {
    if (!initValue) {
      //some init here
      initValue = true;
      eventBus.on().listen((event) {
        LogHelper.info(event.runtimeType.toString());
      });
    }
  }

  // void _initLocationService(BuildContext buildContext) async {
  //   late LocationSettings locationSettings;
  //
  //   if (defaultTargetPlatform == TargetPlatform.android) {
  //     locationSettings = AndroidSettings(
  //         accuracy: LocationAccuracy.high,
  //         distanceFilter: 100,
  //         forceLocationManager: true,
  //         intervalDuration: const Duration(seconds: 10),
  //         //(Optional) Set foreground notification config to keep the app alive
  //         //when going to the background
  //         foregroundNotificationConfig: ForegroundNotificationConfig(
  //           notificationText:
  //               AppLocalizations.of(buildContext)!.runningInBackgroundTip,
  //           notificationTitle:
  //               AppLocalizations.of(buildContext)!.runningInBackground,
  //           enableWakeLock: true,
  //         ));
  //   } else if (defaultTargetPlatform == TargetPlatform.iOS ||
  //       defaultTargetPlatform == TargetPlatform.macOS) {
  //     locationSettings = AppleSettings(
  //       accuracy: LocationAccuracy.high,
  //       activityType: ActivityType.fitness,
  //       distanceFilter: 100,
  //       pauseLocationUpdatesAutomatically: true,
  //       // Only set to true if our app will be started up in the background.
  //       showBackgroundLocationIndicator: false,
  //     );
  //   } else {
  //     locationSettings = LocationSettings(
  //       accuracy: LocationAccuracy.high,
  //       distanceFilter: 100,
  //     );
  //   }
  //
  //   positionStream =
  //       Geolocator.getPositionStream(locationSettings: locationSettings)
  //           .listen((Position? position) {
  //     // LogHelper.info(position == null ? 'Unknown' : '${position.latitude.toString()}, ${position.longitude.toString()}');
  //   });
  // }

  void _initTimerTask(BuildContext buildContext) {
    timerRunningTask.add(Task(
        name: "taskInfo",
        offset: 20000,
        callback: (Task task) {
          LogHelper.info("------task info start------");
          for (Task item in timerRunningTask) {
            LogHelper.info(
                "${item.name},previousExcuteTime=${item.previousExcuteTime},offset=${item.offset},running=${item.running}");
          }
          LogHelper.info("------task info end------");
          task.running = false;
        }));
  }

  void initWithContext(BuildContext buildContext) {
    if (!isInitWithContext) {
      isInitWithContext = true;
      // Initialize Timer Task
      _initTimerTask(buildContext);
      periodicTimer = Timer.periodic(const Duration(seconds: 5), (timer) {
        timerCount += 1;
        LogHelper.info("App Timer count = $timerCount");
        DateTime now = DateTime.now();
        LogHelper.info("now = $now");
        for (Task task in timerRunningTask) {
          if (task.running) {
            continue;
          } else {
            if (task.previousExcuteTime == null ||
                now.isAfter(task.previousExcuteTime!
                    .add(Duration(milliseconds: task.offset)))) {
              task.running = true;
              task.previousExcuteTime = now;
              task.callback(task);
            } else {
              continue;
            }
          }
        }
      });
    }
  }

  void unmountInitWithContent() {
    timerRunningTask.clear();
    periodicTimer?.cancel();
    positionStream?.cancel();
    initLocation = false;
    isInitWithContext = false;
  }

  void initUI(BuildContext context) {
    if (!initUIValue) {
      AppSetting.update(context, getAppSetting());
      initUIValue = true;
    }
  }

  bool get showDatetime => _showDatetime;

  bool get showOnlineMemo => _showOnlineMemo;

  // get deviceId => _deviceId;

  void toggleShowDatetime() {
    _showDatetime = !showDatetime;
    notifyListeners();
  }

  void toggleShowOnlineMemo() {
    _showOnlineMemo = !_showOnlineMemo;
    notifyListeners();
  }

  static AppSetting getDefaultAppSetting() {
    return AppSetting(
        themeMode: ThemeMode.system,
        platform: defaultTargetPlatform,
        textScaleFactorValue: systemTextScaleFactorOption,
        timeDilation: timeDilation,
        localeValue: AppSetting.systemLocaleOption.toString());
  }

  AppSetting getAppSetting() {
    String appSettingString = LocalStorage.get(itemKeyAppSetting, "");
    if (appSettingString.isNotEmpty) {
      return AppSetting.fromJson(jsonDecode(appSettingString));
    } else {
      return getDefaultAppSetting();
    }
  }

  void updateAppSetting(AppSetting appSetting) {
    LocalStorage.save(itemKeyAppSetting, jsonEncode(appSetting));
  }

  static Future<void> saveTtsLocale(String? locale) {
    return LocalStorage.save(itemKeyTtsLocale, locale);
  }

  static String getTtsLocale() {
    return LocalStorage.get(itemKeyTtsLocale, "");
  }

  Menu getMenu() {
    return menu;
  }

  void setMenu(Menu menu) {
    this.menu = menu;
    notifyListeners();
  }
}

class MenuOpenEvent {}

class MemoFilterChangeEvent {
  MemoFilterChangeEvent({this.displaymode});

  int? displaymode;
}

class MemoSearchChangeEvent {
  MemoSearchChangeEvent({this.keyword});

  String? keyword;
}

class ErrorToastEvent {
  ErrorToastEvent({required this.e});

  Object e;
}

class LoginEvent {
  LoginEvent({required this.loginSuccess});

  bool loginSuccess;
}

class NotificationEvent {
  NotificationEvent({required this.name, required this.text});
  String name;
  String text;
}

import 'dart:io';

import 'package:logging/logging.dart';
import 'package:app/modules/auth/login_mode.dart';

class EnvConfig {
  /// APP升级地址
  final String appcastURL;
  Level logLevel;
  final bool loginFeatureEnable;
  final String oauthUrl;
  final String oauthClientId;
  final String oauthClientSecret;
  final String oauthScope;
  final LoginMode loginMode;
  final String authorizationHeader;

  /// 连接超时，单位：milliseconds
  final int connectionTimeout;
  EnvConfig({
    //app更新信息获取地址
    required this.appcastURL,
    //---login feature start
    this.loginFeatureEnable = false,
    this.loginMode = LoginMode.password,
    required this.oauthUrl,
    //LoginMode的oauth2,password必填
    this.oauthClientId = "",
    //LoginMode的oauth2必填
    this.oauthClientSecret = "",
    //LoginMode的oauth2必填
    this.oauthScope = "",
    //---login feature end
    this.logLevel = Level.FINE,
    this.connectionTimeout = 10000,
    this.authorizationHeader = HttpHeaders.authorizationHeader,
  });
}

class Env {
  // 获取当前环境
  static const appEnv = String.fromEnvironment(EnvName.envKey);

  // 开发环境
  static final EnvConfig _devConfig = EnvConfig(
    appcastURL: "https://github.com/lastsunday/vanling/version.xml",
    loginFeatureEnable: true,
    oauthUrl: "",
    oauthClientId: "",
    authorizationHeader: "Authorization",
    //---oauth2 code start
    // oauthClientId: "spring",
    // oauthClientSecret: "secret",
    // oauthScope: "openid",
    //---oauth2 code end
  );
  // 发布环境
  static final EnvConfig _prodConfig = EnvConfig(
    appcastURL: "https://github.com/lastsunday/vanling/version.xml",
    oauthUrl: "",
    logLevel: Level.INFO,
    authorizationHeader: "Authorization",
  );
  // // 测试环境
  // static final EnvConfig _testConfig = EnvConfig(
  //   appcastURL: "",
  // );

  static EnvConfig get config => _getEnvConfig();

  // 根据不同环境返回对应的环境配置
  static EnvConfig _getEnvConfig() {
    switch (appEnv) {
      case EnvName.dev:
        return _devConfig;
      case EnvName.prod:
        return _prodConfig;
      // case EnvName.test:
      //   return _testConfig;
      default:
        return _devConfig;
    }
  }
}

// 声明的环境
abstract class EnvName {
  // 环境key
  static const String envKey = "DART_DEFINE_APP_ENV";
  // 环境value
  static const String dev = "dev";
  static const String prod = "prod";
  // static const String test = "test";
}

import 'package:flutter/material.dart';
import 'package:app/core/connection_provider/connection_provider.dart';
import 'package:app/core/connection_provider/user.dart' as base_user;
import 'package:app/core/domain/global_time.dart';
import 'package:app/core/domain/token.dart';
import 'package:app/core/log_helper.dart';
import 'package:app/env.dart';
import 'package:app/modules/app/net/http.dart';
import 'package:app/modules/auth/login_result.dart';
import 'package:app/modules/auth/login_user_info.dart';

class LoginModel {
  static const _path = "/auth/login";
  static const _pathUserInfo = "/system/user/getInfo";

  static LoginModel _instance = LoginModel._internal();

  LoginModel._internal();

  factory LoginModel() => _instance;

  bool _onAuthTokenRequest = false;

  Future<bool> exchangeToken(String username, String password) async {
    if (_onAuthTokenRequest) return false;
    _onAuthTokenRequest = true;
    Map<String, dynamic> data = {
      // "tenantId": "000000",
      "username": username,
      "password": password,
      "rememberMe": false,
      "clientId": Env.config.oauthClientId,
      "grantType": "password"
    };

    Map<String, String> header = {
      // "isEncrypt": "true",
      //   Env.config.authorizationHeader:
      //       "Basic ${base64.encode(utf8.encode("${Env.config.oauthClientId}:${Env.config.oauthClientSecret}"))}",
    };
    try {
      await ConnectionProvider().addUserByToken(
          await _doRequest(data, header, Env.config.oauthUrl),
          Env.config.oauthUrl, (baseUrl, token) async {
        //Fetch login user info
        var loginUserInfo = await Http.instance()
            .getWithHeader<LoginUserInfoResult>('$baseUrl$_pathUserInfo', {
          Env.config.authorizationHeader: 'Bearer ${token.accessToken}',
          "Clientid": Env.config.oauthClientId,
        }, (data) {
          return LoginUserInfoResult.fromJson(data!);
        });
        var user = base_user.User(
            sub: loginUserInfo.user.userName,
            avatar: loginUserInfo.user.avatar);
        return Future.value(user);
      });
      _onAuthTokenRequest = false;
      return Future.value(true);
    } catch (e, stackTrace) {
      LogHelper.err(e.toString(), stackTrace);
      _onAuthTokenRequest = false;
      return Future.error(e);
    }
  }

  Future<Token> _doRequest(Map<String, dynamic> data,
      Map<String, String> header, String baseUrl) async {
    var loginResult = await Http.instance()
        .postWithConnection<LoginResult>('$baseUrl$_path', (data) {
      return LoginResult.fromJson(data!);
    }, data: data, headers: header);
    var token = Token(loginResult.accessToken, "", loginResult.expireIn, "",
        GlobalTime.now().millisecondsSinceEpoch);
    return Future.value(token);
  }

  @visibleForTesting
  void fullReset() {
    _instance = LoginModel._internal();
  }
}

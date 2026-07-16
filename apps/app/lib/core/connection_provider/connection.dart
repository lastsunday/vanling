import 'dart:convert';

import 'package:app/core/connection_provider/connection_provider.dart';
import 'package:app/core/connection_provider/connections.dart';
import 'package:app/core/connection_provider/user.dart';
import 'package:app/core/domain/token.dart';
import 'package:app/core/log_helper.dart';
import 'package:app/core/net/http_client.dart';
import 'package:app/env.dart';

class Connection {
  User userInfo;
  Token? token;
  bool active;
  BaseUrl baseUrl;

  bool tokenInvalid = false;

  Connection(this.userInfo, this.baseUrl, {required this.active});

  factory Connection.fromJson(Map<String, dynamic> json) {
    var connection = Connection(
      User.fromJson(json['info']),
      BaseUrl(json['base_url']),
      active: json['active'],
    );
    connection.token = json['token'] == null
        ? null
        : Token.fromJson(json['token']);
    return connection;
  }

  bool get shouldRefreshToken => token?.expired ?? false;

  bool get canRefreshToken => token?.refreshToken.isNotEmpty ?? false;

  static const _path = "/oauth2/token";
  static const _grantTypeRefresh = "refresh_token";

  Future<bool> refreshToken() async {
    if (Connections.onAuthTokenRequest) {
      LogHelper.info("Already refreshed, interrupted.");
      return false;
    }
    LogHelper.info("Start refreshing...");
    Connections.onAuthTokenRequest = true;
    try {
      Map<String, dynamic> queryParameters = {
        "client_id": Env.config.oauthClientId,
        "grant_type": _grantTypeRefresh,
        "refresh_token": token!.refreshToken,
      };
      Map<String, String> headers = {
        Env.config.authorizationHeader:
            "Basic ${base64.encode(utf8.encode("${Env.config.oauthClientId}:${Env.config.oauthClientSecret}"))}",
      };
      token = await _doRequest(
        baseUrl.get,
        queryParameters: queryParameters,
        headers: headers,
      );
      return Future.value(true);
    } catch (e) {
      LogHelper.err('RefreshToken error', e);
      return Future.value(false);
    } finally {
      Connections.onAuthTokenRequest = false;
      LogHelper.info("Refresh completed.");
      ConnectionProvider().notifyChanged();
    }
  }

  Future<Token> _doRequest(
    String baseUrl, {
    Object? data,
    Map<String, dynamic>? queryParameters,
    Map<String, String>? headers,
  }) async {
    var response = await HttpClient.instance()
        .postWithConnection<Map<String, dynamic>>(
          '$baseUrl$_path',
          data: data,
          queryParameters: queryParameters,
          headers: headers,
        );
    return Token.fromJson(response.body());
  }

  Map<String, String> get authHeaders {
    if (token != null) {
      return {Env.config.authorizationHeader: 'Bearer ${token!.accessToken}'};
    }
    return {};
  }

  Map<String, dynamic> toJson() {
    return <String, dynamic>{
      "info": userInfo.toJson(),
      "token": token?.toJson(),
      "active": active,
      "base_url": baseUrl.get,
    };
  }

  bool get authorized => token != null;
}

class BaseUrl {
  final String _url;

  BaseUrl(this._url);

  String get get => _url;
}

import 'dart:convert';

import 'package:flutter/material.dart';
import 'package:app/core/connection_provider/connection_provider.dart';
import 'package:app/core/connection_provider/user.dart';
import 'package:app/core/domain/global_time.dart';
import 'package:app/core/domain/token.dart';
import 'package:app/core/log_helper.dart';
import 'package:app/core/net/http_client.dart';
import 'package:app/env.dart';

class Oauth2CodeLoginModel {
  static const _path = "/oauth2/token";
  static const _grantTypeExchange = "authorization_code";
  static const _redirectUri = "/login/oauth2/code";

  static Oauth2CodeLoginModel _instance = Oauth2CodeLoginModel._internal();

  Oauth2CodeLoginModel._internal();

  factory Oauth2CodeLoginModel() => _instance;

  String? _authorizeUrl;
  String? _baseUrl;
  bool _onAuthTokenRequest = false;
  String? redirectUri;

  void configuration({required String host}) {
    _authorizeUrl =
        "$host/oauth2/authorize?client_id=${Env.config.oauthClientId}&"
        "redirect_uri=$_redirectUri&response_type=code&scope=${Env.config.oauthScope}";
    _baseUrl = host;
    redirectUri = _baseUrl! + _redirectUri;
  }

  Future<bool> exchangeToken(String code, String redirectUri) async {
    if (_onAuthTokenRequest) return false;
    _onAuthTokenRequest = true;
    Map<String, dynamic> data = {
      "client_id": Env.config.oauthClientId,
      "grant_type": _grantTypeExchange,
      "redirect_uri": redirectUri,
      "code": code,
    };

    Map<String, String> header = {
      Env.config.authorizationHeader:
          "Basic ${base64.encode(utf8.encode("${Env.config.oauthClientId}:${Env.config.oauthClientSecret}"))}",
    };
    try {
      await ConnectionProvider().addUserByToken(
        await _doRequest(data, header, _baseUrl!),
        _baseUrl!,
        ((baseUrl, token) async {
          var response = await HttpClient.instance()
              .getWithHeader<Map<String, dynamic>>('$baseUrl/userinfo', {
                Env.config.authorizationHeader: 'Bearer ${token.accessToken}',
              });
          var resp = response.body();
          return User.fromJson(resp);
        }),
      );
      _onAuthTokenRequest = false;
      return Future.value(true);
    } catch (e) {
      LogHelper.err('Oauth2CodeLoginModel exchangeToken error', e);
      _onAuthTokenRequest = false;
      return Future.value(false);
    }
  }

  Future<Token> _doRequest(
    Map<String, dynamic> data,
    Map<String, String> header,
    String baseUrl,
  ) async {
    var response = await HttpClient.instance()
        .postWithQueryAndHeader<Map<String, dynamic>>(
          '$baseUrl$_path',
          data,
          header,
        );
    var result = Token.fromJson(response.body());
    result.expiresIn = result.expiresIn * 1000;
    result.createdAt = GlobalTime.now().millisecondsSinceEpoch;
    return result;
  }

  @visibleForTesting
  void fullReset() {
    _instance = Oauth2CodeLoginModel._internal();
  }

  String? get authorizeUrl => _authorizeUrl;

  String? get callbackUrl => _redirectUri;
}

import 'package:dio/dio.dart';

import 'package:app/core/connection_provider/connection.dart';
import 'package:app/core/connection_provider/connection_provider.dart';
import 'package:app/core/log_helper.dart';
// import 'package:app/core/net/http_client_encrypt.dart';
import 'package:app/env.dart';

class RefreshTokenInterceptor extends Interceptor {
  final Dio _dio;

  Connection? connection;

  RefreshTokenInterceptor(this._dio, this.connection);

  @override
  void onRequest(
    RequestOptions options,
    RequestInterceptorHandler handler,
  ) async {
    //encrypt reqeust
    // HttpClientEncrypt.instance().handleEncrypt(options);
    if (connection != null) {
      options.baseUrl = connection!.baseUrl.get;
    } else {
      options.baseUrl = Env.config.oauthUrl;
    }
    if (connection == null) {
      LogHelper.info("onRequest path ${options.method}: ${options.path}");
      handler.next(options);
      return;
    }
    LogHelper.info(
      "onRequest path ${options.method}: ${options.baseUrl}, ${options.path}",
    );
    _addAuthorizationHeader(options);
    _addClientIdHeader(options);
    if (ConnectionProvider.onAuthTokenRequest) {
      handler.next(options);
      return;
    }
    if (connection!.shouldRefreshToken &&
        connection!.canRefreshToken &&
        !options.path.contains("/oauth/token")) {
      LogHelper.info(
        "onRequest refreshToken path ${options.method}: ${options.baseUrl}, ${options.path}",
      );
      if (await connection!.refreshToken()) {
        _addAuthorizationHeader(options);
        _addClientIdHeader(options);
        LogHelper.info("token will expire,auto refresh success");
      } else if (!ConnectionProvider.onAuthTokenRequest) {
        LogHelper.debug(
          "onRequest refreshToken failed path ${options.method}: ${options.baseUrl}, ${options.path}",
        );
        ConnectionProvider().clearActive();
        ConnectionProvider().loggedOut(byUser: false);
      }
    }
    handler.next(options);
  }

  @override
  void onError(DioException err, ErrorInterceptorHandler handler) async {
    var response = err.response;
    LogHelper.err(
      "onError path ${response?.requestOptions.method}: ${response?.requestOptions.baseUrl}, ${response?.requestOptions.path}",
      err,
    );
    if (ConnectionProvider.onAuthTokenRequest) {
      handler.next(err);
      return;
    }
    if (connection != null && response != null && response.statusCode == 401) {
      LogHelper.info(
        "onError refreshToken path ${response.requestOptions.method}: ${response.requestOptions.baseUrl}, ${response.requestOptions.path}",
      );
      if (connection!.canRefreshToken && await connection!.refreshToken()) {
        LogHelper.info("token expired,auto refresh success");
        handler.resolve(await _retry(response.requestOptions));
      } else if (!ConnectionProvider.onAuthTokenRequest) {
        LogHelper.debug(
          "onError refreshToken failed path ${response.requestOptions.method}: ${response.requestOptions.baseUrl}, ${response.requestOptions.path}",
        );
        ConnectionProvider().clearActive();
        ConnectionProvider().loggedOut(byUser: false);
        handler.next(err);
      }
    } else {
      handler.next(err);
    }
  }

  Future<Response> _retry(RequestOptions options) async {
    return await _dio.request(
      options.path,
      data: options.data,
      queryParameters: options.queryParameters,
      options: Options(method: options.method, headers: options.headers),
    );
  }

  void _addAuthorizationHeader(RequestOptions options) {
    options.headers.addAll(connection!.authHeaders);
  }

  void _addClientIdHeader(RequestOptions options) {
    options.headers["Clientid"] = Env.config.oauthClientId;
  }
}

import 'dart:async';

import 'package:app/core/connection_provider/connection_provider.dart';
import 'package:app/core/net/authorized_denied_exception.dart';
import 'package:app/core/net/http_client.dart';
import 'package:app/modules/app/common/response_model.dart';

import '../common/service_exception.dart';

class Http {
  static final Http _instance = Http._internal();

  Http._internal();

  factory Http.instance() => _instance;

  Future<R> get<T, R>(
    String path,
    Map<String, dynamic>? queryParameters,
    T Function(Map<String, dynamic>? json) fromJsonT,
    R Function(List<T>? list, int? total) fromListT,
  ) async {
    var responseResult = await HttpClient.instance().get<Map<String, dynamic>>(
      path,
      queryParameters: queryParameters,
    );
    return _handleResponse(responseResult.body(), fromJsonT, fromListT);
  }

  Future<T> postWithConnection<T>(
    String path,
    T Function(Map<String, dynamic>? json) fromJsonT, {
    Object? data,
    Map<String, dynamic>? queryParameters,
    Map<String, dynamic>? headers,
  }) async {
    var responseResult = await HttpClient.instance()
        .postWithConnection<Map<String, dynamic>>(
          path,
          data: data,
          queryParameters: queryParameters,
          headers: headers,
        );
    return _handleResponse(responseResult.body(), fromJsonT, null);
  }

  Future<T> postJson<T>(
    String path,
    data,
    T Function(Map<String, dynamic>? json) fromJsonT,
  ) async {
    var responseResult = await HttpClient.instance().postJson(path, data);
    return _handleResponse(responseResult.body(), fromJsonT, null);
  }

  Future<T> delete<T>(
    String path,
    T Function(Map<String, dynamic>? json) fromJsonT, {
    Map<String, dynamic>? queryParameters,
  }) async {
    var responseResult = await HttpClient.instance().delete(
      path,
      queryParameters: queryParameters,
    );
    return _handleResponse(responseResult.body(), fromJsonT, null);
  }

  Future<T> putJson<T>(
    String path,
    data,
    T Function(Map<String, dynamic>? json) fromJsonT,
  ) async {
    var responseResult = await HttpClient.instance().putJson(path, data);
    return _handleResponse(responseResult.body(), fromJsonT, null);
  }

  Future<T> getWithHeader<T>(
    String path,
    Map<String, String> header,
    T Function(Map<String, dynamic>? json) fromJsonT,
  ) async {
    var responseResult = await HttpClient.instance()
        .getJsonWithHeader<Map<String, dynamic>>(path, header);
    return _handleResponse(responseResult.body(), fromJsonT, null);
  }

  Future<R> _handleResponse<T, R>(
    Map<String, dynamic> body,
    T Function(Map<String, dynamic>? json) fromJsonT,
    R Function(List<T>? list, int? total)? fromListT,
  ) {
    var response = ResponseModel<T>.fromJson(body, fromJsonT);
    if (response.code == ResponseModel.codeSuccess) {
      if (response.data == null && response.rows == null) {
        return Future.value();
      } else {
        if (fromListT == null) {
          return Future.value(response.data as FutureOr<R>?);
        }
        return Future.value(fromListT(response.rows, response.total));
      }
    } else if (response.code == 401) {
      ConnectionProvider.connection?.tokenInvalid = true;
      ConnectionProvider().notifyChanged();
      return Future.error(AuthorizedDeniedException());
    } else {
      return Future.error(
        ServiceException(response.msg ?? response.code as String),
      );
    }
  }
}

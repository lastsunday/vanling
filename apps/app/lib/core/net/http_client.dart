import 'package:dio/dio.dart';
import 'package:flutter/cupertino.dart';
import 'package:app/core/connection_provider/connection.dart';
import 'package:app/core/connection_provider/connection_provider.dart';
import 'package:app/core/net/authorized_denied_exception.dart';
import 'package:app/core/net/interceptor/refresh_token_interceptor.dart';
import 'package:app/core/net/response.dart' as r;
import 'package:app/env.dart';

class HttpClient {
  static HttpClient _instance = HttpClient._internal();
  static Dio Function() getDio = () => Dio();

  Dio initNewDio(Connection? connection) {
    var dio = getDio();
    dio.interceptors.add(RefreshTokenInterceptor(dio, connection));
    return dio;
  }

  HttpClient._internal();

  factory HttpClient.instance() => _instance;

  Future<r.Response<T>> get<T>(
    String path, {
    Connection? connection,
    bool useActiveConnection = true,
    Map<String, dynamic>? queryParameters,
  }) async {
    return _request(() async {
      var dio = initNewDio(
        connection ??
            (useActiveConnection ? ConnectionProvider.connection : null),
      );
      var response = (await dio.get(path, queryParameters: queryParameters));
      return r.Response(response.data);
    });
  }

  Future<r.Response<T>> getWithHeader<T>(
    String path,
    Map<String, String> header,
  ) async {
    return _request(() async {
      var dio = initNewDio(null);
      var response = (await dio.get(path, options: Options(headers: header)));
      return r.Response(response.data);
    });
  }

  Future<r.Response<T>> getJsonWithHeader<T>(
    String path,
    Map<String, String> header,
  ) async {
    return _request(() async {
      var dio = initNewDio(null);
      var response = (await dio.get(path, options: Options(headers: header)));
      return r.Response(response.data);
    });
  }

  Future<r.Response<T>> getWithConnection<T>(
    String path,
    Connection? connection,
  ) async {
    return _request(() async {
      var dio = initNewDio(connection);
      var response = (await dio.get(path));
      return r.Response(response.data);
    });
  }

  Future<r.Response<T>> post<T>(
    String path,
    data, {
    Connection? connection,
    bool useActiveConnection = true,
  }) async {
    return _request(() async {
      var dio = initNewDio(
        connection ??
            (useActiveConnection ? ConnectionProvider.connection : null),
      );
      var response = await dio.post(path, data: data);
      return r.Response(response.data);
    });
  }

  Future<r.Response<T>> postJson<T>(
    String path,
    data, {
    Connection? connection,
    bool useActiveConnection = true,
  }) async {
    return _request(() async {
      var dio = initNewDio(
        connection ??
            (useActiveConnection ? ConnectionProvider.connection : null),
      );
      dio.options.contentType = "application/json";
      var response = await dio.post(path, data: data);
      return r.Response(response.data);
    });
  }

  Future<r.Response<T>> putJson<T>(
    String path,
    data, {
    Connection? connection,
    bool useActiveConnection = true,
  }) async {
    return _request(() async {
      var dio = initNewDio(
        connection ??
            (useActiveConnection ? ConnectionProvider.connection : null),
      );
      dio.options.contentType = "application/json";
      var response = await dio.put(path, data: data);
      return r.Response(response.data);
    });
  }

  Future<r.Response<T>> postWithHeader<T>(
    String path,
    data,
    Map<String, String> header,
  ) async {
    return _request(() async {
      var dio = initNewDio(null);
      var response = await dio.post(
        path,
        data: data,
        options: Options(
          headers: header,
          receiveTimeout: Duration(milliseconds: Env.config.connectionTimeout),
        ),
      );
      return r.Response(response.data);
    });
  }

  Future<r.Response<T>> postWithQueryAndHeader<T>(
    String path,
    query,
    Map<String, String> header,
  ) async {
    return _request(() async {
      var dio = initNewDio(null);
      var response = await dio.post(
        path,
        queryParameters: query,
        options: Options(
          headers: header,
          receiveTimeout: Duration(milliseconds: Env.config.connectionTimeout),
        ),
      );
      return r.Response(response.data);
    });
  }

  Future<r.Response<T>> postWithConnection<T>(
    String path, {
    Object? data,
    Map<String, dynamic>? queryParameters,
    Map<String, dynamic>? headers,
    Connection? connection,
  }) async {
    return _request(() async {
      var dio = initNewDio(connection);
      var response = await dio.post(
        path,
        queryParameters: queryParameters,
        options: Options(headers: headers),
        data: data,
      );
      return r.Response(response.data);
    });
  }

  Future<r.Response<T>> put<T>(
    String path,
    data, {
    Connection? connection,
    bool useActiveConnection = true,
  }) async {
    return _request(() async {
      var dio = initNewDio(
        connection ??
            (useActiveConnection ? ConnectionProvider.connection : null),
      );
      var response = await dio.put(path, data: data);
      return r.Response(response.data);
    });
  }

  Future<r.Response<T>> delete<T>(
    String path, {
    Connection? connection,
    bool useActiveConnection = true,
    Map<String, dynamic>? queryParameters,
  }) async {
    return _request(() async {
      var dio = initNewDio(
        connection ??
            (useActiveConnection ? ConnectionProvider.connection : null),
      );
      var response = await dio.delete(path, queryParameters: queryParameters);
      return r.Response(response.data);
    });
  }

  Future<r.Response<T>> _request<T>(
    Future<r.Response<T>> Function() function,
  ) async {
    try {
      return await function();
    } on DioException catch (e) {
      if (e.error == 'Http status error [403]') {
        throw AuthorizedDeniedException();
      }
      rethrow;
    }
  }

  @visibleForTesting
  static void injectInstanceForTesting(HttpClient value) {
    _instance = value;
  }

  @visibleForTesting
  void injectDioForTesting(Dio dio) {
    getDio = () => dio;
  }
}

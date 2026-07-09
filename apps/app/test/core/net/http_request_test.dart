import 'package:dio/dio.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:app/core/net/authorized_denied_exception.dart';
import 'package:app/core/net/http_client.dart';
import 'package:app/core/net/response.dart' as r;
import 'package:app/core/connection_provider/connection_provider.dart';
import 'package:mockito/annotations.dart';
import 'package:mockito/mockito.dart';

import 'http_request_test.mocks.dart';

@GenerateMocks([HttpClient])
@GenerateNiceMocks([MockSpec<Dio>()])
void main() {
  late HttpClient instance;
  late MockDio mockDio;

  setUp(() async {
    mockDio = MockDio();
    when(mockDio.interceptors).thenReturn(Interceptors());
    instance = HttpClient.instance();
    instance.injectDioForTesting(mockDio);
  });

  test('Should make a GET request', () async {
    when(mockDio.get<String>("/ping")).thenAnswer((_) => Future(() =>
        Response<String>(
            requestOptions: RequestOptions(path: '/ping'), data: "pong")));
    r.Response<String> response = await instance.get<String>("/ping");
    expect(response.body(), "pong");
  });

  test('Should make a GET request with header.', () async {
    when(mockDio.get<String>("/ping", options: anyNamed('options'))).thenAnswer(
        (_) => Future(() => Response<String>(
            requestOptions: RequestOptions(path: '/ping'), data: "pong")));
    r.Response<String> response =
        await instance.getWithHeader<String>("/ping", {'a': 'b'});
    expect(response.body(), "pong");
  });

  test('Should make a GET request with connection.', () async {
    when(mockDio.get<String>("/ping")).thenAnswer((_) => Future(() =>
        Response<String>(
            requestOptions: RequestOptions(path: '/ping'), data: "pong")));
    r.Response<String> response = await instance.getWithConnection<String>(
        "/ping", ConnectionProvider.connection);
    expect(response.body(), "pong");
  });

  test('Should make a POST request', () async {
    when(mockDio.post<String>("/ping",
            data: {},
            queryParameters: null,
            options: null,
            cancelToken: null,
            onSendProgress: null,
            onReceiveProgress: null))
        .thenAnswer((_) => Future(() => Response<String>(
            requestOptions: RequestOptions(path: '/ping'), data: "pong")));
    r.Response<String> response = await instance.post<String>("/ping", {});
    expect(response.body(), "pong");
  });

  test('Should make a PUT request', () async {
    when(mockDio.put<String>("/ping",
            data: {},
            queryParameters: null,
            options: null,
            cancelToken: null,
            onSendProgress: null,
            onReceiveProgress: null))
        .thenAnswer((_) => Future(() => Response<String>(
            requestOptions: RequestOptions(path: '/ping'), data: "pong")));
    r.Response<String> response = await instance.put<String>("/ping", {});
    expect(response.body(), "pong");
  });

  test('Should make a POST request with some headers', () async {
    when(mockDio.options).thenReturn(BaseOptions());
    when(mockDio.post<String>("/ping",
            data: {},
            queryParameters: null,
            options: anyNamed('options'),
            cancelToken: null,
            onSendProgress: null,
            onReceiveProgress: null))
        .thenAnswer((_) => Future(() => Response<String>(
            requestOptions: RequestOptions(path: '/ping'), data: "pong")));
    r.Response<String> response =
        await instance.postWithHeader<String>("/ping", {}, {});
    expect(response.body(), "pong");
  });

  test('Should make a POST request with connection', () async {
    when(mockDio.options).thenReturn(BaseOptions());
    when(mockDio.post<String>("/ping",
            data: {},
            queryParameters: null,
            options: anyNamed("options"),
            cancelToken: null,
            onSendProgress: null,
            onReceiveProgress: null))
        .thenAnswer((_) => Future(() => Response<String>(
            requestOptions: RequestOptions(path: '/ping'), data: "pong")));
    r.Response<String> response = await instance
        .postWithConnection<String>("/ping", data: {}, connection: null);
    expect(response.body(), "pong");
  });

  test(
      'Should throw AuthorizedDeniedException when received 403 from the GET request',
      () async {
    when(mockDio.get<String>("/ping")).thenThrow(DioException(
        requestOptions: RequestOptions(path: ''),
        response:
            Response(requestOptions: RequestOptions(path: ''), statusCode: 304),
        error: 'Http status error [403]'));
    expect(() async => await instance.get<String>("/ping"),
        throwsA(const TypeMatcher<AuthorizedDeniedException>()));
  });

  test(
      'Should throw AuthorizedDeniedException when received 403 from the POST request',
      () async {
    when(mockDio.post<String>("/ping",
            data: {},
            queryParameters: null,
            options: null,
            cancelToken: null,
            onSendProgress: null,
            onReceiveProgress: null))
        .thenThrow(DioException(
            requestOptions: RequestOptions(path: ''),
            response: Response(
                requestOptions: RequestOptions(path: ''), statusCode: 304),
            error: 'Http status error [403]'));
    expect(() async => await instance.post<String>("/ping", {}),
        throwsA(const TypeMatcher<AuthorizedDeniedException>()));
  });

  test(
      'Should throw AuthorizedDeniedException when received 403 from the PUT request',
      () async {
    when(mockDio.put<String>("/ping",
            data: {},
            queryParameters: null,
            options: null,
            cancelToken: null,
            onSendProgress: null,
            onReceiveProgress: null))
        .thenThrow(DioException(
            requestOptions: RequestOptions(path: ''),
            response: Response(
                requestOptions: RequestOptions(path: ''), statusCode: 304),
            error: 'Http status error [403]'));
    expect(() async => await instance.put<String>("/ping", {}),
        throwsA(const TypeMatcher<AuthorizedDeniedException>()));
  });
}

import 'package:flutter_test/flutter_test.dart';
import 'package:app/core/local_storage.dart';
import 'package:app/core/net/http_client.dart';
import 'package:app/core/net/response.dart';
import 'package:app/core/connection_provider/connection_provider.dart';
import 'package:app/modules/auth/oauth2_code_login_model.dart';
import 'package:mockito/mockito.dart';
import 'package:shared_preferences/shared_preferences.dart';

import '../../core/net/http_request_test.mocks.dart';
import '../../test_data/user.dart';

void main() {
  setUp(() async {
    SharedPreferences.setMockInitialValues(<String, Object>{});
    await LocalStorage.init();
  });

  test('Should create Login Model object with singleton instance', () {
    Oauth2CodeLoginModel model = Oauth2CodeLoginModel();
    expect(model.authorizeUrl, isNull);
    expect(model.callbackUrl, '/login/oauth2/code');
    expect(model, Oauth2CodeLoginModel());
  });

  test('Should exchange token from server', () async {
    final client = MockHttpClient();
    when(
      client.getWithHeader<Map<String, dynamic>>(
        "https://memo.lastsunday.info/userinfo",
        any,
      ),
    ).thenAnswer(
      (_) => Future(() => Response.of<Map<String, dynamic>>(userInfo)),
    );
    when(
      client.postWithQueryAndHeader<Map<String, dynamic>>(
        'https://memo.lastsunday.info/oauth2/token',
        any,
        any,
      ),
    ).thenAnswer(
      (_) => Future(
        () => Response.of<Map<String, dynamic>>({
          'access_token': 'access_token',
          'refresh_token': 'refresh_token',
          'expires_in': 7200,
          'token_type': 'Bearer',
          'created_at': 1672736401,
        }),
      ),
    );
    HttpClient.injectInstanceForTesting(client);

    Oauth2CodeLoginModel model = Oauth2CodeLoginModel();
    model.configuration(host: 'https://memo.lastsunday.info');
    bool result = await model.exchangeToken(
      'code',
      'https://memo.lastsunday.info/login/oauth2/code/spring',
    );
    expect(result, isTrue);
    expect(ConnectionProvider.accessToken, 'access_token');
    expect(ConnectionProvider.refreshToken, 'refresh_token');

    Oauth2CodeLoginModel().fullReset();
  });

  test('Should exchange token be failed when server response error', () async {
    final client = MockHttpClient();
    when(
      client.post<Map<String, dynamic>>(
        'https://memo.lastsunday.info/oauth2/token',
        any,
      ),
    ).thenThrow(Exception());
    HttpClient.injectInstanceForTesting(client);

    Oauth2CodeLoginModel model = Oauth2CodeLoginModel();
    model.configuration(host: 'https://memo.lastsunday.info');
    bool result = await model.exchangeToken(
      'code',
      'https://memo.lastsunday.info/login/oauth2/code/spring',
    );
    expect(result, isFalse);

    Oauth2CodeLoginModel().fullReset();
  });
}

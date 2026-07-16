import 'package:flutter/cupertino.dart';
import 'package:flutter/material.dart';
import 'package:app/core/connection_provider/connection.dart';
import 'package:app/core/connection_provider/connections.dart';
import 'package:app/core/connection_provider/user.dart';
import 'package:app/core/domain/token.dart';
import 'package:app/core/local_storage.dart';
import 'package:app/core/log_helper.dart';

typedef FetchUserByTokenCallback =
    Future<User> Function(String baseUrl, Token token);

class ConnectionProvider extends ChangeNotifier {
  static ConnectionProvider _instance = ConnectionProvider._internal();
  static const String _persistenceLoggedOutKey = "logged-out-by-user-key";
  static final Connections _connections = Connections();

  static bool _isLoggedOutByUser = false;

  ConnectionProvider._internal();

  factory ConnectionProvider() => _instance;

  static String get accessToken => connection?.token?.accessToken ?? '';

  static String get refreshToken => connection?.token?.refreshToken ?? '';

  bool get tokenInvalid => connection?.tokenInvalid ?? false;

  static bool get canRefreshToken =>
      connection?.token?.refreshToken.isNotEmpty ?? false;

  static bool get shouldRefreshToken => connection?.token?.expired ?? false;

  static bool get onAuthTokenRequest => Connections.onAuthTokenRequest;

  Future<Map<String, dynamic>> addUserByToken(
    Token token,
    String baseUrl,
    FetchUserByTokenCallback fetchUserByTokenCallback,
  ) async {
    try {
      User user = await fetchUserByTokenCallback(baseUrl, token);
      var connection = Connection(user, BaseUrl(baseUrl), active: true);
      connection.token = token;
      _connections.add(connection);
      notifyListeners();
      return Future.value({"success": true, "message": ""});
    } catch (e, stackTrace) {
      LogHelper.err(e.toString(), stackTrace);
      notifyListeners();
      return Future.value({"success": false, "message": "$e"});
    }
  }

  static List<Connection> get connections => _connections.get;

  void resetCurrentToken(Token token) {
    _connections.resetCurrentToken(token);
    ConnectionProvider().notifyChanged();
  }

  // Future<Map<String, dynamic>> refresh() async {
  //   try {
  //     for (var connection in _connections.get) {
  //       try {
  //         connection.userInfo = await _fetchUserByToken(
  //             connection.baseUrl.get, connection.token!);
  //       } catch (e) {
  //         debugPrint('Refresh error: $e');
  //       }
  //     }
  //     _connections.save();
  //     notifyListeners();
  //     return Future.value({"success": true, "message": ""});
  //   } catch (error) {
  //     LogHelper.err('UserProvider refresh error', error);
  //     return Future.value({"success": false, "message": "$error"});
  //   }
  // }

  static void notifyTokenChanged(Token token) {
    _connections.notifyTokenChanged(token);
  }

  Future<void> restore() async {
    await _connections.restore();
    bool byUser = await LocalStorage.get(_persistenceLoggedOutKey, false);
    _isLoggedOutByUser = byUser;
    notifyListeners();
  }

  Future<void> clearActive({bool notify = true}) async {
    _connections.clearActive();
    if (notify) {
      notifyListeners();
    }
  }

  void loggedOutSelectedConnection(Connection connection) {
    _connections.loggedOutSelected(connection);
    loggedOut(byUser: true);
    notifyChanged();
  }

  void changeAccount(Connection connection) {
    _connections.changeAccount(connection);
    notifyChanged();
  }

  void notifyChanged() {
    _connections.save();
    notifyListeners();
  }

  void loggedOut({required bool byUser}) {
    _isLoggedOutByUser = byUser;
    LocalStorage.save(_persistenceLoggedOutKey, byUser);
  }

  @visibleForTesting
  void reset(Connection connection) {
    _connections.clearActive();
    _connections.add(connection);
    notifyListeners();
  }

  @visibleForTesting
  void injectConnectionForTest(Connection connection) {
    _connections.add(connection);
    changeAccount(connection);
    notifyListeners();
  }

  @visibleForTesting
  void fullReset() {
    clearActive(notify: false);
    _instance = ConnectionProvider._internal();
  }

  bool _isAuthorized() {
    return ConnectionProvider.accessToken.isNotEmpty &&
        ConnectionProvider.connection?.userInfo.sub != null;
  }

  static BaseUrl get currentBaseUrl => connection?.baseUrl ?? BaseUrl('');

  bool get authorized => _isAuthorized();

  static Connection? get connection => _connections.activeConnection;

  static String? get connectionId => connection?.userInfo.sub;

  static bool get isLoggedOutByUser => _isLoggedOutByUser;
}

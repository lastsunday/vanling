import 'package:app/core/connection_provider/connection.dart';
import 'package:app/core/connection_provider/user.dart';
import 'package:app/core/domain/token.dart';

class Tester {
  static Token token() {
    return Token('access_token', 'token', 7200, 'Bearer', 1669793770);
  }

  static Connection user() {
    var userInfo = User.fromJson({"sub": "user"});
    var user = Connection(
      userInfo,
      BaseUrl('https://memo.lastsunday.info'),
      active: true,
    );
    user.token = token();
    return user;
  }
}

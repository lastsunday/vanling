import 'package:app/core/domain/global_time.dart';

class Token {
  static const int _tokenValidSecondsOffset = 600 * 1000;
  String accessToken;
  String refreshToken;
  int expiresIn;
  String tokenType;
  int createdAt;

  Token(
    this.accessToken,
    this.refreshToken,
    this.expiresIn,
    this.tokenType,
    this.createdAt,
  );

  factory Token.fromJson(Map<String, dynamic> json) => Token(
    json['access_token'] as String,
    json['refresh_token'] as String,
    json['expires_in'] as int,
    json['token_type'] as String,
    json['created_at'] as int,
  );

  Map<String, dynamic> toJson() => <String, dynamic>{
    'access_token': accessToken,
    'refresh_token': refreshToken,
    'expires_in': expiresIn,
    'token_type': tokenType,
    'created_at': createdAt,
  };

  bool get expired {
    var now = GlobalTime.now().millisecondsSinceEpoch;
    return createdAt + expiresIn - _tokenValidSecondsOffset <= now;
  }
}

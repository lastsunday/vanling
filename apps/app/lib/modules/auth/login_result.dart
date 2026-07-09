import 'package:json_annotation/json_annotation.dart';

part 'login_result.g.dart';

@JsonSerializable()
class LoginResult {
  LoginResult(
      {required this.accessToken, required this.expireIn, this.clientId});

  @JsonKey(name: 'access_token')
  String accessToken;
  @JsonKey(name: 'expire_in')
  int expireIn;
  @JsonKey(name: 'client_id')
  String? clientId;
  String? imToken;

  factory LoginResult.fromJson(Map<String, dynamic> json) =>
      _$LoginResultFromJson(json);

  Map<String, dynamic> toJson() => _$LoginResultToJson(this);
}

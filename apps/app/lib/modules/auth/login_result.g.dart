// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'login_result.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

LoginResult _$LoginResultFromJson(Map<String, dynamic> json) => LoginResult(
      accessToken: json['access_token'] as String,
      expireIn: (json['expire_in'] as num).toInt(),
      clientId: json['client_id'] as String?,
    )..imToken = json['imToken'] as String?;

Map<String, dynamic> _$LoginResultToJson(LoginResult instance) =>
    <String, dynamic>{
      'access_token': instance.accessToken,
      'expire_in': instance.expireIn,
      'client_id': instance.clientId,
      'imToken': instance.imToken,
    };

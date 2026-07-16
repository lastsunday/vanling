// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'login_user_info.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

LoginUserInfoResult _$LoginUserInfoResultFromJson(Map<String, dynamic> json) =>
    LoginUserInfoResult(
      user: User.fromJson(json['user'] as Map<String, dynamic>),
    );

Map<String, dynamic> _$LoginUserInfoResultToJson(
  LoginUserInfoResult instance,
) => <String, dynamic>{'user': instance.user};

User _$UserFromJson(Map<String, dynamic> json) =>
    User(userName: json['userName'] as String)
      ..avatar = json['avatar'] as String?;

Map<String, dynamic> _$UserToJson(User instance) => <String, dynamic>{
  'userName': instance.userName,
  'avatar': instance.avatar,
};

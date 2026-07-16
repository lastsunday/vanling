// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'user_vo.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

UserVo _$UserVoFromJson(Map<String, dynamic> json) => UserVo(
  userId: json['userId'],
  userName: json['userName'] as String,
  nickName: json['nickName'] as String,
  email: json['email'] as String?,
  phonenumber: json['phonenumber'] as String?,
  sex: json['sex'] as String?,
  avatar: json['avatar'] as String?,
);

Map<String, dynamic> _$UserVoToJson(UserVo instance) => <String, dynamic>{
  'userId': instance.userId,
  'userName': instance.userName,
  'nickName': instance.nickName,
  'email': instance.email,
  'phonenumber': instance.phonenumber,
  'sex': instance.sex,
  'avatar': instance.avatar,
};

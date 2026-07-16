// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'memo_vo.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

MemoVo _$MemoVoFromJson(Map<String, dynamic> json) => MemoVo(
  id: json['id'] as String,
  content: json['content'] as String,
  displaymode: (json['displaymode'] as num).toInt(),
  createTime: DateTime.parse(json['createTime'] as String),
  updateTime: json['updateTime'] == null
      ? null
      : DateTime.parse(json['updateTime'] as String),
);

Map<String, dynamic> _$MemoVoToJson(MemoVo instance) => <String, dynamic>{
  'id': instance.id,
  'content': instance.content,
  'displaymode': instance.displaymode,
  'createTime': instance.createTime.toIso8601String(),
  'updateTime': instance.updateTime?.toIso8601String(),
};

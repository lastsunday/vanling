// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'memo_bo.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

MemoBo _$MemoBoFromJson(Map<String, dynamic> json) => MemoBo(
  json['id'] as String?,
  json['content'] as String?,
  (json['displaymode'] as num?)?.toInt(),
  (json['displaymodeForDisplayList'] as num?)?.toInt(),
);

Map<String, dynamic> _$MemoBoToJson(MemoBo instance) => <String, dynamic>{
  'id': instance.id,
  'content': instance.content,
  'displaymode': instance.displaymode,
  'displaymodeForDisplayList': instance.displaymodeForDisplayList,
};

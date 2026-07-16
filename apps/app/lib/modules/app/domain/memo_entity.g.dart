// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'memo_entity.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

MemoEntity _$MemoEntityFromJson(Map<String, dynamic> json) => MemoEntity(
  id: json['id'] as String?,
  content: json['content'] as String?,
  datetime: json['datetime'] as String?,
  displaymode: (json['displaymode'] as num?)?.toInt(),
)..updatedatetime = json['updatedatetime'] as String?;

Map<String, dynamic> _$MemoEntityToJson(MemoEntity instance) =>
    <String, dynamic>{
      'id': instance.id,
      'content': instance.content,
      'datetime': instance.datetime,
      'displaymode': instance.displaymode,
      'updatedatetime': instance.updatedatetime,
    };

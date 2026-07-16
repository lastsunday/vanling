// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'memo_model.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

MemoModel _$MemoModelFromJson(Map<String, dynamic> json) => MemoModel(
  id: json['id'] as String?,
  content: json['content'] as String,
  datetime: DateTime.parse(json['datetime'] as String),
  updatedatetime: json['updatedatetime'] == null
      ? null
      : DateTime.parse(json['updatedatetime'] as String),
  displaymode: (json['displaymode'] as num?)?.toInt() ?? 0,
);

Map<String, dynamic> _$MemoModelToJson(MemoModel instance) => <String, dynamic>{
  'id': instance.id,
  'content': instance.content,
  'datetime': instance.datetime.toIso8601String(),
  'displaymode': instance.displaymode,
  'updatedatetime': instance.updatedatetime?.toIso8601String(),
};

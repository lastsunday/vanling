// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'memo_sort_bo.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

MemoSortBo _$MemoSortBoFromJson(Map<String, dynamic> json) => MemoSortBo(
  (json['displaymode'] as num?)?.toInt(),
  Map<String, int>.from(json['idAndSeqMap'] as Map),
  (json['minSeq'] as num).toInt(),
  (json['maxSeq'] as num).toInt(),
);

Map<String, dynamic> _$MemoSortBoToJson(MemoSortBo instance) =>
    <String, dynamic>{
      'displaymode': instance.displaymode,
      'idAndSeqMap': instance.idAndSeqMap,
      'minSeq': instance.minSeq,
      'maxSeq': instance.maxSeq,
    };

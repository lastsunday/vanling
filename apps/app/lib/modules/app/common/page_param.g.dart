// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'page_param.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

PageParam _$PageParamFromJson(Map<String, dynamic> json) => PageParam(
  pageNum: (json['pageNum'] as num?)?.toInt() ?? 1,
  pageSize: (json['pageSize'] as num?)?.toInt() ?? 20,
  orderByColumn: json['orderByColumn'] as String?,
  isAsc: json['isAsc'] as String?,
);

Map<String, dynamic> _$PageParamToJson(PageParam instance) => <String, dynamic>{
  'pageNum': instance.pageNum,
  'pageSize': instance.pageSize,
  'orderByColumn': instance.orderByColumn,
  'isAsc': instance.isAsc,
};

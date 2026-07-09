import 'package:json_annotation/json_annotation.dart';

part 'page_param.g.dart';

@JsonSerializable()
class PageParam {
  int pageNum;
  int pageSize;
  String? orderByColumn;
  String? isAsc;

  PageParam(
      {this.pageNum = 1, this.pageSize = 20, this.orderByColumn, this.isAsc});

  int get limit => pageSize;

  int get offset => (pageNum - 1) * pageSize;

  String get orderBy => " $orderByColumn $isAsc";

  factory PageParam.fromJson(Map<String, dynamic> json) =>
      _$PageParamFromJson(json);

  Map<String, dynamic> toJson() => _$PageParamToJson(this);
}

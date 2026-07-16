// import 'package:json_annotation/json_annotation.dart';

// part 'response_model.g.dart';

// @JsonSerializable(
//     genericArgumentFactories: true, fieldRename: FieldRename.snake)
class ResponseModel<T> {
  static const codeSuccess = 200;

  ResponseModel({
    required this.code,
    this.msg,
    this.data,
    this.rows,
    this.total,
  });

  final int code;
  final String? msg;
  final T? data;
  final List<T>? rows;
  final int? total;

  factory ResponseModel.fromJson(
    Map<String, dynamic> json,
    T Function(Map<String, dynamic>? json) fromJsonT,
  ) => _$ResponseModelFromJson(json, fromJsonT);

  Map<String, dynamic> toJson(Object Function(T value) toJsonT) =>
      _$ResponseModelToJson(this, toJsonT);
}

ResponseModel<T> _$ResponseModelFromJson<T>(
  Map<String, dynamic> json,
  T Function(Map<String, dynamic>? json) fromJsonT,
) {
  List<T> rowsTarget = [];
  if (json.containsKey('rows')) {
    List<dynamic> rows = json['rows'];
    for (var element in rows) {
      rowsTarget.add(_$nullableGenericFromJson(element, fromJsonT) as T);
    }
  } else if (json['data'] is List) {
    List<T> rowsTarget = [];
    List<dynamic> rows = json['data'];
    for (var element in rows) {
      rowsTarget.add(_$nullableGenericFromJson(element, fromJsonT) as T);
    }
    return ResponseModel<T>(
      code: json['code'] as int,
      msg: json['msg'] as String?,
      data: null,
      rows: rowsTarget,
      total: rowsTarget.length,
    );
  }
  return ResponseModel<T>(
    code: json['code'] as int,
    msg: json['msg'] as String?,
    data: _$nullableGenericFromJson(json['data'], fromJsonT),
    rows: rowsTarget,
    total: json['total'] as int?,
  );
}

Map<String, dynamic> _$ResponseModelToJson<T>(
  ResponseModel<T> instance,
  Object? Function(T value) toJsonT,
) => <String, dynamic>{
  'code': instance.code,
  'msg': instance.msg,
  'data': _$nullableGenericToJson(instance.data, toJsonT),
};

T? _$nullableGenericFromJson<T>(
  Map<String, dynamic>? input,
  T Function(Map<String, dynamic>? json) fromJson,
) => input == null ? null : fromJson(input);

Object? _$nullableGenericToJson<T>(
  T? input,
  Object? Function(T value) toJson,
) => input == null ? null : toJson(input);

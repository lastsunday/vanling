import 'package:json_annotation/json_annotation.dart';

part 'memo_bo.g.dart';

@JsonSerializable()
class MemoBo {
  MemoBo(
    this.id,
    this.content,
    this.displaymode,
    this.displaymodeForDisplayList,
  );

  String? id;
  String? content = "";
  int? displaymode = 0;
  int? displaymodeForDisplayList;

  factory MemoBo.fromJson(Map<String, dynamic> json) => _$MemoBoFromJson(json);

  Map<String, dynamic> toJson() => _$MemoBoToJson(this);
}

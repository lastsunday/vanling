import 'package:json_annotation/json_annotation.dart';

part 'memo_vo.g.dart';

@JsonSerializable()
class MemoVo {
  MemoVo({
    required this.id,
    required this.content,
    required this.displaymode,
    required this.createTime,
    this.updateTime,
  });

  String id;
  String content;
  int displaymode;
  DateTime createTime;
  DateTime? updateTime;

  factory MemoVo.fromJson(Map<String, dynamic> json) => _$MemoVoFromJson(json);

  Map<String, dynamic> toJson() => _$MemoVoToJson(this);
}

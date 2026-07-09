import 'package:json_annotation/json_annotation.dart';

part 'memo_model.g.dart';

@JsonSerializable()
class MemoModel {
  MemoModel(
      {this.id,
      required this.content,
      required this.datetime,
      this.updatedatetime,
      this.displaymode = 0});

  String? id;
  String content = "";
  DateTime datetime = DateTime.now();
  int displaymode = 0;
  DateTime? updatedatetime;

  factory MemoModel.fromJson(Map<String, dynamic> json) =>
      _$MemoModelFromJson(json);

  Map<String, dynamic> toJson() => _$MemoModelToJson(this);
}

enum DisplayMode {
  auto(0),
  text(1),
  image(2);

  const DisplayMode(this.value);

  final int value;

  static DisplayMode getType(int value) =>
      DisplayMode.values.firstWhere((element) => element.value == value);
}

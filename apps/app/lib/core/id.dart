class Id {
  final int _id;

  Id._(this._id);

  factory Id.fromGid(String gid) {
    return Id._(int.parse(gid.split("/").last));
  }

  factory Id.from(int id) {
    return Id._(id);
  }

  int get id => _id;

  @override
  bool operator ==(Object other) =>
      identical(this, other) ||
      other is Id && runtimeType == other.runtimeType && _id == other._id;

  @override
  int get hashCode => _id.hashCode;
}

import 'package:flutter/foundation.dart';
import 'package:app/core/db/db_changelog.dart';
import 'package:app/core/log_helper.dart';

import 'package:path/path.dart';
import 'package:sqflite_common_ffi/sqflite_ffi.dart';
import 'package:sqflite_common_ffi_web/sqflite_ffi_web.dart';

class DbManager {
  static final DbManager _instance = DbManager._internal();

  String? _name;
  String? _databasesPath;

  Database? _db;

  DbManager._internal();

  factory DbManager.instance() => _instance;

  List<Changelog> changelog() => DbChangelog.getChangelogList();

  void init(List<Changelog> changelogList, String databasesPath, String name) {
    changelog().addAll(changelogList);
    _name = name;
    _databasesPath = databasesPath;
  }

  Future<Database> open() async {
    if (_name == null || _databasesPath == null) {
      throw Exception("please init database path and name,use init method");
    }
    DatabaseFactory databaseFactory;
    String dbFilePath;
    if (!kIsWeb) {
      sqfliteFfiInit();
      databaseFactory = databaseFactoryFfi;
      dbFilePath = join(_databasesPath!, _name);
    } else {
      databaseFactory = databaseFactoryFfiWeb;
      dbFilePath = _name!;
    }
    LogHelper.info("databasesPath path = $_databasesPath");
    var options = OpenDatabaseOptions(
        version: changelog().length,
        onCreate: _onCreate,
        onUpgrade: _onUpgrade);
    return await databaseFactory.openDatabase(dbFilePath, options: options);
  }

  Future<Database> getDb() async {
    _db ??= await open();
    return Future.value(_db);
  }

  Future<void>? close() {
    return _db?.close();
  }

  void _onCreate(Database db, int version) {
    LogHelper.info('[DB] onCreate version: $version');
    _onUpgrade(db, 0, version);
  }

  void _onUpgrade(Database db, int oldVersion, int newVersion) {
    LogHelper.info(
        '[DB] onUpgrade:oldVersion -> $oldVersion, newVersion -> $newVersion');
    List<Changelog> changelogList = DbChangelog.getChangelogList();
    LogHelper.info("changelogList length = ${changelogList.length}");
    db.transaction((txn) async {
      LogHelper.info("[DB] transaction execute");
      for (int i = oldVersion; i < newVersion; i++) {
        Changelog changelog = changelogList[i];
        List<String> sqlList = changelog.getSqlList();
        for (int n = 0; n < sqlList.length; n++) {
          String sql = sqlList[n];
          LogHelper.info("[DB] execute sql = $sql");
          await db.execute(sql);
        }
      }
    });
  }

  Future<int> insert(String table, Map<String, Object?> values,
      {String? nullColumnHack, ConflictAlgorithm? conflictAlgorithm}) async {
    var db = await getDb();
    return db.insert(table, values,
        nullColumnHack: nullColumnHack, conflictAlgorithm: conflictAlgorithm);
  }

  Future<List<int>> batchInsert(
      String tableName, List<Map<String, dynamic>> values,
      {String? nullColumnHack, ConflictAlgorithm? conflictAlgorithm}) async {
    var db = await getDb();
    return db.transaction((txn) async {
      List<int> result = [];
      for (var element in values) {
        int id = await txn.insert(tableName, element,
            nullColumnHack: nullColumnHack,
            conflictAlgorithm: conflictAlgorithm);
        result.add(id);
      }
      return result;
    });
  }

  Future<int> batchUpdate(String tableName, List<Map<String, dynamic>> values,
      {String? nullColumnHack, ConflictAlgorithm? conflictAlgorithm}) async {
    var db = await getDb();
    return await db.transaction<int>((txn) async {
      int result = 0;
      for (var value in values) {
        int count = await txn.update(tableName, value,
            where: " id = ? ", whereArgs: [value['id']]);
        result += count;
      }
      return result;
    });
  }

  Future<Map<String, dynamic>?> findOne(String tableName,
      {bool? distinct,
      List<String>? columns,
      String? where,
      List<Object?>? whereArgs}) async {
    var db = await getDb();
    var list = await db.query(tableName,
        distinct: distinct,
        columns: columns,
        where: where,
        whereArgs: whereArgs,
        limit: 1);
    return list.isNotEmpty ? list[0] : null;
  }

  Future<List<Map<String, dynamic>>> find(String tableName,
      {bool? distinct,
      List<String>? columns,
      String? where,
      List<Object?>? whereArgs,
      String? groupBy,
      String? having,
      String? orderBy,
      int? limit,
      int? offset}) async {
    var db = await getDb();
    return await db.query(tableName,
        distinct: distinct,
        columns: columns,
        where: where,
        whereArgs: whereArgs,
        groupBy: groupBy,
        having: having,
        orderBy: orderBy,
        limit: limit,
        offset: offset);
  }

  Future<List<Map<String, dynamic>>> rawFind(String sql) async {
    var db = await getDb();
    return await db.rawQuery(sql);
  }

  Future<int> delete(String tableName,
      {String? where, List<Object?>? whereArgs}) async {
    return (await getDb())
        .delete(tableName, where: where, whereArgs: whereArgs);
  }

  Future<int> rawDelete(String sql) async {
    var db = await getDb();
    return await db.rawDelete(sql);
  }

  Future<int> update(String tableName, Map<String, Object?> values,
      {String? where,
      List<Object?>? whereArgs,
      ConflictAlgorithm? conflictAlgorithm}) async {
    return (await getDb()).update(tableName, values,
        where: where,
        whereArgs: whereArgs,
        conflictAlgorithm: conflictAlgorithm);
  }

  Future<int> rawUpdate(String sql) async {
    return (await getDb()).rawUpdate(sql);
  }

  Future<int> count(String tableName,
      {String? where, List<Object?>? whereArgs}) async {
    List<Map<String, Object?>> query = await (await getDb())
        .query(tableName, columns: ['id'], where: where, whereArgs: whereArgs);
    return Future(() => query.length);
  }

  @visibleForTesting
  void injectDatabaseForTesting(Database mockDatabase) {
    _db = mockDatabase;
  }
}

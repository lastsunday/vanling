import 'dart:io';

import 'package:flutter_test/flutter_test.dart';
import 'package:app/core/db/db_changelog.dart';
import 'package:app/core/db/db_manager.dart';
import 'package:sqflite_common_ffi/sqflite_ffi.dart';

void main() {
  test('Should create database with changelog', () async {
    if (Platform.isWindows || Platform.isLinux) {
      // Initialize FFI
      sqfliteFfiInit();
    }
    // Change the default factory. On iOS/Android, if not using `sqlite_flutter_lib` you can forget
    // this step, it will use the sqlite version available on the system.
    databaseFactory = databaseFactoryFfi;
    final dbPath = Directory.systemTemp.createTempSync('chobits_test_');
    DbManager.instance().init([ChangelogV1()], dbPath.path, 'test.db');
    await DbManager.instance().open();
    var result = await DbManager.instance()
        .findOne("memo", where: "content = ?", whereArgs: ["test"]);
    expect(result!["content"], "test");
    await DbManager.instance().close();
    dbPath.deleteSync(recursive: true);
  });
}

class ChangelogV1 implements Changelog {
  @override
  List<String> getSqlList() {
    return [
      """
      CREATE TABLE memo(
          id INTEGER PRIMARY KEY AUTOINCREMENT,
          content TEXT,
          datetime TEXT
        )
    """,
      """
      INSERT INTO memo(content,datetime) VALUES('test','2023-06-30 19:00:00')
    """
    ];
  }
}

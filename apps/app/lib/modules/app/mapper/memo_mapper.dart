import 'package:app/core/db/db_manager.dart';
import 'package:app/core/log_helper.dart';
import 'package:app/modules/app/common/page_param.dart';
import 'package:app/modules/app/common/page_result.dart';
import 'package:app/modules/app/domain/memo_entity.dart';
import 'package:app/modules/app/domain/memo_order_entity.dart';

class MemoMapper {
  static Future<List<MemoEntity>> selectAll() async {
    List<Map<String, dynamic>> findResult = await DbManager.instance().find(
      MemoEntity.tableName,
      orderBy: " datetime desc",
    );
    return Future.value(findResult.map((e) => MemoEntity.fromJson(e)).toList());
  }

  static Future<void> save(MemoEntity entity) async {
    int result = await DbManager.instance().insert(
      MemoEntity.tableName,
      entity.toJson(),
    );
    if (result <= 0) {
      throw Exception("save memo failure");
    }
    return Future.value();
  }

  static Future<void> saveToTop(MemoEntity entity, int? displaymode) async {
    var db = await DbManager.instance().getDb();
    return db.transaction((txn) async {
      LogHelper.info("[DB] transaction execute");
      LogHelper.info("[DB] update memo order");
      String sqlWhereForDisplayMode;
      if (displaymode == null) {
        sqlWhereForDisplayMode = " displaymode is null ";
      } else {
        sqlWhereForDisplayMode = " displaymode = $displaymode ";
      }
      var updateOtherSeqItemSql =
          "UPDATE memo_order SET seq = seq + 1 WHERE $sqlWhereForDisplayMode";
      LogHelper.info("[DB] update sql = $updateOtherSeqItemSql");
      await txn.rawUpdate(updateOtherSeqItemSql);
      LogHelper.info("[DB] insert memo = $entity");
      int id = await txn.insert(MemoEntity.tableName, entity.toJson());
      LogHelper.info("[DB] query entity by rowid = $id");
      var entityMap = await txn.query(
        MemoEntity.tableName,
        where: "rowid = ?",
        whereArgs: [id],
      );
      var entityResult = MemoEntity.fromJson(entityMap.first);
      var insertMemoOrderSql =
          "INSERT INTO memo_order(displaymode,id,seq) VALUES($displaymode,'${entityResult.id}',0)";
      LogHelper.info("[DB] insert sql = $insertMemoOrderSql");
      await txn.rawInsert(insertMemoOrderSql);
    });
  }

  static Future<void> update(MemoEntity entity) async {
    int result = await DbManager.instance().update(
      MemoEntity.tableName,
      entity.toJson(),
      where: " id = ? ",
      whereArgs: [entity.id],
    );
    if (result <= 0) {
      throw Exception("update memo failure");
    }
    return Future.value();
  }

  static Future<void> delete(MemoEntity entity) async {
    var db = await DbManager.instance().getDb();
    return db.transaction((txn) async {
      LogHelper.info("[DB] transaction execute");
      var entityMap = await txn.query(
        MemoOrderEntity.tableName,
        where: "id = ?",
        whereArgs: [entity.id],
      );
      for (var element in entityMap) {
        var entityResult = MemoOrderEntity.fromJson(element);
        LogHelper.info(
          "[DB] update memo order displaymode = ${entityResult.displaymode}",
        );
        String sqlWhereForDisplayMode;
        if (entityResult.displaymode == null) {
          sqlWhereForDisplayMode = " displaymode is null ";
        } else {
          sqlWhereForDisplayMode =
              " displaymode = ${entityResult.displaymode} ";
        }
        var updateOtherSeqItemSql =
            "UPDATE memo_order SET seq = seq - 1 WHERE $sqlWhereForDisplayMode and seq > ${entityResult.seq}";
        LogHelper.info("[DB] update sql = $updateOtherSeqItemSql");
        await txn.rawUpdate(updateOtherSeqItemSql);
      }
      LogHelper.info("[DB] delete memo order id = ${entity.id}");
      await txn.delete(
        MemoOrderEntity.tableName,
        where: " id = ? ",
        whereArgs: [entity.id],
      );
      LogHelper.info("[DB] delete memo id = ${entity.id}");
      int result = await txn.delete(
        MemoEntity.tableName,
        where: " id = ? ",
        whereArgs: [entity.id],
      );
      if (result <= 0) {
        throw Exception("delete memo failure");
      }
    });
  }

  static Future<int> count({int? displaymode, String? content}) async {
    String where = "";
    List<Object> whereArgs = [];
    if (displaymode != null) {
      where += "displaymode = ?";
      whereArgs.add(displaymode);
    }
    if (content != null && content.isNotEmpty) {
      where += "content like ?";
      whereArgs.add("%$content%");
    }
    return DbManager.instance().count(
      MemoEntity.tableName,
      where: where.isNotEmpty ? where : null,
      whereArgs: whereArgs.isNotEmpty ? whereArgs : null,
    );
  }

  static Future<PageResult<MemoEntity>> page(
    PageParam param, {
    int? displaymode,
    String? content,
  }) async {
    int total = await count(displaymode: displaymode, content: content);
    String where = "";
    if (displaymode != null) {
      where += "t1.displaymode = $displaymode";
    }
    if (content != null && content.isNotEmpty) {
      where += "t1.content like '%$content%'";
    }
    if (where.isNotEmpty) {
      where = " where $where";
    }
    String sqlWhereForDisplayMode;
    if (displaymode == null) {
      sqlWhereForDisplayMode = " t2.displaymode is null ";
    } else {
      sqlWhereForDisplayMode = " t2.displaymode = $displaymode ";
    }
    String sql =
        "SELECT t1.id,t1.content,t1.datetime,t1.displaymode,t1.updatedatetime FROM memo t1 LEFT JOIN memo_order t2 on t1.id = t2.id and $sqlWhereForDisplayMode $where ORDER BY t2.seq asc NULLS LAST,${param.orderBy} LIMIT ${param.offset},${param.limit}";
    LogHelper.info("[DB] select page sql = $sql");
    List<Map<String, dynamic>> result = await DbManager.instance().rawFind(sql);
    List<MemoEntity> rows = result.map((e) => MemoEntity.fromJson(e)).toList();
    return Future.value(
      PageResult<MemoEntity>(param: param, total: total, rows: rows),
    );
  }

  static Future<void> sort(
    int? displaymode,
    Map<String, int> idAndSeqMap,
    int minSeq,
    int maxSeq,
  ) async {
    var db = await DbManager.instance().getDb();
    return db.transaction((txn) async {
      LogHelper.info("[DB] transaction execute");
      String sqlWhereForDisplayMode;
      if (displaymode == null) {
        sqlWhereForDisplayMode = " displaymode is null ";
      } else {
        sqlWhereForDisplayMode = " displaymode = $displaymode ";
      }
      var deleteSql =
          "DELETE FROM memo_order where $sqlWhereForDisplayMode and seq >= $minSeq and seq <= $maxSeq";
      LogHelper.info("[DB] delete sql = $deleteSql");
      await txn.rawDelete(deleteSql);
      for (var id in idAndSeqMap.keys) {
        var insertSql =
            "INSERT INTO memo_order(displaymode,id,seq) VALUES($displaymode,'$id',${idAndSeqMap[id]})";
        LogHelper.info("[DB] insert sql = $insertSql");
        await txn.rawInsert(insertSql);
      }
    });
  }
}

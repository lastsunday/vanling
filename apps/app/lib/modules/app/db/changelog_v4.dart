import 'package:app/core/db/db_changelog.dart';

class ChangeLogV4 implements Changelog {
  @override
  List<String> getSqlList() {
    String sqlAlterTableAddColumnForUpdateDatetimeMemo = """
      ALTER TABLE memo ADD COLUMN updatedatetime TEXT;
      """;
    return [sqlAlterTableAddColumnForUpdateDatetimeMemo];
  }
}

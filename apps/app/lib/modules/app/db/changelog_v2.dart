import 'package:app/core/db/db_changelog.dart';

class ChangeLogV2 implements Changelog {
  @override
  List<String> getSqlList() {
    //memo column displaymode,0=auto,1=text,2=image
    String sqlAlterTableAddColumnMemo = """
      ALTER TABLE memo ADD COLUMN displaymode INTEGER DEFAULT 0;
      """;
    return [sqlAlterTableAddColumnMemo];
  }
}

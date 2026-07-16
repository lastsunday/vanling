import 'package:flutter/foundation.dart';
import 'package:app/modules/app/common/page_param.dart';
import 'package:app/modules/app/common/page_result.dart';
import 'package:app/modules/app/model/memo_model.dart';

abstract class MemoStore extends ChangeNotifier {
  int memoTotal = 0;

  Future<PageResult<MemoModel>> pageMemo(
    PageParam param, {
    int? displaymode,
    String? keyword,
  });

  Future<void> addMemo(MemoModel memo);

  Future<void> addMemoToTop(MemoModel memo, int? displaymode);

  Future<void> updateMemo(MemoModel memo);

  Future<void> deleteMemo(MemoModel memo);

  Future<void> sortMemo(
    int? displaymode,
    Map<String, int> idAndSeqMap,
    int minSeq,
    int maxSeq,
  );
}

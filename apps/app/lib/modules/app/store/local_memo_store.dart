import 'package:app/core/log_helper.dart';
import 'package:app/core/util/id_util.dart';
import 'package:app/modules/app/common/page_param.dart';

import 'package:app/modules/app/common/page_result.dart';
import 'package:app/modules/app/domain/memo_entity.dart';
import 'package:app/modules/app/mapper/memo_mapper.dart';

import 'package:app/modules/app/model/memo_model.dart';

import 'memo_store.dart';

class LocalMemoStore extends MemoStore {
  @override
  Future<PageResult<MemoModel>> pageMemo(
    PageParam param, {
    int? displaymode,
    String? keyword,
  }) async {
    PageResult<MemoEntity> result = await MemoMapper.page(
      param,
      displaymode: displaymode,
      content: keyword,
    );
    memoTotal = result.total;
    notifyListeners();
    return Future(
      () => PageResult(
        param: param,
        rows: result.rows.map((e) => MemoModel.fromJson(e.toJson())).toList(),
        total: result.total,
      ),
    );
  }

  @override
  Future<void> addMemo(MemoModel memo) async {
    LogHelper.info("[Memo] add memo ${memo.toJson()}");
    await _saveMemosToStorage(memo);
    notifyListeners();
    return Future.value(null);
  }

  @override
  Future<void> addMemoToTop(MemoModel memo, int? displaymode) async {
    LogHelper.info("[Memo] add memo displaymode=$displaymode,${memo.toJson()}");
    var param = MemoModel.fromJson(memo.toJson());
    param.id = IdUtil.genUUID();
    await MemoMapper.saveToTop(
      MemoEntity.fromJson(param.toJson()),
      displaymode,
    );
    notifyListeners();
    return Future.value(null);
  }

  @override
  Future<void> updateMemo(MemoModel memo) async {
    LogHelper.info("[Memo] update memo ${memo.toJson()}");
    await _saveMemosToStorage(memo);
    notifyListeners();
    return Future.value(null);
  }

  @override
  Future<void> deleteMemo(MemoModel memo) async {
    LogHelper.info("[Memo] delete memo ${memo.toJson()}");
    await MemoMapper.delete(MemoEntity.fromJson(memo.toJson()));
    notifyListeners();
    return Future.value(null);
  }

  @override
  Future<void> sortMemo(
    int? displaymode,
    Map<String, int> idAndSeqMap,
    int minSeq,
    int maxSeq,
  ) {
    LogHelper.info("[Memo] sort memo}");
    return MemoMapper.sort(displaymode, idAndSeqMap, minSeq, maxSeq);
  }

  Future<void> _saveMemosToStorage(MemoModel memo) {
    if (memo.id == null) {
      var param = MemoModel.fromJson(memo.toJson());
      param.id = IdUtil.genUUID();
      return MemoMapper.save(MemoEntity.fromJson(param.toJson()));
    } else {
      return MemoMapper.update(MemoEntity.fromJson(memo.toJson()));
    }
  }
}

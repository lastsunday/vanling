import 'package:app/modules/app/common/page_param.dart';
import 'package:app/modules/app/common/page_result.dart';
import 'package:app/modules/app/model/memo_model.dart';
import 'package:app/modules/app/net/api/bo/memo_bo.dart';
import 'package:app/modules/app/net/api/bo/memo_search_bo.dart';
import 'package:app/modules/app/net/api/bo/memo_sort_bo.dart';
import 'package:app/modules/app/net/api/memo_api.dart';
import 'package:app/modules/app/net/api/vo/memo_vo.dart';
import 'package:app/modules/app/store/memo_store.dart';

class OnlineMemoStore extends MemoStore {
  @override
  Future<PageResult<MemoModel>> pageMemo(
    PageParam param, {
    int? displaymode,
    String? keyword,
  }) async {
    PageResult<MemoVo> result;
    if (keyword == null || keyword.isEmpty) {
      var bo = MemoBo(null, null, null, displaymode);
      result = await MemoApi.instance().list(bo, param);
    } else {
      var bo = MemoSearchBo(keyword, displaymode);
      result = await MemoApi.instance().search(bo, param);
    }
    memoTotal = result.total;
    notifyListeners();
    return Future(
      () => PageResult(
        param: param,
        rows: result.rows
            .map(
              (e) => MemoModel(
                content: e.content,
                datetime: e.createTime,
                id: e.id,
                displaymode: e.displaymode,
                updatedatetime: e.updateTime,
              ),
            )
            .toList(),
        total: result.total,
      ),
    );
  }

  @override
  Future<void> addMemo(MemoModel memo) {
    // TODO: implement addMemoToTop
    throw UnimplementedError();
  }

  @override
  Future<void> addMemoToTop(MemoModel memo, int? displaymode) {
    MemoBo memoBo = MemoBo(
      memo.id,
      memo.content,
      memo.displaymode,
      displaymode,
    );
    return MemoApi.instance().insertToTop(memoBo);
  }

  @override
  Future<void> deleteMemo(MemoModel memo) {
    return MemoApi.instance().delete(memo.id!);
  }

  @override
  Future<void> sortMemo(
    int? displaymode,
    Map<String, int> idAndSeqMap,
    int minSeq,
    int maxSeq,
  ) {
    MemoSortBo bo = MemoSortBo(displaymode, idAndSeqMap, minSeq, maxSeq);
    return MemoApi.instance().sort(bo);
  }

  @override
  Future<void> updateMemo(MemoModel memo) {
    MemoBo memoBo = MemoBo(memo.id, memo.content, memo.displaymode, null);
    return MemoApi.instance().update(memoBo);
  }
}

import 'package:app/modules/app/common/page_param.dart';
import 'package:app/modules/app/common/page_result.dart';
import 'package:app/modules/app/net/api/bo/memo_bo.dart';
import 'package:app/modules/app/net/api/bo/memo_sort_bo.dart';
import 'package:app/modules/app/net/api/vo/memo_vo.dart';
import 'package:app/modules/app/net/http.dart';

import 'bo/memo_search_bo.dart';

class MemoApi {
  final urlPathList = "/memo/memo/listWithOrder";
  final urlPathSearch = "/memo/memo/search";
  final urlPathMemo = "/memo/memo";
  final urlPathMemoAddToTop = "/memo/memo/addToTop";
  final urlPathDeleteMemoWithOrder = "/memo/memo/deleteWithOrder";
  final urlPathSortMemo = "/memo/memo/sort";

  static final MemoApi _instance = MemoApi._internal();

  MemoApi._internal();

  factory MemoApi.instance() => _instance;

  Future<PageResult<MemoVo>> list(MemoBo? bo, PageParam page) async {
    var result = <String, dynamic>{};
    PageParam target = PageParam.fromJson(page.toJson());
    if (target.orderByColumn == "datetime") {
      target.orderByColumn = "create_time";
    }
    result.addAll(target.toJson());
    if (bo != null) {
      result.addAll(bo.toJson());
    }
    return await Http.instance().get<MemoVo, PageResult<MemoVo>>(
      urlPathList,
      result,
      (json) {
        return MemoVo.fromJson(json!);
      },
      (list, total) {
        return PageResult(param: target, total: total!, rows: list!);
      },
    );
  }

  Future<void> insertToTop(MemoBo bo) async {
    return await Http.instance().postJson<void>(urlPathMemoAddToTop, bo, (
      json,
    ) {
      return;
    });
  }

  Future<void> delete(String id) async {
    return await Http.instance().delete<void>(
      "$urlPathDeleteMemoWithOrder/$id",
      (json) {
        return;
      },
    );
  }

  Future<void> update(MemoBo bo) async {
    return await Http.instance().putJson<void>(urlPathMemo, bo, (json) {
      return;
    });
  }

  Future<void> sort(MemoSortBo bo) async {
    return await Http.instance().postJson<void>(urlPathSortMemo, bo, (json) {
      return;
    });
  }

  Future<PageResult<MemoVo>> search(MemoSearchBo? bo, PageParam page) async {
    var result = <String, dynamic>{};
    PageParam target = PageParam.fromJson(page.toJson());
    if (target.orderByColumn == "datetime") {
      target.orderByColumn = "create_time";
    }
    result.addAll(target.toJson());
    if (bo != null) {
      result.addAll(bo.toJson());
    }
    return await Http.instance().get<MemoVo, PageResult<MemoVo>>(
      urlPathSearch,
      result,
      (json) {
        return MemoVo.fromJson(json!);
      },
      (list, total) {
        return PageResult(param: target, total: total!, rows: list!);
      },
    );
  }
}

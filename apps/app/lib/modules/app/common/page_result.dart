import 'package:app/modules/app/common/page_param.dart';

class PageResult<T> {
  PageParam param;
  int total;
  List<T> rows;

  PageResult({required this.param, required this.total, required this.rows});

  bool get hasNext => param.pageNum * param.pageSize < total;
}

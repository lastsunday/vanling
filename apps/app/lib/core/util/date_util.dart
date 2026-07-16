import 'package:date_format/date_format.dart';

class DateUtil {
  static String format(DateTime dateTime) {
    return formatDate(dateTime, [
      "yyyy",
      "-",
      "mm",
      "-",
      "dd",
      " ",
      "HH",
      ":",
      "nn",
      ":",
      "ss",
    ]);
  }
}

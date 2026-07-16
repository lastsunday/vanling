import 'package:flutter_easyloading/flutter_easyloading.dart';

class UI {
  static void showLoading({String? text}) {
    EasyLoading.show(maskType: EasyLoadingMaskType.black, status: text);
  }

  static void showProgress(double value) {
    EasyLoading.showProgress(
      value,
      maskType: EasyLoadingMaskType.black,
      status: "${(value * 100).toStringAsFixed(2)}%",
    );
  }

  static void hideLoading() {
    EasyLoading.dismiss();
  }

  static void showSuccess(String text) {
    EasyLoading.showSuccess(text);
  }

  static void showError(String text) {
    EasyLoading.showError(text);
  }

  static void showInfo(String text) {
    EasyLoading.showInfo(text);
  }

  static void showToast(String text) {
    EasyLoading.showToast(text);
  }
}

import 'package:flutter/material.dart';
import 'package:fluttertoast/fluttertoast.dart' as ft;

class Toast {
  static void show(String message) {
    ft.Fluttertoast.showToast(
      msg: message,
      toastLength: ft.Toast.LENGTH_SHORT,
      gravity: ft.ToastGravity.CENTER,
      timeInSecForIosWeb: 2,
      backgroundColor: Colors.black45,
      textColor: Colors.white,
      fontSize: 16.0,
    );
  }

  static void success(
    BuildContext context,
    String message, {
    String image = "assets/images/add_success.png",
  }) {
    ft.FToast()
        .init(context)
        .showToast(
          child: _buildToastView(context, message, image),
          gravity: ft.ToastGravity.TOP,
        );
  }

  static void error(
    BuildContext context,
    String message, {
    String image = "assets/images/add_fail.png",
  }) {
    ft.FToast()
        .init(context)
        .showToast(
          child: _buildToastView(context, message, image),
          gravity: ft.ToastGravity.TOP,
        );
  }

  static Widget _buildToastView(
    BuildContext context,
    String message,
    String image,
  ) {
    return Container(
      padding: const EdgeInsets.symmetric(horizontal: 20, vertical: 12),
      decoration: BoxDecoration(
        color: Colors.white,
        borderRadius: BorderRadius.circular(4),
        boxShadow: const [BoxShadow(color: Colors.grey, blurRadius: 11)],
      ),
      child: Row(
        mainAxisSize: MainAxisSize.min,
        mainAxisAlignment: MainAxisAlignment.start,
        children: [
          Image.asset(image, width: 20, height: 20),
          const SizedBox(width: 8),
          Expanded(
            child: Text(
              message,
              // textAlign: TextAlign.center,
              style: const TextStyle(color: Colors.black, fontSize: 13),
            ),
          ),
        ],
      ),
    );
  }
}

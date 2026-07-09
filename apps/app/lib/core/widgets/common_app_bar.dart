import 'package:flutter/material.dart';

class CommonAppBar extends AppBar {
  CommonAppBar(
      {super.key,
      super.title,
      Widget? leading,
      bool showLeading = false,
      super.automaticallyImplyLeading = true,
      super.centerTitle = true,
      super.titleTextStyle = const TextStyle(color: Color(0xFF171321), fontSize: 16, fontWeight: FontWeight.w600),
      super.backgroundColor,
      super.elevation = 0,
      super.actions})
      : super(leading: showLeading ? (leading ?? const BackButton(color: Colors.black)) : null);
}

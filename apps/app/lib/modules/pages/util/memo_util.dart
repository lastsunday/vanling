import 'package:app/l10n/app_localizations.dart';
import 'package:flutter/material.dart';
import 'package:app/modules/app/model/memo_model.dart';

class MemoUtil {
  static String getLableByDisplayMode(
      DisplayMode displayMode, BuildContext context) {
    return switch (displayMode) {
      DisplayMode.auto => AppLocalizations.of(context)!.displayModeAuto,
      DisplayMode.text => AppLocalizations.of(context)!.displayModeText,
      DisplayMode.image => AppLocalizations.of(context)!.displayModeImage,
    };
  }

  static IconData getIconByDisplayMode(DisplayMode displayMode) {
    return switch (displayMode) {
      DisplayMode.auto => Icons.remove_red_eye,
      DisplayMode.text => Icons.text_fields,
      DisplayMode.image => Icons.image,
    };
  }
}

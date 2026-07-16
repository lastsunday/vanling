import 'package:flutter/material.dart';

class GlobalKeys {
  static GlobalKey<ScaffoldState> rootAppKey = GlobalKey();
  static GlobalKey<ScaffoldState> rootScaffoldKey = GlobalKey();
  static GlobalKey<State<BottomNavigationBar>> rootBottomNavigationBarKey =
      GlobalKey();
}

class LocalStorageKeys {
  static String navigationBarCurrentIndexKey = "navigation_bar_current_index";
  static String currentMergeRequestWebUrlKey = "current_merge_request_web_url";
}

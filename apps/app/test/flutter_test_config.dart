import 'dart:async';

import 'package:flutter_test/flutter_test.dart';
import 'package:app/core/domain/global_time.dart';
import 'package:app/core/local_storage.dart';
import 'package:shared_preferences/shared_preferences.dart';
import 'package:webview_flutter_android/webview_flutter_android.dart';
import 'package:webview_flutter_platform_interface/webview_flutter_platform_interface.dart';

Future<void> testExecutable(FutureOr<void> Function() testMain) async {
  setUp(() async {
    SharedPreferences.setMockInitialValues(<String, Object>{});
    await LocalStorage.init();
    // TODO: set the privacy alert not showing for some test issue: "not hit test on the specified widget"
    LocalStorage.save('privacy-policy-agreed', true);
    WebViewPlatform.instance = TestingWebViewPlatform();
  });

  tearDown(() async {
    await LocalStorage.removeAll();
    GlobalTime.reset();
  });

  await testMain();
}

class TestingWebViewPlatform extends WebViewPlatform {
  @override
  PlatformWebViewController createPlatformWebViewController(
    PlatformWebViewControllerCreationParams params,
  ) {
    return TestingWebViewController(params);
  }

  @override
  PlatformWebViewCookieManager createPlatformCookieManager(
    PlatformWebViewCookieManagerCreationParams params,
  ) {
    return AndroidWebViewCookieManager(params);
  }

  @override
  PlatformNavigationDelegate createPlatformNavigationDelegate(
    PlatformNavigationDelegateCreationParams params,
  ) {
    return AndroidNavigationDelegate(params);
  }

  @override
  PlatformWebViewWidget createPlatformWebViewWidget(
    PlatformWebViewWidgetCreationParams params,
  ) {
    return AndroidWebViewWidget(params);
  }
}

class TestingWebViewController extends AndroidWebViewController {
  TestingWebViewController(super.params);

  @override
  Future<bool> canGoBack() {
    return Future(() => false);
  }
}

import 'dart:async';

import 'package:flutter/material.dart';
import 'package:app/core/connection_provider/connection_provider.dart';
import 'package:app/core/widgets/common_app_bar.dart';
import 'package:app/modules/auth/oauth2_code_login_model.dart';
import 'package:webview_flutter/webview_flutter.dart';
import 'package:webview_flutter_wkwebview/webview_flutter_wkwebview.dart';

class Oauth2CodeLoginPage extends StatefulWidget {
  final Map arguments;

  const Oauth2CodeLoginPage({required this.arguments, super.key});

  @override
  State<Oauth2CodeLoginPage> createState() => _Oauth2CodeLoginPageState();
}

class _Oauth2CodeLoginPageState extends State<Oauth2CodeLoginPage> {
  final Oauth2CodeLoginModel _loginModel = Oauth2CodeLoginModel();
  late WebViewController _controller;
  late final WebViewCookieManager cookieManager = WebViewCookieManager();

  @override
  void initState() {
    super.initState();
    _loginModel.configuration(host: widget.arguments['host']);
    late final PlatformWebViewControllerCreationParams params;
    if (WebViewPlatform.instance is WebKitWebViewPlatform) {
      params = WebKitWebViewControllerCreationParams(
          allowsInlineMediaPlayback: true,
          mediaTypesRequiringUserAction: const <PlaybackMediaTypes>{});
    } else {
      params = const PlatformWebViewControllerCreationParams();
    }
    _controller = WebViewController.fromPlatformCreationParams(params)
      ..setJavaScriptMode(JavaScriptMode.unrestricted)
      ..setNavigationDelegate(NavigationDelegate(
        onNavigationRequest: (NavigationRequest request) {
          if (request.url.startsWith(_loginModel.redirectUri!)) {
            var uri = Uri.parse(request.url);
            String code = uri.queryParameters["code"] as String;
            String redirectUri = uri.origin + uri.path;
            _loginModel.exchangeToken(code, redirectUri).then((bool success) {
              if (success && mounted) Navigator.pop(context, true);
            }).catchError((e) {
              _clearCookies();
              if (mounted) Navigator.pop(context, true);
              throw e;
            });
            return NavigationDecision.prevent;
          }
          return NavigationDecision.navigate;
        },
      ));
    if (ConnectionProvider.isLoggedOutByUser) {
      _controller.clearCache();
      _clearCookies();
    }
    if (_loginModel.authorizeUrl != null) {
      _controller.loadRequest(Uri.parse(_loginModel.authorizeUrl!));
    }
  }

  @override
  Widget build(BuildContext context) {
    return PopScope(
        canPop: false,
        onPopInvokedWithResult: (didPop, result) async {
          if (didPop) return;
          var canGoBack = await _controller.canGoBack();
          if (canGoBack) {
            _controller.goBack();
          }
        },
        child: Scaffold(
            appBar: CommonAppBar(
              title: const Text(""),
              showLeading: true,
              backgroundColor: Colors.white,
              leading: BackButton(
                  color: Colors.black,
                  onPressed: () {
                    _controller.canGoBack().then((value) {
                      if (value) {
                        _controller.goBack();
                      } else if (mounted) {
                        Navigator.pop(this.context);
                      }
                    });
                  },
            )),
            body: _loginModel.authorizeUrl == null
                ? Container()
                : WebViewWidget(controller: _controller)));
  }

  Future<void> _clearCookies() async {
    await cookieManager.clearCookies();
  }
}

import 'dart:async';

import 'package:app/l10n/app_localizations.dart';
import 'package:camera/camera.dart';
import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_easyloading/flutter_easyloading.dart';
import 'package:flutter_localized_locales/flutter_localized_locales.dart';
import 'package:go_router/go_router.dart';
import 'package:app/core/connection_provider/connection_provider.dart';
// import 'package:google_fonts/google_fonts.dart';
import 'package:app/core/local_storage.dart';
import 'package:app/env.dart';
import 'package:app/modules/app/common/service_exception.dart';
import 'package:app/modules/app/store/local_memo_store.dart';
import 'package:app/modules/app/store/online_memo_store.dart';
import 'package:app/modules/auth/auth.dart';
import 'package:app/modules/auth/login_page.dart';
import 'package:app/modules/pages/about.dart';
import 'package:app/modules/pages/barcode/barcode_scanner_view.dart';
import 'package:app/modules/pages/home.dart';
import 'package:app/modules/pages/memo_add_or_update.dart';
import 'package:app/modules/pages/setting.dart';
import 'package:app/theme/app_theme_data.dart';
import 'package:provider/provider.dart';
import 'package:app/core/log_helper.dart';

import 'modules/app/app_setting.dart';
import 'modules/app/app_store.dart';
import 'modules/auth/login_mode.dart';

List<CameraDescription> cameras = [];

Future<void> main() async {
  runZonedGuarded(
    () async {
      WidgetsFlutterBinding.ensureInitialized();
      await LocalStorage.init();
      await ConnectionProvider().restore();
      await AppStore.initEnv();
      // GoogleFonts.config.allowRuntimeFetching = false;
      int cameraLength = 0;
      try {
        cameras = await availableCameras();
        cameraLength = cameras.length;
      } on CameraException {
        // log.fine(e.toString());
      } on MissingPluginException {
        // log.fine(e.toString());
      }

      LogHelper.info("cameraLength = $cameraLength");
      runApp(MyApp());
      LogHelper.info("App Start");
    },
    (error, stack) {
      AppStore.eventBus.fire(ErrorToastEvent(e: error));
      if (error is ServiceException) {
        LogHelper.err(error.serviceMessage, stack);
      } else {
        LogHelper.err(error.toString(), stack);
      }
    },
  );
}

final GoRouter _router = GoRouter(
  routes: <RouteBase>[
    GoRoute(
      path: '/',
      builder: (BuildContext context, GoRouterState state) {
        return const HomePage();
      },
      routes: <RouteBase>[
        GoRoute(
          path: loginPageRouteName,
          builder: (context, state) =>
              LoginMode.password == Env.config.loginMode
              ? LoginPage(arguments: state.extra as Map)
              : Oauth2CodeLoginPage(arguments: state.extra as Map),
        ),
        GoRoute(
          path: "memo/add",
          builder: (context, state) => MemoAddOrUpdatePage(
            updatePage: false,
            memo: (state.extra as MemoParam).memo,
          ),
        ),
        GoRoute(
          path: "scan",
          builder: (context, state) => BarcodeScannerView(),
        ),
        GoRoute(
          path: "memo/update",
          builder: (context, state) => MemoAddOrUpdatePage(
            updatePage: true,
            memo: (state.extra as MemoParam).memo,
          ),
        ),
        GoRoute(
          path: 'setting',
          builder: (BuildContext context, GoRouterState state) {
            return const SettingPage();
          },
        ),
        GoRoute(path: 'about', builder: (context, state) => const AboutPage()),
      ],
    ),
  ],
);

class MyApp extends StatelessWidget {
  const MyApp({super.key});

  // This widget is the root of your application.
  @override
  Widget build(BuildContext context) {
    return MultiProvider(
      providers: [
        ChangeNotifierProvider(create: (context) => AppStore()),
        ChangeNotifierProvider(create: (context) => LocalMemoStore()),
        ChangeNotifierProvider(create: (context) => OnlineMemoStore()),
        ChangeNotifierProvider(create: (context) => ConnectionProvider()),
      ],
      child: ModelBinding(
        initialModel: AppStore.getDefaultAppSetting(),
        child: FutureBuilder(
          future: Future<bool>.value(true),
          builder: (BuildContext context, AsyncSnapshot snapshot) {
            final options = AppSetting.of(context);
            if (snapshot.data == true) {
              Provider.of<AppStore>(context).init();
              WidgetsBinding.instance.addPostFrameCallback((_) {
                Provider.of<AppStore>(context, listen: false).initUI(context);
              });
              return ApplyTextOptions(
                child: MaterialApp.router(
                  routerConfig: _router,
                  title: 'Chobits',
                  themeMode: options.themeMode,
                  theme: AppThemeData.lightThemeData.copyWith(
                    platform: options.platform,
                  ),
                  darkTheme: AppThemeData.darkThemeData.copyWith(
                    platform: options.platform,
                  ),
                  localizationsDelegates: const [
                    AppLocalizations.delegate,
                    ...AppLocalizations.localizationsDelegates,
                    LocaleNamesLocalizationsDelegate(),
                  ],
                  supportedLocales: const [
                    ...AppLocalizations.supportedLocales,
                  ],
                  locale: options.locale,
                  localeListResolutionCallback: (locales, supportedLocales) {
                    deviceLocale = locales?.first;
                    return basicLocaleListResolution(locales, supportedLocales);
                  },
                  builder: EasyLoading.init(),
                ),
              );
            } else {
              return const CircularProgressIndicator();
            }
          },
        ),
      ),
    );
  }
}

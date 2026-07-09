import 'dart:async';

import 'package:app/l10n/app_localizations.dart';
import 'package:dio/dio.dart';
import 'package:flutter/material.dart';
import 'package:flutter_local_notifications/flutter_local_notifications.dart';
import 'package:go_router/go_router.dart';
import 'package:app/core/connection_provider/connection_provider.dart';
import 'package:app/core/net/authorized_denied_exception.dart';
import 'package:app/env.dart';
import 'package:app/modules/app/app_store.dart';
import 'package:app/modules/app/common/service_exception.dart';
import 'package:app/modules/app/layout/adaptive.dart';
import 'package:app/modules/app/store/local_memo_store.dart';
import 'package:app/modules/app/store/memo_store.dart';
import 'package:app/modules/app/store/menu.dart';
import 'package:app/modules/app/store/online_memo_store.dart';
import 'package:app/modules/app/ui.dart';
import 'package:app/modules/auth/login_mode.dart';
import 'package:provider/provider.dart';
import 'package:r_upgrade/r_upgrade.dart';
import 'package:upgrader/upgrader.dart';
import 'package:app/core/log_helper.dart';
import 'package:version/version.dart';

import 'memo.dart';

class HomePage extends StatefulWidget {
  const HomePage({super.key});

  @override
  State<HomePage> createState() => _HomePageState();
}

class _HomePageState extends State<HomePage> {
  static const String notificationChannelId = "channelId";
  static const String notificationChannelName = "channelName";

  final appcast = Appcast(osVersion: Version(0, 0, 0));

  int installPackageCurrentLength = 0;
  int installPackageMaxLength = 0;
  double installPackageSpeed = 0.0;
  double installPackagePlanTime = 0.0;
  int? id;
  final GlobalKey<ScaffoldState> _scaffoldkey = GlobalKey<ScaffoldState>();

  StreamSubscription? menuOpenSubscription;

  StreamSubscription? errorToastSubscription;

  StreamSubscription? notificationSubscription;
  bool desktopShowMenu = true;

  late MemoStore memoStore = Provider.of<LocalMemoStore>(context, listen: true);

  @override
  void initState() {
    super.initState();
    Provider.of<AppStore>(context, listen: false).initWithContext(context);
    _setupSubscription();
  }

  void _setupSubscription() {
    menuOpenSubscription = AppStore.eventBus.on<MenuOpenEvent>().listen((
      event,
    ) {
      if (!mounted) return;
      final isDesktop = isDisplayDesktop(context);
      if (isDesktop) {
        desktopShowMenu = !desktopShowMenu;
      } else {
        _scaffoldkey.currentState!.openDrawer();
      }
      setState(() {});
    });
    errorToastSubscription = AppStore.eventBus.on<ErrorToastEvent>().listen((
      event,
    ) {
      if (!mounted) return;
      if (event.e is AuthorizedDeniedException) {
        UI.showError(AppLocalizations.of(context)!.pleaseLogin);
      } else if (event.e is ServiceException) {
        ServiceException exception = event.e as ServiceException;
        UI.showError(exception.serviceMessage);
      } else if (event.e is DioException) {
        UI.showError(AppLocalizations.of(context)!.serverError);
      } else {
        UI.showError(event.e.toString());
      }
    });
    notificationSubscription = AppStore.eventBus.on<NotificationEvent>().listen((
      event,
    ) async {
      LogHelper.info(
        "[IM] on listen NotificationEvent name = ${event.name},text = ${event.text}",
      );

      FlutterLocalNotificationsPlugin flutterLocalNotificationsPlugin =
          FlutterLocalNotificationsPlugin();
      // initialise the plugin. app_icon needs to be a added as a drawable resource to the Android head project
      const AndroidInitializationSettings initializationSettingsAndroid =
          AndroidInitializationSettings('ic_launcher');
      const DarwinInitializationSettings initializationSettingsDarwin =
          DarwinInitializationSettings();
      const LinuxInitializationSettings initializationSettingsLinux =
          LinuxInitializationSettings(defaultActionName: 'Open notification');
      const InitializationSettings initializationSettings =
          InitializationSettings(
            android: initializationSettingsAndroid,
            iOS: initializationSettingsDarwin,
            macOS: initializationSettingsDarwin,
            linux: initializationSettingsLinux,
          );
      await flutterLocalNotificationsPlugin.initialize(
        initializationSettings,
        onDidReceiveNotificationResponse: onDidReceiveNotificationResponse,
      );
      const AndroidNotificationDetails androidNotificationDetails =
          AndroidNotificationDetails(
            notificationChannelId,
            notificationChannelName,
            importance: Importance.max,
            priority: Priority.high,
          );
      const NotificationDetails notificationDetails = NotificationDetails(
        android: androidNotificationDetails,
      );
      flutterLocalNotificationsPlugin.show(
        0,
        event.name,
        event.text,
        notificationDetails,
      );
    });
  }

  void onDidReceiveNotificationResponse(
    NotificationResponse notificationResponse,
  ) async {
    final String? payload = notificationResponse.payload;
    if (notificationResponse.payload != null) {
      LogHelper.info('notification payload: $payload');
    }
  }

  void _showDownloadDialog() async {
    StreamSubscription<DownloadInfo>? streamSubscription;
    AppcastItem? bestItem = appcast.bestItem();
    // id = await RUpgrade.getLastUpgradedId();//如果获取上一次持久化的id，如果文件下载不完整则会一直安装失败（如果RUpgrade的类库支持文件安装前校验（如md5），则可解决。这暂未找到类库有此特性提供）
    if (id != null) {
      DownloadStatus? status = await RUpgrade.getDownloadStatus(id!);
      if (status != null) {
        if (status == DownloadStatus.STATUS_SUCCESSFUL) {
          RUpgrade.install(id!);
          return;
        } else if (status == DownloadStatus.STATUS_FAILED) {
          RUpgrade.cancel(id!);
        }
      }
      bool? isSuccess = await RUpgrade.upgradeWithId(id!);
      if (!isSuccess!) {
        id = await RUpgrade.upgrade(
          bestItem!.fileURL!,
          fileName: bestItem.versionString,
        );
      }
    } else {
      id = await RUpgrade.upgrade(bestItem!.fileURL!);
    }
    Future.delayed(const Duration(milliseconds: 0), () async {
      if (!mounted) return;
      await showDialog(
        context: context,
        builder: (context) {
          return StatefulBuilder(
            builder: (context, setState) {
              streamSubscription = RUpgrade.stream.listen((event) {
                if (context.mounted) {
                  if (event.status == DownloadStatus.STATUS_SUCCESSFUL) {
                    //当下载完成，从event获取的currentLength和maxLength的值为0，所以使用有效的值进行替代（值为0这个问题，类库需要进行修正）
                    setState(() {
                      installPackageCurrentLength = installPackageMaxLength;
                      installPackageSpeed = 0;
                      installPackagePlanTime = 0;
                    });
                  }
                  if (event.status == DownloadStatus.STATUS_RUNNING) {
                    setState(() {
                      installPackageCurrentLength = event.currentLength!;
                      installPackageMaxLength = event.maxLength!;
                      installPackageSpeed = event.speed ?? 0;
                      installPackagePlanTime = event.planTime ?? 0;
                    });
                  }
                }
                LogHelper.debug(
                  "${event.currentLength}/${event.maxLength},${event.status},${event.path},${event.speed},${event.planTime}",
                );
              });
              return AlertDialog(
                title: Text(AppLocalizations.of(context)!.newVersionDownload),
                content: Column(
                  mainAxisSize: MainAxisSize.min,
                  children: [
                    Padding(
                      padding: const EdgeInsets.all(10),
                      child: LinearProgressIndicator(
                        value: installPackageMaxLength == 0
                            ? 0
                            : (installPackageCurrentLength /
                                  installPackageMaxLength),
                      ),
                    ),
                    Text(
                      "${(installPackageCurrentLength / (1024 * 1024)).toStringAsFixed(2)}MB/${(installPackageMaxLength / (1024 * 1024)).toStringAsFixed(2)}MB(${(installPackageCurrentLength / installPackageMaxLength * 100).toStringAsFixed(2)}%)",
                    ),
                    Text(
                      "${installPackageSpeed.toInt()} kb/s,${AppLocalizations.of(context)!.planTime}:${installPackagePlanTime.toInt()}s",
                    ),
                  ],
                ),
                actions: [
                  TextButton(
                    onPressed: () {
                      context.pop();
                    },
                    child: Text(AppLocalizations.of(context)!.cancel),
                  ),
                ],
              );
            },
          );
        },
      );
      streamSubscription?.cancel();
      RUpgrade.pause(id!);
    });
  }

  @override
  void dispose() {
    super.dispose();
    Provider.of<AppStore>(context, listen: false).unmountInitWithContent();
    if (menuOpenSubscription != null) {
      menuOpenSubscription!.cancel();
    }
    if (errorToastSubscription != null) {
      errorToastSubscription!.cancel();
    }
    if (notificationSubscription != null) {
      notificationSubscription!.cancel();
    }
  }

  void go({String? path}) {
    final isDesktop = isDisplayDesktop(context);
    if (!isDesktop) {
      context.pop();
    }
    if (path != null) {
      context.go(path);
    }
  }

  @override
  Widget build(BuildContext context) {
    final isDesktop = isDisplayDesktop(context);

    void toLogin() async {
      if (Env.config.loginFeatureEnable) {
        await context.push(
          "/$loginPageRouteName",
          extra: {'host': Env.config.oauthUrl},
        );
        if (mounted) {
          AppStore.eventBus.fire(
            LoginEvent(
              loginSuccess: Provider.of<ConnectionProvider>(
                this.context,
                listen: false,
              ).authorized,
            ),
          );
        }
      } else {
        await showDialog(
          context: context,
          builder: (context) {
            return AlertDialog(
              content: Text(AppLocalizations.of(context)!.loginNotOpen),
            );
          },
        );
        if (mounted) {
          AppStore.eventBus.fire(
            LoginEvent(
              loginSuccess: Provider.of<ConnectionProvider>(
                this.context,
                listen: false,
              ).authorized,
            ),
          );
        }
      }
    }

    Widget getMenuHeader() {
      return Provider.of<ConnectionProvider>(context).authorized
          ? GestureDetector(
              child: UserAccountsDrawerHeader(
                accountName:
                    Provider.of<ConnectionProvider>(context).tokenInvalid
                    ? Row(
                        children: [
                          Text(ConnectionProvider.connection!.userInfo.sub),
                          Text(
                            "(${AppLocalizations.of(context)!.loginTimeout})",
                          ),
                        ],
                      )
                    : Text(ConnectionProvider.connection!.userInfo.sub),
                accountEmail: const Text(""),
                currentAccountPicture:
                    ConnectionProvider.connection!.userInfo.avatar != null
                    ? ClipRRect(
                        borderRadius: const BorderRadius.all(
                          Radius.circular(42),
                        ),
                        child: Image.network(
                          ConnectionProvider.connection!.userInfo.avatar!,
                          width: 42,
                          height: 42,
                          errorBuilder: (context, error, stackTrace) {
                            return const CircleAvatar(
                              child: FlutterLogo(size: 42.0),
                            );
                          },
                        ),
                      )
                    : const CircleAvatar(child: FlutterLogo(size: 42.0)),
              ),
              onTap: () {
                if (ConnectionProvider().tokenInvalid) {
                  var connection = ConnectionProvider.connection!;
                  ConnectionProvider().loggedOutSelectedConnection(connection);
                  toLogin();
                } else {
                  showDialog(
                    context: context,
                    builder: (context) {
                      return AlertDialog(
                        content: Text(
                          AppLocalizations.of(context)!.areYouSureLogout,
                        ),
                        actions: [
                          TextButton(
                            onPressed: () => {context.pop()},
                            child: Text(AppLocalizations.of(context)!.cancel),
                          ),
                          TextButton(
                            onPressed: () {
                              var connection = ConnectionProvider.connection!;
                              ConnectionProvider().loggedOutSelectedConnection(
                                connection,
                              );
                              setState(() {});
                              context.pop();
                            },
                            child: Text(AppLocalizations.of(context)!.confirm),
                          ),
                        ],
                      );
                    },
                  );
                }
              },
            )
          : GestureDetector(
              child: UserAccountsDrawerHeader(
                accountName: Text(AppLocalizations.of(context)!.pleaseLogin),
                accountEmail: const Text(""),
                currentAccountPicture: const CircleAvatar(
                  child: FlutterLogo(size: 42.0),
                ),
              ),
              onTap: () {
                toLogin();
              },
            );
    }

    ListView getMenuItem(BuildContext context) {
      return ListView(
        children: [
          getMenuHeader(),
          ListTile(
            title: Text(
              "${AppLocalizations.of(context)!.memo}(${memoStore.memoTotal})",
            ),
            leading: const Icon(Icons.comment),
            onTap: () async {
              Provider.of<AppStore>(context, listen: false).setMenu(Menu.memo);
              go();
            },
          ),
          ListTile(
            title: Text(AppLocalizations.of(context)!.setting),
            leading: const Icon(Icons.settings),
            onTap: () {
              go(path: "/setting");
            },
          ),
          ListTile(
            title: Text(AppLocalizations.of(context)!.about),
            leading: const Icon(Icons.info),
            onTap: () {
              go(path: "/about");
            },
          ),
        ],
      );
    }

    Drawer getMenu(BuildContext context) {
      return Drawer(child: getMenuItem(context));
    }

    Widget getMemoPage() {
      if (Provider.of<AppStore>(context).showOnlineMemo) {
        memoStore = Provider.of<OnlineMemoStore>(context, listen: true);
        return MemoPage(memoStore, key: const Key("onlineMemo"));
      } else {
        memoStore = Provider.of<LocalMemoStore>(context, listen: true);
        return MemoPage(memoStore, key: const Key("localMemo"));
      }
    }

    Widget getTargetPage() {
      return getMemoPage();
    }

    Widget getDisplay() {
      final body = SafeArea(
        child: Stack(
          children: [
            getTargetPage(),
            UpgradeAlert(
              upgrader: Upgrader(
                storeController: UpgraderStoreController(
                  onAndroid: () => UpgraderAppcastStore(
                    appcastURL: Env.config.appcastURL,
                    osVersion: Version(0, 0, 0),
                  ),
                ),
                messages: UpgraderMessages(
                  code: AppLocalizations.of(context)!.localeName,
                ),
              ),
              onUpdate: () {
                _showDownloadDialog();
                return false;
              },
            ),
          ],
        ),
      );

      if (isDesktop) {
        return Row(
          children: [
            desktopShowMenu ? getMenu(context) : Container(),
            const VerticalDivider(width: 1),
            Expanded(
              child: Scaffold(
                resizeToAvoidBottomInset: false,
                key: _scaffoldkey,
                body: body,
              ),
            ),
          ],
        );
      } else {
        return Scaffold(
          resizeToAvoidBottomInset: false,
          key: _scaffoldkey,
          drawer: getMenu(context),
          body: body,
        );
      }
    }

    // This method is rerun every time setState is called, for instance as done
    // by the _incrementCounter method above.
    //
    // The Flutter framework has been optimized to make rerunning build methods
    // fast, so that you can just rebuild anything that needs updating rather
    // than having to individually change instances of widgets.
    return getDisplay();
  }
}

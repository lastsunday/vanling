import 'dart:async';

import 'package:app/l10n/app_localizations.dart';
import 'package:flutter/material.dart';
import 'package:go_router/go_router.dart';
import 'package:app/env.dart';
import 'package:package_info_plus/package_info_plus.dart';
import 'package:r_upgrade/r_upgrade.dart';
import 'package:upgrader/upgrader.dart';
import 'package:app/core/log_helper.dart';
import 'package:version/version.dart';

class AboutPage extends StatefulWidget {
  const AboutPage({super.key});

  @override
  State<AboutPage> createState() => _AboutPageState();
}

class _AboutPageState extends State<AboutPage> {
  PackageInfo _packageInfo = PackageInfo(
    appName: 'Unknown',
    packageName: 'Unknown',
    version: 'Unknown',
    buildNumber: 'Unknown',
    buildSignature: 'Unknown',
    installerStore: 'Unknown',
  );

  bool _newVersionAvailable = false;
  bool _versionUpToDate = false;
  String? _newVersion = "";

  final appcast = Appcast(osVersion: Version(0, 0, 0));
  int installPackageCurrentLength = 0;
  int installPackageMaxLength = 0;
  double installPackageSpeed = 0.0;
  double installPackagePlanTime = 0.0;
  int? id;

  @override
  void initState() {
    super.initState();
    _initPackageInfo();
    _checkAppVersion();
  }

  Future<void> _initPackageInfo() async {
    final info = await PackageInfo.fromPlatform();
    setState(() {
      _packageInfo = info;
    });
  }

  Future<void> _checkAppVersion() async {
    Upgrader upgrader = Upgrader(
      storeController: UpgraderStoreController(
        onAndroid: () => UpgraderAppcastStore(
          appcastURL: Env.config.appcastURL,
          osVersion: Version(0, 0, 0),
        ),
      ),
    );
    await upgrader.initialize();
    if (upgrader.isUpdateAvailable()) {
      setState(() {
        _newVersionAvailable = true;
        _versionUpToDate = false;
        _newVersion = upgrader.currentAppStoreVersion;
      });
    } else {
      setState(() {
        _versionUpToDate = true;
        _newVersionAvailable = false;
      });
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
  Widget build(BuildContext context) {
    return Scaffold(
      appBar: AppBar(title: Text(AppLocalizations.of(context)!.about)),
      body: SingleChildScrollView(
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.center,
          children: [
            Column(
              children: [
                const Padding(
                  padding: EdgeInsets.all(10),
                  child: FlutterLogo(size: 100),
                ),
                Padding(
                  padding: const EdgeInsets.fromLTRB(0, 0, 0, 40),
                  child: Text(
                    "${AppLocalizations.of(context)!.version}:${_packageInfo.version}",
                    style: Theme.of(context).textTheme.bodyMedium,
                  ),
                ),
              ],
            ),
            ListView(
              shrinkWrap: true,
              children: ListTile.divideTiles(
                context: context,
                tiles: [
                  ListTile(
                    title: Text(AppLocalizations.of(context)!.newVersionUpdate),
                    trailing: _newVersionAvailable
                        ? Wrap(
                            alignment: WrapAlignment.center,
                            crossAxisAlignment: WrapCrossAlignment.center,
                            children: [
                              Container(
                                padding: const EdgeInsets.all(3),
                                color: Theme.of(
                                  context,
                                ).colorScheme.primaryContainer,
                                child: Text(
                                  AppLocalizations.of(context)!.newVersion,
                                  style: TextStyle(
                                    fontSize: Theme.of(
                                      context,
                                    ).textTheme.labelSmall!.fontSize,
                                    color: Theme.of(
                                      context,
                                    ).colorScheme.secondary,
                                  ),
                                ),
                              ),
                              Text(_newVersion ?? ""),
                            ],
                          )
                        : (_versionUpToDate
                              ? Text(
                                  AppLocalizations.of(context)!.versionUpToDate,
                                )
                              : const CircularProgressIndicator()),
                    onTap: () {
                      if (_newVersionAvailable) {
                        _showDownloadDialog();
                      }
                    },
                  ),
                  ListTile(
                    // tileColor: Colors.white.withOpacity(1),
                    onTap: () => {showLicensePage(context: context)},
                    title: Text(
                      MaterialLocalizations.of(context).licensesPageTitle,
                    ),
                  ),
                ],
              ).toList(),
            ),
            //   GestureDetector(
            //       onTap: () {
            //         FlutterClipboard.copy(
            //                 Provider.of<AppStore>(context, listen: false).deviceId)
            //             .then((value) => UI
            //                 .showInfo(AppLocalizations.of(context)!.copyDeviceId));
            //       },
            //       child: Column(
            //         children: [
            //           Padding(
            //             padding: const EdgeInsets.fromLTRB(0, 10, 0, 0),
            //             child: Text(AppLocalizations.of(context)!.deviceId,
            //                 style: Theme.of(context).textTheme.bodyMedium),
            //           ),
            //           SizedBox(
            //             width: 150,
            //             height: 150,
            //             child: PrettyQrView.data(
            //               data: Provider.of<AppStore>(context, listen: false)
            //                   .deviceId,
            //             ),
            //           ),
            //           Text(Provider.of<AppStore>(context, listen: false).deviceId,
            //               style: Theme.of(context).textTheme.bodyMedium),
            //         ],
            //       ))
            //
          ],
        ),
      ),
    );
  }
}

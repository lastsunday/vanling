import 'package:app/l10n/app_localizations.dart';
import 'package:flutter/material.dart';
import 'package:go_router/go_router.dart';
import 'package:app/env.dart';
import 'package:package_info_plus/package_info_plus.dart';
import 'package:background_downloader/background_downloader.dart';
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
  double installPackageProgress = 0.0;
  DownloadTask? _downloadTask;

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
    AppcastItem? bestItem = appcast.bestItem();
    if (bestItem == null || bestItem.fileURL == null) return;
    final task = DownloadTask(
      url: bestItem.fileURL!,
      filename: bestItem.versionString,
      baseDirectory: BaseDirectory.applicationSupport,
      allowPause: true,
      updates: Updates.statusAndProgress,
    );
    _downloadTask = task;
    if (!mounted) return;
    await showDialog(
      context: context,
      builder: (context) {
        return StatefulBuilder(
          builder: (context, setState) {
            FileDownloader().download(
              task,
              onProgress: (progress) {
                if (context.mounted) {
                  setState(() {
                    installPackageProgress = progress;
                  });
                }
                LogHelper.debug("download progress: $progress");
              },
              onStatus: (status) {
                LogHelper.debug("download status: $status");
                if (status == TaskStatus.complete && context.mounted) {
                  FileDownloader().openFile(
                    task: task,
                    mimeType: 'application/vnd.android.package-archive',
                  );
                }
              },
            );
            return AlertDialog(
              title: Text(AppLocalizations.of(context)!.newVersionDownload),
              content: Column(
                mainAxisSize: MainAxisSize.min,
                children: [
                  Padding(
                    padding: const EdgeInsets.all(10),
                    child: LinearProgressIndicator(
                      value: installPackageProgress,
                    ),
                  ),
                  Text("${(installPackageProgress * 100).toStringAsFixed(2)}%"),
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
    if (_downloadTask != null) {
      FileDownloader().pause(_downloadTask!);
    }
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

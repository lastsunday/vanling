import 'dart:convert';
import 'dart:io';

import 'package:app/l10n/app_localizations.dart';
import 'package:file_picker/file_picker.dart';
import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:go_router/go_router.dart';
import 'package:badges/badges.dart' as badges;
import 'package:app/core/log_helper.dart';
import 'package:app/core/util/date_util.dart';
import 'package:app/modules/app/model/memo_model.dart';
import 'package:app/modules/app/ui.dart';
import 'package:app/modules/pages/util/memo_util.dart';

class MemoParam {
  MemoParam({required this.memo});

  MemoModel memo;
}

class MemoResult {
  MemoResult({required this.memo, this.delete = false});

  MemoModel memo;
  bool delete;
}

// ignore: must_be_immutable
class MemoAddOrUpdatePage extends StatefulWidget {
  static const defaultContent = "";

  MemoAddOrUpdatePage({super.key, this.updatePage = false, this.memo}) {
    var now = DateTime.now();
    memo ??= MemoModel(content: "", datetime: now, updatedatetime: now);
  }

  final bool updatePage;
  MemoModel? memo;

  @override
  State<MemoAddOrUpdatePage> createState() => _MemoAddOrUpdatePageState();
}

class _MemoAddOrUpdatePageState extends State<MemoAddOrUpdatePage> {
  String content = "";
  int wordCount = 0;
  int displaymode = 0;

  final contentController = TextEditingController();

  @override
  void initState() {
    super.initState();
    if (widget.memo == null) {
      contentController.text = "";
      content = "";
    } else {
      contentController.text = widget.memo!.content;
      content = widget.memo!.content;
      displaymode = widget.memo!.displaymode;
    }
    wordCount = content.length;
  }

  void _save() {
    widget.memo!.displaymode = displaymode;
    widget.memo!.content = content;
    context.pop({MemoResult(memo: widget.memo!)});
  }

  void _delete() {
    showDialog(
        context: context,
        builder: (context) {
          return AlertDialog(
            content: Text(AppLocalizations.of(context)!.confirmDelete),
            actions: [
              TextButton(
                  onPressed: () => {context.pop()},
                  child: Text(AppLocalizations.of(context)!.cancel)),
              TextButton(
                  onPressed: () {
                    context.pop();
                    context.pop({MemoResult(memo: widget.memo!, delete: true)});
                  },
                  child: Text(AppLocalizations.of(context)!.confirm)),
            ],
          );
        });
  }

  void _changeViewer(BuildContext context) {
    var displayModes = _getDisplayMode();
    showModalBottomSheet<void>(
      context: context,
      builder: (context) {
        return SizedBox(
          height: 300,
          child: Column(
            children: [
              SizedBox(
                height: 50,
                child: Center(
                  child: Text(
                    AppLocalizations.of(context)!.displayMode,
                    textAlign: TextAlign.center,
                  ),
                ),
              ),
              const Divider(thickness: 1),
              Expanded(
                child: ListView.builder(
                  itemCount: displayModes.length,
                  itemBuilder: (context, index) {
                    return ListTile(
                        title: TextButton.icon(
                      icon: Icon(
                          MemoUtil.getIconByDisplayMode(displayModes[index])),
                      label: Text(MemoUtil.getLableByDisplayMode(
                          displayModes[index], context)),
                      onPressed: () {
                        displaymode = index;
                        setState(() {});
                        Navigator.pop(context);
                      },
                    ));
                  },
                ),
              ),
            ],
          ),
        );
      },
    );
  }

  Widget _getIconWidget(int displaymode, IconData iconData) {
    if (displaymode == DisplayMode.image.value && content.startsWith("http")) {
      return badges.Badge(
        badgeContent:
            const Icon(Icons.tips_and_updates, color: Colors.white, size: 10),
        position: badges.BadgePosition.topEnd(top: -10, end: -12),
        showBadge: true,
        child: Icon(iconData),
      );
    } else {
      return Icon(iconData);
    }
  }

  void _convertContent(BuildContext context) {
    if (displaymode != DisplayMode.image.value) {
      return;
    }
    if (!content.startsWith("http")) {
      return;
    }
    showModalBottomSheet<void>(
      context: context,
      builder: (context) {
        return SizedBox(
          height: 300,
          child: Column(
            children: [
              SizedBox(
                height: 200,
                child: Center(
                  child: content.startsWith("http")
                      ? Image.network(content)
                      : throw Exception("not support other content"),
                ),
              ),
              const Divider(thickness: 1),
              Expanded(
                child: ListView.builder(
                  itemCount: 1,
                  itemBuilder: (context, index) {
                    return ListTile(
                        title: TextButton.icon(
                      icon: const Icon(Icons.transform),
                      label:
                          Text(AppLocalizations.of(context)!.convertToBase64),
                      onPressed: () async {
                        final ByteData imageData =
                            await NetworkAssetBundle(Uri.parse(content))
                                .load("");
                        final Uint8List bytes = imageData.buffer.asUint8List();
                        var base64Content = base64Encode(bytes);
                        setState(() {
                          content = base64Content;
                        });
                        contentController.text = content;
                        if (context.mounted) {
                          context.pop();
                        }
                      },
                    ));
                  },
                ),
              ),
            ],
          ),
        );
      },
    );
  }

  List<DisplayMode> _getDisplayMode() {
    var displayModes = [DisplayMode.auto, DisplayMode.text, DisplayMode.image];
    return displayModes;
  }

  void _fileOpen() async {
    UI.showLoading(text: AppLocalizations.of(context)!.selectFile);
    FilePickerResult? result = await FilePicker.platform.pickFiles();
    if (result != null) {
      PlatformFile selectFile = result.files.first;
      File file = File(selectFile.path!);
      var fileTotalLength = file.lengthSync();
      List<int> dataLoaded = [];
      file.openRead().listen(
          (event) async {
            dataLoaded.addAll(event);
            var countLength = dataLoaded.length;
            double progressValue = countLength / fileTotalLength;
            LogHelper.debug("countLength = $countLength");
            LogHelper.debug("fileTotalLength = $fileTotalLength");
            LogHelper.debug("progress = $progressValue");
            UI.showProgress(countLength / fileTotalLength);
          },
          onError: (Object error) => {UI.showError("$error")},
          onDone: () async {
            UI.showProgress(1);
            await Future.delayed(const Duration(milliseconds: 500));
            if (displaymode == DisplayMode.image.value) {
              var text = const Base64Encoder().convert(dataLoaded.cast());
              setState(() {
                content = text;
              });
            } else {
              var text = String.fromCharCodes(dataLoaded);
              setState(() {
                content = text;
              });
            }
            UI.hideLoading();
          });
    } else {
      // User canceled the picker
      UI.hideLoading();
    }
  }

  List<Widget> _getActionList(BuildContext context) {
    List<Widget> result = [];
    result.add(IconButton(
      onPressed: _fileOpen,
      icon: const Icon(Icons.file_open),
      tooltip: AppLocalizations.of(context)!.importFile,
    ));
    result.add(Tooltip(
      message: AppLocalizations.of(context)!.displayMode,
      child: Center(
          child: InkWell(
              onLongPress: () => _convertContent(context),
              onTap: () => _changeViewer(context),
              child: Ink(
                child: Padding(
                    padding: const EdgeInsets.all(8),
                    child: _getIconWidget(
                        displaymode,
                        MemoUtil.getIconByDisplayMode(
                            DisplayMode.getType(displaymode)))),
              ))),
    ));
    if (widget.updatePage) {
      result.add(IconButton(
        onPressed: _delete,
        icon: const Icon(Icons.delete),
        tooltip: AppLocalizations.of(context)!.delete,
      ));
    }
    result.add(IconButton(
      onPressed: _save,
      icon: const Icon(Icons.save),
      tooltip: AppLocalizations.of(context)!.save,
    ));
    return result;
  }

  @override
  Widget build(BuildContext context) {
    // This method is rerun every time setState is called, for instance as done
    // by the _incrementCounter method above.
    //
    // The Flutter framework has been optimized to make rerunning build methods
    // fast, so that you can just rebuild anything that needs updating rather
    // than having to individually change instances of widgets.
    return Scaffold(
        appBar: AppBar(
          // TRY THIS: Try changing the color here to a specific color (to
          // Colors.amber, perhaps?) and trigger a hot reload to see the AppBar
          // change color while the other colors stay the same.
          backgroundColor: Theme.of(context).colorScheme.primaryContainer,
          // Here we take the value from the MyHomePage object that was created by
          // the App.build method, and use it to set our appbar title.
          title: widget.updatePage
              ? Text(AppLocalizations.of(context)!.updateMemo)
              : Text(AppLocalizations.of(context)!.addMemo),
          actions: _getActionList(context),
        ),
        // Center is a layout widget. It takes a single child and positions it
        // in the middle of the parent.
        body: Column(children: [
          Row(
            children: [
              Text(DateUtil.format(widget.memo!.datetime)),
              const Text(" | "),
              Text("$wordCount ${AppLocalizations.of(context)!.word}")
            ],
          ),
          Expanded(
            child: TextFormField(
              autofocus: !widget.updatePage,
              controller: contentController,
              maxLines: null,
              expands: true,
              textAlignVertical: TextAlignVertical.top,
              decoration: InputDecoration(
                  contentPadding: const EdgeInsets.all(5),
                  border: const OutlineInputBorder(),
                  enabledBorder:
                      const OutlineInputBorder(borderSide: BorderSide.none),
                  disabledBorder:
                      const OutlineInputBorder(borderSide: BorderSide.none),
                  focusedBorder:
                      const OutlineInputBorder(borderSide: BorderSide.none),
                  fillColor: Colors.grey.withValues(alpha: 0.1),
                  filled: true,
                  hintText: AppLocalizations.of(context)!.inputContentHint),
              style: Theme.of(context).textTheme.bodyMedium,
              // initialValue: content,
              onChanged: (value) => {
                setState(() {
                  content = value;
                  wordCount = value.length;
                })
              },
            ),
          )
        ]));
  }
}

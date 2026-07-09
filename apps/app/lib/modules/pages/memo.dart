import 'dart:async';
import 'dart:convert';
import 'dart:io';
import 'dart:math';

import 'package:app/l10n/app_localizations.dart';
import 'package:easy_refresh/easy_refresh.dart';
import 'package:flag/flag.dart';
import 'package:flutter/foundation.dart';
import 'package:flutter/material.dart';
import 'package:flutter/semantics.dart';
import 'package:flutter_spinkit/flutter_spinkit.dart';
import 'package:flutter_tts/flutter_tts.dart';
import 'package:go_router/go_router.dart';
import 'package:app/core/log_helper.dart';
import 'package:app/core/widgets/refresh_footer.dart';
import 'package:app/core/widgets/refresh_header.dart';
import 'package:app/main.dart';
import 'package:app/modules/app/app_store.dart';
import 'package:app/modules/app/common/page_param.dart';
import 'package:app/modules/app/common/page_result.dart';
import 'package:app/modules/app/model/memo_model.dart';
import 'package:app/modules/app/store/memo_store.dart';
import 'package:app/modules/pages/barcode/barcode_scanner_view_result.dart';
import 'package:app/modules/pages/util/memo_util.dart';
import 'package:app/modules/pages/widget/memo_item.dart';
import 'package:app/third_party/flutter_speed_dial/flutter_speed_dial.dart';
import 'package:photo_view/photo_view.dart';
import 'package:provider/provider.dart';
import 'package:badges/badges.dart' as badges;
import 'package:qr_flutter/qr_flutter.dart';
import 'package:reorderable_grid/reorderable_grid.dart';

import 'memo_add_or_update.dart';

enum TtsState { playing, stopped, paused, continued }

// ignore: must_be_immutable
class MemoPage extends StatefulWidget {
  MemoPage(this.memoStore, {super.key});

  MemoStore memoStore;

  @override
  State<MemoPage> createState() => _MemoPageState();
}

class _MemoPageState extends State<MemoPage>
    with SingleTickerProviderStateMixin {
  List<MemoModel> _memoModelList = [];
  late EasyRefreshController _controller;
  late int _pageNum;
  late int _pageSize;
  late int _total;
  int? _displaymode;
  bool _inPageRefreshing = false;
  StreamSubscription? memoFilterSubscription;
  StreamSubscription? memoSearchSubscription;
  bool canMove = false;
  late final AnimationController _itemCanMoveAnimation;
  late final Animation<double> _animation;
  bool hasNext = true;
  String? searchKeyword;
  bool searchMode = false;
  FlutterTts flutterTts = FlutterTts();
  TtsState _ttsState = TtsState.stopped;
  late List<dynamic> languages;
  String? language;
  bool isCurrentLanguageInstalled = false;

  bool get isIOS => !kIsWeb && Platform.isIOS;
  bool get isAndroid => !kIsWeb && Platform.isAndroid;
  bool get isWindows => !kIsWeb && Platform.isWindows;
  bool get isWeb => kIsWeb;

  @override
  void initState() {
    super.initState();
    resetPageInfo();
    _controller = EasyRefreshController(
      controlFinishRefresh: true,
      controlFinishLoad: true,
    );
    _itemCanMoveAnimation = AnimationController(
        vsync: this, duration: const Duration(milliseconds: 300));
    _animation =
        Tween<double>(begin: -0.02, end: 0.02).animate(_itemCanMoveAnimation)
          ..addStatusListener((status) {
            if (status == AnimationStatus.completed) {
              _itemCanMoveAnimation.reverse();
            } else if (status == AnimationStatus.dismissed) {
              _itemCanMoveAnimation.forward();
            }
          });
    _setupSubscription();
  }

  void _initTts(Function setState) {
    flutterTts.setStartHandler(() {
      setState(() {
        _ttsState = TtsState.playing;
      });
    });

    flutterTts.setCompletionHandler(() {
      setState(() {
        _ttsState = TtsState.stopped;
      });
    });

    flutterTts.setErrorHandler((msg) {
      setState(() {
        _ttsState = TtsState.stopped;
      });
    });

    flutterTts.setCancelHandler(() {
      setState(() {
        _ttsState = TtsState.stopped;
      });
    });

    // Android, iOS and Web
    flutterTts.setPauseHandler(() {
      setState(() {
        _ttsState = TtsState.paused;
      });
    });

    flutterTts.setContinueHandler(() {
      setState(() {
        _ttsState = TtsState.continued;
      });
    });
  }

  Widget _languageDropDownSection(List<dynamic> languages, Function setState) =>
      DropdownButton(
        value: language,
        items: getLanguageDropDownMenuItems(languages),
        onChanged: (value) => changedLanguageDropDownItem(value, setState),
      );

  List<DropdownMenuItem<String>> getLanguageDropDownMenuItems(
      List<dynamic> languages) {
    var items = <DropdownMenuItem<String>>[];
    for (dynamic type in languages) {
      String? typeString = type as String;
      List<String> typeSplitList = typeString.toLowerCase().split("-");
      items.add(DropdownMenuItem(
          value: typeString,
          child: Row(
            children: [
              ClipRRect(
                  borderRadius: const BorderRadius.all(Radius.circular(10)),
                  child: Flag.fromString(
                      typeSplitList.length > 1
                          ? typeSplitList[1]
                          : typeSplitList[0],
                      width: 30,
                      height: 30)),
              Text(typeString)
            ],
          )));
    }
    return items;
  }

  void changedLanguageDropDownItem(String? selectedType, Function setState) {
    setState(() {
      language = selectedType;
      AppStore.saveTtsLocale(language);
      flutterTts.setLanguage(language!);
      if (isAndroid) {
        flutterTts
            .isLanguageInstalled(language!)
            .then((value) => isCurrentLanguageInstalled = (value as bool));
      }
    });
  }

  Widget _futureBuilder(Function setState) => FutureBuilder<dynamic>(
      future: _getLanguages(),
      builder: (BuildContext context, AsyncSnapshot<dynamic> snapshot) {
        if (snapshot.hasData) {
          List<dynamic> languageList = snapshot.data as List<dynamic>;
          language = AppStore.getTtsLocale();
          if (language == null || language!.isEmpty) {
            language = languageList.firstOrNull;
          }
          if (language != null) {
            flutterTts.setLanguage(language!);
            if (isAndroid) {
              flutterTts.isLanguageInstalled(language!).then(
                  (value) => isCurrentLanguageInstalled = (value as bool));
            }
          }
          return _languageDropDownSection(languageList, setState);
        } else if (snapshot.hasError) {
          return Container();
        } else {
          return Container();
        }
      });

  Future<dynamic> _getLanguages() async => await flutterTts.getLanguages;

  @override
  void dispose() {
    _controller.dispose();
    _cancelSubscription();
    super.dispose();
  }

  void resetPageInfo() {
    _pageNum = 1;
    _pageSize = 20;
    _total = 0;
    _memoModelList = [];
    _displaymode;
  }

  void _update(MemoModel memo) async {
    var now = DateTime.now();
    await widget.memoStore.updateMemo(MemoModel(
        id: memo.id,
        content: memo.content,
        datetime: memo.datetime,
        displaymode: memo.displaymode,
        updatedatetime: now));
    _refreshList();
  }

  void _save(MemoModel memo) async {
    var now = DateTime.now();
    await widget.memoStore.addMemoToTop(
        MemoModel(
            content: memo.content,
            datetime: now,
            displaymode: memo.displaymode,
            updatedatetime: now),
        _displaymode);
    _refreshList();
  }

  void _delete(MemoModel memo) async {
    await widget.memoStore.deleteMemo(memo);
    _refreshList();
  }

  Future<void> _sortMemo(
      int? displaymode, Map<String, int> idAndSeqMap, int minSeq, int maxSeq) {
    return widget.memoStore.sortMemo(displaymode, idAndSeqMap, minSeq, maxSeq);
  }

  Future<PageResult> _loadData() async {
    var result = await widget.memoStore.pageMemo(
        PageParam(
            pageNum: _pageNum,
            pageSize: _pageSize,
            orderByColumn: "datetime",
            isAsc: "desc"),
        displaymode: _displaymode,
        keyword: searchKeyword);
    if (!mounted) {
      return Future.value(result);
    }
    _memoModelList.addAll(result.rows);
    _total = result.total;
    LogHelper.debug(
        "[Memo] loadData pageNum = $_pageNum,pageSize = $_pageSize,total = $_total,displaymode = $_displaymode,search keyword = $searchKeyword;");
    return Future.value(result);
  }

  void _refreshList() async {
    if (!mounted) {
      return;
    }
    _itemCanMoveAnimation.reverse();
    if (_inPageRefreshing) return;
    _inPageRefreshing = true;
    PageResult? result;
    try {
      resetPageInfo();
      result = await _loadData();
      setState(() {});
    } finally {
      _controller.finishRefresh();
      _controller.resetFooter();
      if (result != null) {
        hasNext = result.hasNext;
      }
      _inPageRefreshing = false;
    }
  }

  void _loadList() async {
    if (!hasNext) {
      _controller.finishLoad(IndicatorResult.noMore);
      return;
    }
    if (!mounted) {
      return;
    }
    _pageNum++;
    PageResult result = await _loadData();
    setState(() {});
    hasNext = result.hasNext;
    if (hasNext) {
      _controller.finishLoad(IndicatorResult.success);
    } else {
      _controller.finishLoad(IndicatorResult.noMore);
    }
  }

  Widget _createMemoItem(MemoModel item, bool showDatetime) {
    return Stack(
      key: Key("${item.id}"),
      children: [
        SpeedDial(
          dialRoot: (context, open, toggleChildren) {
            return Container(
                padding: const EdgeInsets.fromLTRB(5, 5, 5, 5),
                child: GestureDetector(
                    onTap: () async {
                      if (canMove) {
                        canMove = !canMove;
                        setState(() {});
                        return;
                      }
                      preview(item);
                    },
                    onLongPress: canMove ? () => {} : toggleChildren,
                    child: MemoItem(
                      key: Key("${item.id}${item.updatedatetime}"),
                      text: item.content,
                      dateTime: item.datetime,
                      showDatetime: showDatetime,
                      displayMode: DisplayMode.getType(item.displaymode),
                    )));
          },
          direction: SpeedDialDirection.circular,
          children: _getPopupMenuChildren(item),
        ),
      ],
    );
    // );
  }

  void edit(MemoModel item) async {
    if (canMove) {
      canMove = !canMove;
      setState(() {});
      return;
    }
    Set<MemoResult>? result =
        await context.push("/memo/update", extra: MemoParam(memo: item));
    if (result != null) {
      MemoResult item = result.first;
      if (item.delete) {
        _delete(item.memo);
      } else {
        _update(item.memo);
      }
    }
  }

  void preview(MemoModel item) async {
    if (item.displaymode == DisplayMode.image.value) {
      ImageProvider imageProvider;
      if (item.content.startsWith("http")) {
        imageProvider = NetworkImage(item.content);
      } else {
        imageProvider = MemoryImage(base64Decode(item.content));
      }
      showDialog(
          context: context,
          builder: (context) {
            return Material(
              child: Stack(
                children: [
                  PhotoView(
                    imageProvider: imageProvider,
                    loadingBuilder: (context, event) {
                      if (event == null) {
                        return Center(
                          child: Text(AppLocalizations.of(context)!.loading),
                        );
                      }
                      final value = event.cumulativeBytesLoaded /
                          (event.expectedTotalBytes ??
                              event.cumulativeBytesLoaded);
                      final percentage = (100 * value).floor();
                      return Center(
                        child: Text("$percentage%"),
                      );
                    },
                  ),
                  Positioned(
                      right: 30,
                      top: 30,
                      child: IconButton(
                        icon: Icon(
                            color: Theme.of(context).primaryColor, Icons.close),
                        onPressed: () {
                          Navigator.of(context).pop();
                        },
                      )),
                ],
              ),
            );
          });
    } else {
      showDialog(
          context: context,
          builder: (context) {
            return StatefulBuilder(
              builder: (context, setState) {
                return Material(
                  child: Column(
                    children: [
                      Row(
                        mainAxisAlignment: MainAxisAlignment.end,
                        children: [
                          _futureBuilder(setState),
                          IconButton(
                            icon: Icon(
                                color: Theme.of(context).primaryIconTheme.color,
                                _ttsState == TtsState.stopped
                                    ? Icons.play_arrow
                                    : Icons.stop),
                            onPressed: () async {
                              _initTts(setState);
                              if (_ttsState == TtsState.stopped) {
                                await flutterTts.speak(item.content);
                              } else {
                                await flutterTts.stop();
                              }
                            },
                          ),
                          IconButton(
                            icon: Icon(
                                color: Theme.of(context).primaryIconTheme.color,
                                Icons.close),
                            onPressed: () async {
                              await flutterTts.stop();
                              if (mounted) Navigator.of(this.context).pop();
                            },
                          )
                        ],
                      ),
                      Row(children: [
                        Expanded(
                            child: SingleChildScrollView(
                          child: Text(item.content),
                        ))
                      ])
                    ],
                  ),
                );
              },
            );
          });
    }
  }

  List<SpeedDialChild> _getPopupMenuChildren(MemoModel item) {
    List<SpeedDialChild> result = [];
    result.add(SpeedDialChild(
      child: const Icon(Icons.preview),
      onTap: () {
        preview(item);
      },
    ));
    result.add(SpeedDialChild(
      child: const Icon(Icons.edit),
      onTap: () {
        edit(item);
      },
    ));
    if (!searchMode) {
      result.add(SpeedDialChild(
        child: const Icon(Icons.open_with),
        onTap: () {
          canMove = !canMove;
          if (canMove) {
            if (!_itemCanMoveAnimation.isAnimating) {
              _itemCanMoveAnimation.forward();
            }
          }
          setState(() {});
        },
      ));
    }
    if (utf8.encode(item.content).length * 8 + 20 < 23648) {
      result.add(SpeedDialChild(
        child: const Icon(Icons.qr_code),
        onTap: () {
          showDialog(
              context: context,
              builder: (context) {
                var contentSize = utf8.encode(item.content).length * 8 + 20;
                return GestureDetector(
                    onTap: () => {Navigator.of(context).pop()},
                    child: Dialog(
                        child: contentSize < 23648
                            ? QrImageView(data: item.content)
                            : Center(
                                child: Column(
                                    mainAxisAlignment: MainAxisAlignment.center,
                                    children: [
                                    Text(
                                      AppLocalizations.of(context)!
                                          .qrcodeNotShow,
                                      style: Theme.of(context)
                                          .textTheme
                                          .displayMedium,
                                    ),
                                    Text(
                                      AppLocalizations.of(context)!
                                          .qrcodeContentToLong(
                                              item.content.length, 2953),
                                      style: Theme.of(context)
                                          .textTheme
                                          .bodyMedium,
                                    )
                                  ]))));
              });
        },
      ));
    }
    return result;
  }

  void _addMemo() async {
    var now = DateTime.now();
    Set? result = await context.push("/memo/add",
        extra: MemoParam(
            memo: MemoModel(
                content: "",
                datetime: now,
                updatedatetime: now,
                displaymode: _displaymode == null ? 0 : _displaymode!)));
    if (result != null) {
      MemoResult item = result.first;
      _save(item.memo);
    }
  }

  void _scanMemo() async {
    BarcodeScannerViewResult? result = await context.push("/scan");
    if (result != null) {
      List<String> selectedResultBarcode = result.contentList;
      var resultString = "";
      for (int i = 0; i < selectedResultBarcode.length; i++) {
        resultString += "${selectedResultBarcode.elementAt(i)}\n";
      }
      var now = DateTime.now();
      _save(
          MemoModel(content: resultString, datetime: now, updatedatetime: now));
    }
  }

  Widget _getFloatingActionButton() {
    List<Widget> buttons = [];
    if (cameras.isNotEmpty) {
      buttons.add(Padding(
          padding: const EdgeInsets.all(5),
          child: FloatingActionButton(
            heroTag: "scan",
            onPressed: _scanMemo,
            tooltip: AppLocalizations.of(context)!.scan,
            child: const Icon(Icons.scanner),
          )));
    }
    buttons.add(Padding(
        padding: const EdgeInsets.all(5),
        child: FloatingActionButton(
          heroTag: "add",
          onPressed: _addMemo,
          tooltip: AppLocalizations.of(context)!.add,
          child: const Icon(Icons.add),
        )));
    return Row(mainAxisAlignment: MainAxisAlignment.end, children: buttons);
  }

  void _cancelSubscription() {
    memoFilterSubscription?.cancel();
    memoSearchSubscription?.cancel();
  }

  void _setupSubscription() {
    memoFilterSubscription?.cancel();
    memoFilterSubscription =
        AppStore.eventBus.on<MemoFilterChangeEvent>().listen((event) {
      _displaymode = event.displaymode;
      resetPageInfo();
      _refreshList();
    });
    memoSearchSubscription?.cancel();
    memoSearchSubscription =
        AppStore.eventBus.on<MemoSearchChangeEvent>().listen((event) {
      searchKeyword = event.keyword;
      if (searchKeyword != null && searchKeyword!.isNotEmpty) {
        searchMode = true;
      } else {
        searchMode = false;
      }
      _controller.callRefresh();
    });
  }

  @override
  Widget build(BuildContext context) {
    void onReorder(int oldIndex, int newIndex) async {
      var orgList = List<MemoModel>.from(_memoModelList);
      LogHelper.info("oldIndex = $oldIndex,newIndex = $newIndex");
      _memoModelList.insert(newIndex, _memoModelList.removeAt(oldIndex));
      var idAndSeqMap = <String, int>{};
      var minIndex = 0;
      var maxIndex = max(oldIndex, newIndex);
      var list = _memoModelList.sublist(minIndex, maxIndex + 1);
      for (int i = 0; i < list.length; i++) {
        idAndSeqMap[list[i].id!] = i + minIndex;
      }
      try {
        await _sortMemo(_displaymode, idAndSeqMap, minIndex, maxIndex);
      } catch (e) {
        _memoModelList.clear();
        _memoModelList.addAll(orgList);
        setState(() {});
        rethrow;
      }
      setState(() {});
    }

    return Scaffold(
      body: EasyRefresh.builder(
        refreshOnStart: true,
        refreshOnStartHeader: BuilderHeader(
          triggerOffset: 70,
          clamping: true,
          position: IndicatorPosition.above,
          processedDuration: Duration.zero,
          builder: (ctx, state) {
            if (state.mode == IndicatorMode.inactive ||
                state.mode == IndicatorMode.done) {
              return const SizedBox();
            }
            return Container(
              padding: const EdgeInsets.only(bottom: 100),
              width: double.infinity,
              height: state.viewportDimension,
              alignment: Alignment.center,
              child: SpinKitFadingCube(
                size: 24,
                color: Theme.of(context).colorScheme.primary,
              ),
            );
          },
        ),
        controller: _controller,
        header: RefreshHeader(context),
        footer: RefreshFooter(context),
        onRefresh: canMove ? null : _refreshList,
        onLoad: canMove ? null : _loadList,
        childBuilder: (context, physics) {
          if (_memoModelList.isEmpty) {
            return CustomScrollView(
              physics: physics,
              slivers: [
                SliverFillRemaining(
                    child: GestureDetector(
                  child: Column(
                    mainAxisAlignment: MainAxisAlignment.center,
                    mainAxisSize: MainAxisSize.max,
                    children: [
                      SpinKitCubeGrid(
                        size: 80,
                        color: Theme.of(context).colorScheme.primary,
                      ),
                      const SizedBox(height: 16),
                      Text(AppLocalizations.of(context)!.hasNoDataClickRefresh)
                    ],
                  ),
                  onTap: () {
                    _controller.callRefresh();
                  },
                ))
              ],
            );
          } else {
            return CustomScrollView(physics: physics, slivers: [
              SliverReorderableGrid(
                itemBuilder: (context, index) {
                  if (_memoModelList.length > index) {
                    return GestureDetector(
                        key: Key("${_memoModelList[index].id}"),
                        child: ReorderableGridDragStartListener(
                          enabled: canMove,
                          index: index,
                          child: canMove
                              ? RotationTransition(
                                  turns: _animation,
                                  child: _createMemoItem(
                                      _memoModelList[index],
                                      Provider.of<AppStore>(context)
                                          .showDatetime))
                              : _createMemoItem(_memoModelList[index],
                                  Provider.of<AppStore>(context).showDatetime),
                        ));
                  } else {
                    return Container(
                      key: const Key("key"),
                    );
                  }
                },
                itemCount: _memoModelList.length,
                onReorder: onReorder,
                gridDelegate: const SliverGridDelegateWithMaxCrossAxisExtent(
                    maxCrossAxisExtent: 210),
              ),
            ]);
          }
        },
      ),
      bottomNavigationBar:
          _BottomAppBar(_displaymode, widget.memoStore, searchMode),
      floatingActionButton: Semantics(
        sortKey: const OrdinalSortKey(0),
        container: true,
        child: _getFloatingActionButton(),
      ),
      floatingActionButtonLocation: FloatingActionButtonLocation.endDocked,
      resizeToAvoidBottomInset: false,
    );
  }
}

// ignore: must_be_immutable
class _BottomAppBar extends StatelessWidget {
  _BottomAppBar(this._displaymode, this.memoStore, this.searchMode);

  final int? _displaymode;
  final MemoStore memoStore;
  final searchTextController = TextEditingController();
  bool searchMode;

  Icon _getDisplaymodeIcon(int? displaymode) {
    if (displaymode == null) {
      return const Icon(Icons.comment);
    } else {
      return Icon(
          MemoUtil.getIconByDisplayMode(DisplayMode.getType(displaymode)));
    }
  }

  List<SpeedDialChild> _getMemoFilterMenu(BuildContext context) {
    List<SpeedDialChild> result = [];
    result.add(SpeedDialChild(
      child: Tooltip(
        message: AppLocalizations.of(context)!.displayAll,
        child: _getDisplaymodeIcon(null),
      ),
      onTap: () {
        AppStore.eventBus.fire(MemoFilterChangeEvent());
      },
    ));
    for (var element in DisplayMode.values) {
      result.add(SpeedDialChild(
        child: Tooltip(
          message: MemoUtil.getLableByDisplayMode(element, context),
          child: _getDisplaymodeIcon(element.value),
        ),
        onTap: () {
          AppStore.eventBus
              .fire(MemoFilterChangeEvent(displaymode: element.value));
        },
      ));
    }
    return result;
  }

  List<Widget> _createBottomBottom(BuildContext context) {
    List<Widget> result = [];
    result.add(IconButton(
      tooltip: MaterialLocalizations.of(context).openAppDrawerTooltip,
      icon: const Icon(Icons.menu),
      onPressed: () {
        AppStore.eventBus.fire(MenuOpenEvent());
      },
    ));
    result.addAll([
      SpeedDial(
        dialRoot: (context, open, toggleChildren) {
          return badges.Badge(
            badgeContent: Text("${memoStore.memoTotal}"),
            child: IconButton(
              tooltip:
                  "${AppLocalizations.of(context)!.memo}(${memoStore.memoTotal})",
              icon: _getDisplaymodeIcon(_displaymode),
              onPressed: () {
                toggleChildren();
              },
            ),
          );
        },
        direction: SpeedDialDirection.up,
        children: _getMemoFilterMenu(context),
      ),
      IconButton(
        tooltip: AppLocalizations.of(context)!.toggleDatetime,
        icon: Provider.of<AppStore>(context).showDatetime
            ? const Icon(Icons.calendar_month)
            : const Icon(Icons.calendar_today),
        onPressed: () {
          Provider.of<AppStore>(context, listen: false).toggleShowDatetime();
        },
      ),
      IconButton(
        tooltip: AppLocalizations.of(context)!.toggleOnlineMemo,
        icon: Provider.of<AppStore>(context).showOnlineMemo
            ? const Icon(Icons.online_prediction)
            : const Icon(Icons.offline_bolt),
        onPressed: () {
          Provider.of<AppStore>(context, listen: false).toggleShowOnlineMemo();
        },
      ),
      IconButton(
        tooltip: AppLocalizations.of(context)!.search,
        icon: searchMode
            ? const Icon(Icons.saved_search)
            : const Icon(Icons.search),
        onPressed: () {
          showDialog(
              context: context,
              builder: (context) {
                return GestureDetector(
                  onTap: () => {Navigator.of(context).pop()},
                  child: Center(
                      child: Column(children: [
                    Container(
                      margin: const EdgeInsets.fromLTRB(10, 10, 10, 0),
                      decoration: BoxDecoration(
                          color: Theme.of(context).colorScheme.secondary,
                          borderRadius:
                              const BorderRadius.all(Radius.circular(9))),
                      child: TextField(
                        controller: searchTextController,
                        autofocus: true,
                        onSubmitted: (value) {
                          AppStore.eventBus
                              .fire(MemoSearchChangeEvent(keyword: value));
                          searchTextController.clear();
                          context.pop();
                        },
                        decoration: InputDecoration(
                            hintText:
                                AppLocalizations.of(context)!.inputSearchHint,
                            border: const OutlineInputBorder(
                                borderRadius:
                                    BorderRadius.all(Radius.circular(9))),
                            suffixIcon: IconButton(
                              onPressed: () {
                                AppStore.eventBus.fire(MemoSearchChangeEvent(
                                    keyword: searchTextController.text));
                                searchTextController.clear();
                                context.pop();
                              },
                              icon: const Icon(Icons.search),
                            )),
                      ),
                    )
                  ])),
                );
              });
        },
      )
    ]);
    return result;
  }

  @override
  Widget build(BuildContext context) {
    return Semantics(
      sortKey: const OrdinalSortKey(1),
      container: true,
      child: BottomAppBar(
        child: IconTheme(
          data: IconThemeData(color: Theme.of(context).colorScheme.primary),
          child: Row(
            children: _createBottomBottom(context),
          ),
        ),
      ),
    );
  }
}

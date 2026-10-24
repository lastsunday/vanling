// ignore: unused_import
import 'package:intl/intl.dart' as intl;

import 'app_localizations.dart';

// ignore_for_file: type=lint

/// The translations for Chinese (`zh`).
class AppLocalizationsZh extends AppLocalizations {
  AppLocalizationsZh([String locale = 'zh']) : super(locale);

  @override
  String get memo => '备忘录';

  @override
  String get updateMemo => '更新备忘录';

  @override
  String get addMemo => '添加备忘录';

  @override
  String get word => '字';

  @override
  String get displayMode => '显示模式';

  @override
  String get displayModeAuto => '自动';

  @override
  String get displayModeText => '文本';

  @override
  String get displayModeImage => '图片';

  @override
  String get displayAll => '全部';

  @override
  String get imageConvertFailure => '图片解析异常';

  @override
  String get convertToBase64 => '转换为Base64';

  @override
  String get toggleDatetime => '切换日期显示';

  @override
  String get toggleOnlineMemo => '切换在线备忘录';

  @override
  String get selectFile => '请选择文件';

  @override
  String get importFile => '导入文件';

  @override
  String get setting => '设置';

  @override
  String get scan => '扫描';

  @override
  String get add => '添加';

  @override
  String get confirm => '确定';

  @override
  String get confirmDelete => '确定删除？';

  @override
  String get cancel => '取消';

  @override
  String get delete => '删除';

  @override
  String get save => '保存';

  @override
  String get about => '关于';

  @override
  String get version => '版本';

  @override
  String get newVersionDownload => '新版本下载中';

  @override
  String get newVersionUpdate => '新版本更新';

  @override
  String get versionUpToDate => '已是最新版本';

  @override
  String get newVersion => '新版本';

  @override
  String get planTime => '预计耗时';

  @override
  String get inputContentHint => '请输入内容';

  @override
  String get qrcodeNotShow => '二维码未能显示';

  @override
  String qrcodeContentToLong(int current, int max) {
    return '文本过长($current/$max 字)';
  }

  @override
  String get settingsTextScaling => '文字缩放';

  @override
  String get settingsTextScalingSmall => '小';

  @override
  String get settingsTextScalingNormal => '正常';

  @override
  String get settingsTextScalingLarge => '大';

  @override
  String get settingsTextScalingHuge => '超大';

  @override
  String get settingsTheme => '主题背景';

  @override
  String get settingsDarkTheme => '深色';

  @override
  String get settingsLightTheme => '浅色';

  @override
  String get settingsSystemDefault => '系统';

  @override
  String get settingsLocale => '语言区域';

  @override
  String get login => '登录';

  @override
  String get pleaseLogin => '请登录';

  @override
  String get areYouSureLogout => '确认登出？';

  @override
  String get account => '账号';

  @override
  String get password => '密码';

  @override
  String get loginSuccess => '登录成功';

  @override
  String get loginFailure => '登录失败';

  @override
  String get loginTimeout => '登录超时';

  @override
  String get loginNotOpen => '登录功能暂未开放';

  @override
  String get loadMore => '上拉加载更多...';

  @override
  String get loadFail => '加载失败，请重试';

  @override
  String get releaseToLoad => '松开加载';

  @override
  String get hasNoData => '没有数据了';

  @override
  String get hasNoDataClickRefresh => '没有数据，点击刷新';

  @override
  String get loading => '加载中 ...';

  @override
  String get successful => '加载成功';

  @override
  String get scrollDownToRefresh => '下拉刷新';

  @override
  String get serverError => '服务异常';

  @override
  String get search => '搜索';

  @override
  String get inputSearchHint => '输入搜索词';

  @override
  String get locationServiceDisabled => '定位服务未开启.';

  @override
  String get locationPermissionDenied => '定位服务没有授权.';

  @override
  String get locationPermissionDeniedForever => '定位服务被禁止.';

  @override
  String get runningInBackgroundTip => '应用会在后台轮询定位信息';

  @override
  String get runningInBackground => '后台运行';

  @override
  String get deviceId => '设备编号';

  @override
  String get copyDeviceId => '已复制设备编号到粘贴板';
}

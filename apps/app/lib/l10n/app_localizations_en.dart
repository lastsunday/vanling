// ignore: unused_import
import 'package:intl/intl.dart' as intl;

import 'app_localizations.dart';

// ignore_for_file: type=lint

/// The translations for English (`en`).
class AppLocalizationsEn extends AppLocalizations {
  AppLocalizationsEn([String locale = 'en']) : super(locale);

  @override
  String get memo => 'Memo';

  @override
  String get updateMemo => 'Update Memo';

  @override
  String get addMemo => 'Add Memo';

  @override
  String get word => 'Word';

  @override
  String get displayMode => 'Display Mode';

  @override
  String get displayModeAuto => 'Auto';

  @override
  String get displayModeText => 'Text';

  @override
  String get displayModeImage => 'Image';

  @override
  String get displayAll => 'All';

  @override
  String get imageConvertFailure => 'Image Convert Failure';

  @override
  String get convertToBase64 => 'Convert To Base64';

  @override
  String get toggleDatetime => 'Toggle Datetime';

  @override
  String get toggleOnlineMemo => 'Toggle Online Memo';

  @override
  String get selectFile => 'Please select file';

  @override
  String get importFile => 'Import file';

  @override
  String get setting => 'Setting';

  @override
  String get scan => 'Scan';

  @override
  String get add => 'Add';

  @override
  String get confirm => 'Confirm';

  @override
  String get confirmDelete => 'Delete this item?';

  @override
  String get cancel => 'Cancel';

  @override
  String get delete => 'Delete';

  @override
  String get save => 'Save';

  @override
  String get about => 'About';

  @override
  String get version => 'Version';

  @override
  String get newVersionDownload => 'New Version Downloading';

  @override
  String get newVersionUpdate => 'New Version Update';

  @override
  String get versionUpToDate => 'Up To Date';

  @override
  String get newVersion => 'New';

  @override
  String get planTime => 'Estimated time';

  @override
  String get inputContentHint => 'Please type something';

  @override
  String get qrcodeNotShow => 'The qrcode not show';

  @override
  String qrcodeContentToLong(int current, int max) {
    return 'Text to long($current/$max Words)';
  }

  @override
  String get settingsTextScaling => 'Text scaling';

  @override
  String get settingsTextScalingSmall => 'Small';

  @override
  String get settingsTextScalingNormal => 'Normal';

  @override
  String get settingsTextScalingLarge => 'Large';

  @override
  String get settingsTextScalingHuge => 'Huge';

  @override
  String get settingsTheme => 'Theme';

  @override
  String get settingsDarkTheme => 'Dark';

  @override
  String get settingsLightTheme => 'Light';

  @override
  String get settingsSystemDefault => 'System';

  @override
  String get settingsLocale => 'Locale';

  @override
  String get login => 'Login';

  @override
  String get pleaseLogin => 'Please login';

  @override
  String get areYouSureLogout => 'Are you sure logout?';

  @override
  String get account => 'Account';

  @override
  String get password => 'Password';

  @override
  String get loginSuccess => 'Login success';

  @override
  String get loginFailure => 'Login Failure';

  @override
  String get loginTimeout => 'Login Timeout';

  @override
  String get loginNotOpen => 'Login feature not open';

  @override
  String get loadMore => 'Scroll up to load more...';

  @override
  String get loadFail => 'Loading failed, please try again';

  @override
  String get releaseToLoad => 'Release to loading';

  @override
  String get hasNoData => 'No data';

  @override
  String get hasNoDataClickRefresh => 'No data,click to refresh';

  @override
  String get loading => 'Loading ...';

  @override
  String get successful => 'Load successful';

  @override
  String get scrollDownToRefresh => 'Scroll down to refresh';

  @override
  String get serverError => 'Service Exception';

  @override
  String get search => 'Search';

  @override
  String get inputSearchHint => 'Enter your search term';

  @override
  String get locationServiceDisabled => 'Location services are disabled.';

  @override
  String get locationPermissionDenied => 'Location permissions are denied.';

  @override
  String get locationPermissionDeniedForever =>
      'Location permissions are permanently denied, we cannot request permissions.';

  @override
  String get runningInBackgroundTip =>
      'App will continue to receive your location even when you aren\'t using it';

  @override
  String get runningInBackground => 'Running in Background';

  @override
  String get deviceId => 'Device Id';

  @override
  String get copyDeviceId => 'Copied device id to Clipboard';
}

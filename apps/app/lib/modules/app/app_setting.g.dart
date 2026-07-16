// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'app_setting.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

AppSetting _$AppSettingFromJson(Map<String, dynamic> json) => AppSetting(
  themeMode: $enumDecode(_$ThemeModeEnumMap, json['themeMode']),
  platform: $enumDecodeNullable(_$TargetPlatformEnumMap, json['platform']),
  textScaleFactorValue: (json['textScaleFactorValue'] as num).toDouble(),
  timeDilation: (json['timeDilation'] as num).toDouble(),
  localeValue: json['localeValue'] as String,
);

Map<String, dynamic> _$AppSettingToJson(AppSetting instance) =>
    <String, dynamic>{
      'themeMode': _$ThemeModeEnumMap[instance.themeMode]!,
      'platform': _$TargetPlatformEnumMap[instance.platform],
      'textScaleFactorValue': instance.textScaleFactorValue,
      'timeDilation': instance.timeDilation,
      'localeValue': instance.localeValue,
    };

const _$ThemeModeEnumMap = {
  ThemeMode.system: 'system',
  ThemeMode.light: 'light',
  ThemeMode.dark: 'dark',
};

const _$TargetPlatformEnumMap = {
  TargetPlatform.android: 'android',
  TargetPlatform.fuchsia: 'fuchsia',
  TargetPlatform.iOS: 'iOS',
  TargetPlatform.linux: 'linux',
  TargetPlatform.macOS: 'macOS',
  TargetPlatform.windows: 'windows',
};

import 'dart:collection';

import 'package:app/l10n/app_localizations.dart';
import 'package:collection/collection.dart';
import 'package:flutter/material.dart';
import 'package:flutter_localized_locales/flutter_localized_locales.dart';
import 'package:app/modules/app/app_setting.dart';
import 'package:app/modules/app/app_store.dart';
import 'package:app/modules/pages/widget/settings_list_item.dart';
import 'package:provider/provider.dart';

import 'package:app/constants.dart';

enum _ExpandableSetting { textScale, theme, locale }

class SettingPage extends StatefulWidget {
  const SettingPage({super.key});

  @override
  State<SettingPage> createState() => _SettingPageState();
}

class _SettingPageState extends State<SettingPage> {
  _ExpandableSetting? _expandedSettingId;

  void onTapSetting(_ExpandableSetting settingId) {
    setState(() {
      if (_expandedSettingId == settingId) {
        _expandedSettingId = null;
      } else {
        _expandedSettingId = settingId;
      }
    });
  }

  void _updateSetting(BuildContext context, AppSetting appSetting) {
    AppSetting.update(context, appSetting);
    Provider.of<AppStore>(context, listen: false).updateAppSetting(appSetting);
  }

  /// Given a [Locale], returns a [DisplayOption] with its native name for a
  /// title and its name in the currently selected locale for a subtitle. If the
  /// native name can't be determined, it is omitted. If the locale can't be
  /// determined, the locale code is used.
  DisplayOption _getLocaleDisplayOption(BuildContext context, Locale? locale) {
    final localeCode = locale.toString();
    final localeName = LocaleNames.of(context)!.nameOf(localeCode);
    if (localeName != null) {
      final localeNativeName =
          LocaleNamesLocalizationsDelegate.nativeLocaleNames[localeCode];
      return localeNativeName != null
          ? DisplayOption(localeNativeName, subtitle: localeName)
          : DisplayOption(localeName);
    } else {
      // gsw, fil, and es_419 aren't in flutter_localized_countries' dataset
      // so we handle them separately
      switch (localeCode) {
        case 'gsw':
          return DisplayOption('Schwiizertüütsch', subtitle: 'Swiss German');
        case 'fil':
          return DisplayOption('Filipino', subtitle: 'Filipino');
        case 'es_419':
          return DisplayOption(
            'español (Latinoamérica)',
            subtitle: 'Spanish (Latin America)',
          );
      }
    }

    return DisplayOption(localeCode);
  }

  /// Create a sorted — by native name – map of supported locales to their
  /// intended display string, with a system option as the first element.
  LinkedHashMap<Locale, DisplayOption> _getLocaleOptions() {
    var localeOptions = LinkedHashMap.of({
      AppSetting.systemLocaleOption: DisplayOption(
        AppLocalizations.of(context)!.settingsSystemDefault +
            (deviceLocale != null
                ? ' - ${_getLocaleDisplayOption(context, deviceLocale).title}'
                : ''),
      ),
    });
    var supportedLocales = List<Locale>.from(AppLocalizations.supportedLocales);
    supportedLocales.removeWhere((locale) => locale == deviceLocale);

    final displayLocales =
        Map<Locale, DisplayOption>.fromIterable(
          supportedLocales,
          value: (dynamic locale) =>
              _getLocaleDisplayOption(context, locale as Locale?),
        ).entries.toList()..sort(
          (l1, l2) => compareAsciiUpperCase(l1.value.title, l2.value.title),
        );

    localeOptions.addAll(LinkedHashMap.fromEntries(displayLocales));
    return localeOptions;
  }

  @override
  Widget build(BuildContext context) {
    final options = AppSetting.of(context);
    final localizations = AppLocalizations.of(context)!;

    final settingsListItems = [
      SettingsListItem<double?>(
        title: AppLocalizations.of(context)!.settingsTextScaling,
        selectedOption: options.textScaleFactor(context, useSentinel: true),
        optionsMap: LinkedHashMap.of({
          systemTextScaleFactorOption: DisplayOption(
            // localizations.settingsSystemDefault,
            AppLocalizations.of(context)!.settingsSystemDefault,
          ),
          0.8: DisplayOption(
            // localizations.settingsTextScalingSmall,
            AppLocalizations.of(context)!.settingsTextScalingSmall,
          ),
          1.0: DisplayOption(
            // localizations.settingsTextScalingNormal,
            AppLocalizations.of(context)!.settingsTextScalingNormal,
          ),
          2.0: DisplayOption(
            // localizations.settingsTextScalingLarge,
            AppLocalizations.of(context)!.settingsTextScalingLarge,
          ),
          3.0: DisplayOption(
            // localizations.settingsTextScalingHuge,
            AppLocalizations.of(context)!.settingsTextScalingHuge,
          ),
        }),
        onOptionChanged: (newTextScale) => {
          _updateSetting(
            context,
            options.copyWith(textScaleFactor: newTextScale),
          ),
        },
        onTapSetting: () => onTapSetting(_ExpandableSetting.textScale),
        isExpanded: _expandedSettingId == _ExpandableSetting.textScale,
      ),
      SettingsListItem<ThemeMode?>(
        title: AppLocalizations.of(context)!.settingsTheme,
        selectedOption: options.themeMode,
        optionsMap: LinkedHashMap.of({
          ThemeMode.system: DisplayOption(
            AppLocalizations.of(context)!.settingsSystemDefault,
          ),
          ThemeMode.dark: DisplayOption(
            AppLocalizations.of(context)!.settingsDarkTheme,
          ),
          ThemeMode.light: DisplayOption(
            AppLocalizations.of(context)!.settingsLightTheme,
          ),
        }),
        onOptionChanged: (newThemeMode) =>
            _updateSetting(context, options.copyWith(themeMode: newThemeMode)),
        onTapSetting: () => onTapSetting(_ExpandableSetting.theme),
        isExpanded: _expandedSettingId == _ExpandableSetting.theme,
      ),
      SettingsListItem<Locale?>(
        title: localizations.settingsLocale,
        selectedOption: options.locale == deviceLocale
            ? AppSetting.systemLocaleOption
            : options.locale,
        optionsMap: _getLocaleOptions(),
        onOptionChanged: (newLocale) => _updateSetting(
          context,
          options.copyWith(locale: newLocale!.toString()),
        ),
        onTapSetting: () => onTapSetting(_ExpandableSetting.locale),
        isExpanded: _expandedSettingId == _ExpandableSetting.locale,
      ),
    ];
    return Scaffold(
      appBar: AppBar(title: Text(AppLocalizations.of(context)!.setting)),
      body: Padding(
        padding: const EdgeInsets.fromLTRB(0, 25, 0, 0),
        child: SingleChildScrollView(
          child: Column(children: settingsListItems),
        ),
      ),
    );
  }
}

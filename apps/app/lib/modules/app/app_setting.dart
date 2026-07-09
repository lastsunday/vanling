// Applies text GalleryOptions to a widget
import 'dart:async';

import 'package:flutter/material.dart';
import 'package:flutter/scheduler.dart' show timeDilation;
import 'package:flutter/services.dart';
import 'package:json_annotation/json_annotation.dart';

import 'package:app/constants.dart';

part 'app_setting.g.dart';

Locale? _deviceLocale;

Locale? get deviceLocale => _deviceLocale;

set deviceLocale(Locale? locale) {
  _deviceLocale ??= locale;
}

@JsonSerializable()
class AppSetting {
  AppSetting({
    required this.themeMode,
    required this.platform,
    required this.textScaleFactorValue,
    required this.timeDilation,
    required this.localeValue,
  }) {
    _textScaleFactor = textScaleFactorValue;
    if (localeValue != AppSetting.systemLocaleOption.toString()) {
      _locale = Locale(localeValue);
    }
  }

  static const systemLocaleOption = Locale('system');

  final ThemeMode themeMode;
  final TargetPlatform? platform;
  late double _textScaleFactor;
  final double textScaleFactorValue;
  final double timeDilation;
  Locale? _locale;
  final String localeValue;

  factory AppSetting.fromJson(Map<String, dynamic> json) =>
      _$AppSettingFromJson(json);

  Map<String, dynamic> toJson() => _$AppSettingToJson(this);

  // We use a sentinel value to indicate the system text scale option. By
  // default, return the actual text scale factor, otherwise return the
  // sentinel value.
  double textScaleFactor(BuildContext context, {bool useSentinel = false}) {
    if (_textScaleFactor == systemTextScaleFactorOption) {
      return useSentinel
          ? systemTextScaleFactorOption
          : MediaQuery.textScalerOf(context).scale(1.0);
    } else {
      return _textScaleFactor;
    }
  }

  Locale? get locale => _locale ?? deviceLocale;

  /// Returns a [SystemUiOverlayStyle] based on the [ThemeMode] setting.
  /// In other words, if the theme is dark, returns light; if the theme is
  /// light, returns dark.
  SystemUiOverlayStyle resolvedSystemUiOverlayStyle() {
    Brightness brightness;
    switch (themeMode) {
      case ThemeMode.light:
        brightness = Brightness.light;
        break;
      case ThemeMode.dark:
        brightness = Brightness.dark;
        break;
      default:
        brightness =
            WidgetsBinding.instance.platformDispatcher.platformBrightness;
    }

    final overlayStyle = brightness == Brightness.dark
        ? SystemUiOverlayStyle.light
        : SystemUiOverlayStyle.dark;

    return overlayStyle;
  }

  AppSetting copyWith({
    ThemeMode? themeMode,
    TargetPlatform? platform,
    double? textScaleFactor,
    double? timeDilation,
    String? locale,
  }) {
    return AppSetting(
      themeMode: themeMode ?? this.themeMode,
      platform: platform ?? this.platform,
      textScaleFactorValue: textScaleFactor ?? _textScaleFactor,
      timeDilation: timeDilation ?? this.timeDilation,
      localeValue: locale ?? localeValue,
    );
  }

  @override
  bool operator ==(Object other) =>
      other is AppSetting &&
      themeMode == other.themeMode &&
      platform == other.platform &&
      _textScaleFactor == other._textScaleFactor &&
      timeDilation == other.timeDilation &&
      localeValue == other.localeValue;

  @override
  int get hashCode => Object.hash(
        themeMode,
        platform,
        _textScaleFactor,
        timeDilation,
        localeValue,
      );

  static AppSetting of(BuildContext context) {
    final scope =
        context.dependOnInheritedWidgetOfExactType<_ModelBindingScope>()!;
    return scope.modelBindingState.currentModel;
  }

  static void update(BuildContext context, AppSetting newModel) {
    final scope =
        context.dependOnInheritedWidgetOfExactType<_ModelBindingScope>()!;
    scope.modelBindingState.updateModel(newModel);
  }
}

class ApplyTextOptions extends StatelessWidget {
  const ApplyTextOptions({
    super.key,
    required this.child,
  });

  final Widget child;

  @override
  Widget build(BuildContext context) {
    final options = AppSetting.of(context);
    // final textDirection = options.resolvedTextDirection();
    final textScaleFactor = options.textScaleFactor(context);

    Widget widget = MediaQuery(
      data: MediaQuery.of(context).copyWith(
        textScaler: TextScaler.linear(textScaleFactor),
      ),
      child: child,
    );
    // return textDirection == null
    //     ? widget
    //     : Directionality(
    //         textDirection: textDirection,
    //         child: widget,
    //       );
    return widget;
  }
}

// Everything below is boilerplate except code relating to time dilation.
// See https://medium.com/flutter/managing-flutter-application-state-with-inheritedwidgets-1140452befe1

class _ModelBindingScope extends InheritedWidget {
  const _ModelBindingScope({
    required this.modelBindingState,
    required super.child,
  });

  final _ModelBindingState modelBindingState;

  @override
  bool updateShouldNotify(_ModelBindingScope oldWidget) => true;
}

class ModelBinding extends StatefulWidget {
  const ModelBinding({
    super.key,
    required this.initialModel,
    required this.child,
  });

  final AppSetting initialModel;
  final Widget child;

  @override
  State<ModelBinding> createState() => _ModelBindingState();
}

class _ModelBindingState extends State<ModelBinding> {
  late AppSetting currentModel;
  Timer? _timeDilationTimer;

  @override
  void initState() {
    super.initState();
    currentModel = widget.initialModel;
  }

  @override
  void dispose() {
    _timeDilationTimer?.cancel();
    _timeDilationTimer = null;
    super.dispose();
  }

  void handleTimeDilation(AppSetting newModel) {
    if (currentModel.timeDilation != newModel.timeDilation) {
      _timeDilationTimer?.cancel();
      _timeDilationTimer = null;
      if (newModel.timeDilation > 1) {
        // We delay the time dilation change long enough that the user can see
        // that UI has started reacting and then we slam on the brakes so that
        // they see that the time is in fact now dilated.
        _timeDilationTimer = Timer(const Duration(milliseconds: 150), () {
          timeDilation = newModel.timeDilation;
        });
      } else {
        timeDilation = newModel.timeDilation;
      }
    }
  }

  void updateModel(AppSetting newModel) {
    if (newModel != currentModel) {
      handleTimeDilation(newModel);
      setState(() {
        currentModel = newModel;
      });
    }
  }

  @override
  Widget build(BuildContext context) {
    return _ModelBindingScope(
      modelBindingState: this,
      child: widget.child,
    );
  }
}

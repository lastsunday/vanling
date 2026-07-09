// Copyright 2019 The Flutter team. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

import 'package:flutter/material.dart';

class AppThemeData {
  static const _lightFillColor = Colors.black;
  static const _darkFillColor = Colors.white;

  static final Color _lightFocusColor = Colors.black.withValues(alpha: 0.12);
  static final Color _darkFocusColor = Colors.white.withValues(alpha: 0.12);

  static ThemeData lightThemeData =
      themeData(lightColorScheme, _lightFocusColor, lightListTileThemeData);
  static ThemeData darkThemeData =
      themeData(darkColorScheme, _darkFocusColor, darkListTileThemeData);

  static ThemeData themeData(ColorScheme colorScheme, Color focusColor,
      ListTileThemeData listTileThemeData) {
    return ThemeData(
      colorScheme: colorScheme,
      textTheme: _textTheme,
      appBarTheme: AppBarTheme(
        backgroundColor: colorScheme.surface,
        elevation: 0,
        iconTheme: IconThemeData(color: colorScheme.primary),
      ),
      iconTheme: IconThemeData(color: colorScheme.onPrimary),
      canvasColor: colorScheme.surface,
      scaffoldBackgroundColor: colorScheme.surface,
      highlightColor: Colors.transparent,
      focusColor: focusColor,
      snackBarTheme: SnackBarThemeData(
        behavior: SnackBarBehavior.floating,
        backgroundColor: Color.alphaBlend(
          _lightFillColor.withValues(alpha: 0.80),
          _darkFillColor,
        ),
        contentTextStyle: _textTheme.titleMedium!.apply(color: _darkFillColor),
      ),
      listTileTheme: listTileThemeData,
    );
  }

  static const ColorScheme lightColorScheme = ColorScheme(
    primary: Color(0xFFB93C5D),
    primaryContainer: Color(0xFF117378),
    secondary: Color(0xFFEFF3F3),
    secondaryContainer: Color(0xFFFAFBFB),
    surface: Color(0xFFE6EBEB),
    error: _lightFillColor,
    onError: _lightFillColor,
    onPrimary: _lightFillColor,
    onSecondary: Color(0xFF322942),
    onSurface: Color(0xFF241E30),
    brightness: Brightness.light,
  );

  static const ListTileThemeData lightListTileThemeData = ListTileThemeData();

  static const ColorScheme darkColorScheme = ColorScheme(
    primary: Color(0xFFFF8383),
    primaryContainer: Color(0xFF1CDEC9),
    secondary: Color(0xFF4D1F7C),
    secondaryContainer: Color(0xFF451B6F),
    surface: Color(0xFF241E30),
    error: _darkFillColor,
    onError: _darkFillColor,
    onPrimary: _darkFillColor,
    onSecondary: _darkFillColor,
    onSurface: _darkFillColor,
    brightness: Brightness.dark,
  );

  static const ListTileThemeData darkListTileThemeData = ListTileThemeData();

  // static const _regular = FontWeight.w400;
  // static const _medium = FontWeight.w500;
  // static const _semiBold = FontWeight.w600;
  // static const _bold = FontWeight.w700;

  static const TextTheme _textTheme = TextTheme(
    headlineMedium: TextStyle(fontWeight: _bold, fontSize: 20.0),
    bodySmall: TextStyle(fontWeight: _semiBold, fontSize: 16.0),
    headlineSmall: TextStyle(fontWeight: _medium, fontSize: 16.0),
    titleMedium: TextStyle(fontWeight: _medium, fontSize: 16.0),
    labelSmall: TextStyle(fontWeight: _medium, fontSize: 12.0),
    bodyLarge: TextStyle(fontWeight: _regular, fontSize: 24.0),
    titleSmall: TextStyle(fontWeight: _medium, fontSize: 14.0),
    bodyMedium: TextStyle(fontWeight: _regular, fontSize: 16.0),
    titleLarge: TextStyle(fontWeight: _bold, fontSize: 24.0),
    labelLarge: TextStyle(fontWeight: _semiBold, fontSize: 14.0),
  );

  static const _regular = FontWeight.w400;
  static const _medium = FontWeight.w500;
  static const _semiBold = FontWeight.w600;
  static const _bold = FontWeight.w700;

  // static final TextTheme _textTheme = TextTheme(
  //   headlineMedium: GoogleFonts.montserrat(fontWeight: _bold, fontSize: 20.0),
  //   bodySmall: GoogleFonts.oswald(fontWeight: _semiBold, fontSize: 16.0),
  //   headlineSmall: GoogleFonts.oswald(fontWeight: _medium, fontSize: 16.0),
  //   titleMedium: GoogleFonts.montserrat(fontWeight: _medium, fontSize: 16.0),
  //   labelSmall: GoogleFonts.montserrat(fontWeight: _medium, fontSize: 12.0),
  //   bodyLarge: GoogleFonts.montserrat(fontWeight: _regular, fontSize: 24.0),
  //   titleSmall: GoogleFonts.montserrat(fontWeight: _medium, fontSize: 14.0),
  //   bodyMedium: GoogleFonts.montserrat(fontWeight: _regular, fontSize: 16.0),
  //   titleLarge: GoogleFonts.montserrat(fontWeight: _bold, fontSize: 24.0),
  //   labelLarge: GoogleFonts.montserrat(fontWeight: _semiBold, fontSize: 14.0),
  // );
}

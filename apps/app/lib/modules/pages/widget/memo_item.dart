import 'dart:convert';

import 'package:app/l10n/app_localizations.dart';
import 'package:flutter/material.dart';
import 'package:app/core/util/date_util.dart';
import 'package:app/modules/app/model/memo_model.dart';

// ignore: must_be_immutable
class MemoItem extends StatelessWidget {
  MemoItem({
    super.key,
    required this.text,
    required this.dateTime,
    this.displayMode = DisplayMode.auto,
    this.showDatetime = false,
  });

  String text;
  DateTime dateTime;
  DisplayMode displayMode;
  bool showDatetime;

  Widget _getContentDisplay(
      DisplayMode displayMode, String text, BuildContext context) {
    if (displayMode.value == DisplayMode.image.value) {
      if (text.startsWith("http")) {
        return ConstrainedBox(
            constraints: const BoxConstraints.expand(),
            child: Image.network(
              text,
              fit: BoxFit.cover,
              alignment: Alignment.topCenter,
              gaplessPlayback: true,
              errorBuilder: (context, error, stackTrace) => const Column(
                mainAxisAlignment: MainAxisAlignment.center,
                crossAxisAlignment: CrossAxisAlignment.stretch,
                children: [
                  Icon(
                    Icons.broken_image,
                    size: 100,
                  )
                ],
              ),
            ));
      } else {
        Widget convertFailureWidgt = Material(
            child: TextFormField(
          decoration: InputDecoration(
            contentPadding: const EdgeInsets.all(5),
            border: const OutlineInputBorder(borderSide: BorderSide.none),
            disabledBorder:
                const OutlineInputBorder(borderSide: BorderSide.none),
            fillColor: Theme.of(context).colorScheme.primary,
            filled: true,
          ),
          style: Theme.of(context).textTheme.bodyMedium,
          enabled: false,
          expands: true,
          textAlignVertical: TextAlignVertical.top,
          maxLines: null,
          initialValue: AppLocalizations.of(context)!.imageConvertFailure,
        ));
        try {
          return ConstrainedBox(
              constraints: const BoxConstraints.expand(),
              child: Image.memory(
                const Base64Decoder().convert(text),
                errorBuilder: (context, error, stackTrace) =>
                    convertFailureWidgt,
                fit: BoxFit.cover,
                alignment: Alignment.topCenter,
                gaplessPlayback: true,
              ));
        } catch (e) {
          return convertFailureWidgt;
        }
      }
    } else {
      var targetText = text;
      if (targetText.length > 150) {
        targetText = text.substring(0, 150);
        targetText += "...";
      }
      return Material(
          child: TextFormField(
        decoration: InputDecoration(
          contentPadding: const EdgeInsets.all(5),
          border: const OutlineInputBorder(borderSide: BorderSide.none),
          disabledBorder: const OutlineInputBorder(borderSide: BorderSide.none),
          fillColor: Theme.of(context).colorScheme.secondary,
          filled: true,
        ),
        style: Theme.of(context).textTheme.bodyMedium,
        enabled: false,
        expands: true,
        textAlignVertical: TextAlignVertical.top,
        maxLines: null,
        initialValue: targetText,
      ));
    }
  }

  @override
  Widget build(BuildContext context) {
    return ClipRRect(
      borderRadius: BorderRadius.circular(10),
      child: Stack(children: [
        _getContentDisplay(displayMode, text, context),
        (showDatetime
            ? Positioned(
                left: 0,
                right: 0,
                bottom: 0,
                child: Container(
                    padding: const EdgeInsets.only(right: 5),
                    color: Colors.black.withValues(alpha: 0.6),
                    child: Text(
                      DateUtil.format(dateTime),
                      style: TextStyle(color: Colors.white.withValues(alpha: 1)),
                      textAlign: TextAlign.end,
                    )))
            : Container()),
        const Positioned(top: 0, right: 0, bottom: 0, left: 0, child: Text(""))
      ]),
    );
  }
}

import 'package:app/l10n/app_localizations.dart';
import 'package:easy_refresh/easy_refresh.dart';
import 'package:flutter/material.dart';

class RefreshFooter extends ClassicFooter {
  RefreshFooter(BuildContext context)
    : super(
        dragText: AppLocalizations.of(context)!.loadMore,
        armedText: AppLocalizations.of(context)!.releaseToLoad,
        readyText: AppLocalizations.of(context)!.loading,
        processingText: AppLocalizations.of(context)!.loading,
        processedText: AppLocalizations.of(context)!.successful,
        failedText: AppLocalizations.of(context)!.loadFail,
        noMoreText: AppLocalizations.of(context)!.hasNoData,
        showMessage: false,
      );
}

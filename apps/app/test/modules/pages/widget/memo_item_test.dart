import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:app/modules/pages/widget/memo_item.dart';

void main() {
  // Define a test. The TestWidgets function also provides a WidgetTester
  // to work with. The WidgetTester allows building and interacting
  // with widgets in the test environment.
  testWidgets('MemoItem has a text and date', (tester) async {
    // Create the widget by telling the tester to build it.
    await tester.pumpWidget(MaterialApp(
        home: Scaffold(
      body: MemoItem(text: "T", dateTime: DateTime(2017, 9, 7, 17, 30), showDatetime: true),
    )));
    // Create the Finders.
    final textFinder = find.text('T');
    final dateFinder = find.text('2017-09-07 17:30:00');

    // Use the `findsOneWidget` matcher provided by flutter_test to
    // verify that the Text widgets appear exactly once in the widget tree.
    expect(textFinder, findsOneWidget);
    expect(dateFinder, findsOneWidget);
  });
}

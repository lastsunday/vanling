import 'package:flutter_test/flutter_test.dart';
import 'package:app/core/iterable_extension.dart';

void main() {
  group('Should execute `reduceOr`', () {
    test('Should throw when reduce empty list using reduce', () {
      expect(
        () => [].reduce((value, element) => value += element),
        throwsA(isA<StateError>()),
      );
    });

    test('Should reduce empty list with default value', () {
      expect([].reduceOr((value, element) => value += element, 0), 0);
    });

    test('Should reduce empty list', () {
      expect([1].reduceOr((value, element) => value += element, 0), 1);
    });

    test('Should reduce empty list', () {
      expect([1, 2].reduceOr((value, element) => value += element, 0), 3);
    });
  });

  group('Should execute `first`', () {
    test('Should throw when get first in empty list using first', () {
      expect(() => [].first, throwsA(isA<StateError>()));
    });

    test('Should get default when get firstOr in empty list using firstOr', () {
      expect([].firstOr(0), 0);
    });

    test('Should get first when get firstOr in list using firstOr', () {
      expect([1, 2].firstOr(0), 1);
    });

    test(
      'Should get null when get first in empty list using firstNullable',
      () {
        expect([].firstNullable, null);
      },
    );

    test('Should get first when get first in list using firstNullable', () {
      expect([1].firstNullable, 1);
    });
  });

  group('Should execute `allMatch`', () {
    test('Should return true when all match', () {
      expect([1, 2, 3].allMatch((element) => element > 0), true);
    });

    test('Should return false when not all match', () {
      expect([1, 2, 3].allMatch((element) => element > 1), false);
    });
  });
}

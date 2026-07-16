class GlobalTime {
  static DateTime Function() _currentTimeGetter = () => DateTime.now();

  static DateTime now() {
    return _currentTimeGetter();
  }

  static void reset([String? timeString]) {
    _currentTimeGetter = timeString == null
        ? () => DateTime.now()
        : () => DateTime.parse(timeString);
  }
}

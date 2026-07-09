import 'package:logging/logging.dart';
import 'package:logging_appenders/logging_appenders.dart';

class LogHelper {
  static final _logger = Logger('LogHelper');

  static void debug(String message) {
    _logger.fine(message);
  }

  static void err(String message, Object error) {
    _logger.severe(message, "$error");
  }

  static void info(String message) {
    _logger.info(message);
  }

  static void warning(String message) {
    _logger.warning(message);
  }

  static void verbose(String message) {
    _logger.finer(message);
  }

  static Logger get log => _logger;

  static void setLevel(Level level) {
    PrintAppender.setupLogging(level: level);
  }
}

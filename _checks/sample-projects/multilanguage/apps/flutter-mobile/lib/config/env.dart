// ignore_for_file: constant_identifier_names

/// Baked in at build time: `flutter run --dart-define-from-file=../../.env`
/// All values are public — the binary can be inspected.
abstract class Env {
  static const String PUBLIC_FLUTTER_API_URL = String.fromEnvironment(
    'PUBLIC_FLUTTER_API_URL',
  );
  static const String PUBLIC_FLUTTER_APP_NAME = String.fromEnvironment(
    'PUBLIC_FLUTTER_APP_NAME',
    defaultValue: 'Crabenv Multilang',
  );
  static const String PUBLIC_FLUTTER_SUPERWALL_IOS_API_KEY =
      String.fromEnvironment('PUBLIC_FLUTTER_SUPERWALL_IOS_API_KEY');

  static const Map<String, String> _required = {
    'PUBLIC_FLUTTER_API_URL': PUBLIC_FLUTTER_API_URL,
  };

  static void validate() {
    final missing = _required.entries
        .where((e) => e.value.isEmpty)
        .map((e) => e.key)
        .toList();
    assert(
      missing.isEmpty,
      'Missing env vars: ${missing.join(', ')}. '
      'Did you forget --dart-define-from-file=../../.env ?',
    );
  }
}

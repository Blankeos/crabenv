# Flutter / Dart

Dependencies: none (Flutter SDK `String.fromEnvironment`); monorepo justfile examples use `dotenv-cli`

```sh
# single app
- .env
- .env.example
- lib/
    - config/
        - env.dart
```

```sh
# monorepo (melos)
- .env
- .env.example
- apps/
    - mobile/
        - .env.example
        - lib/config/env.dart
```

## Example

`lib/config/env.dart`

```dart
// ignore_for_file: constant_identifier_names

/// Baked in at build time: `flutter run --dart-define-from-file=.env`
/// All values are public — the binary can be inspected.
abstract class Env {
  static const String PUBLIC_API_URL = String.fromEnvironment('PUBLIC_API_URL');
  static const String PUBLIC_SUPERWALL_IOS_API_KEY = String.fromEnvironment(
    'PUBLIC_SUPERWALL_IOS_API_KEY',
  );

  static const Map<String, String> _required = {
    'PUBLIC_API_URL': PUBLIC_API_URL,
  };

  static void validate() {
    final missing = _required.entries
        .where((e) => e.value.isEmpty)
        .map((e) => e.key)
        .toList();
    assert(
      missing.isEmpty,
      'Missing env vars: ${missing.join(', ')}. '
      'Did you forget --dart-define-from-file=.env ?',
    );
  }
}
```

`justfile` (or a similar script runner)

```just
_env *cmd:
    dotenv -e ../../.env -- {{ cmd }}

dev *args:
    just _env "flutter run --dart-define-from-file=.env {{ args }}"

build *args:
    just _env "flutter build {{ args }} --dart-define-from-file=.env"
```

`main.dart`

```dart
void main() {
  WidgetsFlutterBinding.ensureInitialized();
  Env.validate();
  runApp(const App());
}
```

Use `Env.PUBLIC_*` in code — identifier, dart-define key, and `.env` key stay the same. No server secrets in the app.
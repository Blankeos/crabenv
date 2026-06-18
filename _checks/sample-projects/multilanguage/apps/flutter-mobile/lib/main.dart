import 'package:flutter/widgets.dart';

import 'config/env.dart';

void main() {
  WidgetsFlutterBinding.ensureInitialized();
  Env.validate();
  runApp(const Center(child: Text(Env.PUBLIC_FLUTTER_APP_NAME)));
}

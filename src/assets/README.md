# Assets

Whisper uses its own W monogram for the app and menu bar icons, and provider icons inherited from OpenWhispr.

- `icon.icns` is the macOS app icon.
- `openwhispr.icon/` is the Icon Composer source for `scripts/compile-macos-icon.js`. The source asset keeps its upstream name.
- `Assets.car` is the compiled Icon Composer icon that macOS 26 and later show instead of `icon.icns`. Without it, macOS places the legacy icon on a light platter. CI runners lack an Xcode that can compile `.icon` bundles, so run `pnpm run compile:mac-icon` with Xcode 26 or later and commit the result whenever `openwhispr.icon/` changes.
- `iconTemplate@3x.png` is the macOS menu bar template icon.
- `icon.png` is the favicon and fallback icon.
- `icons/providers/` supplies `src/utils/providerIcons.ts`.
- [JetBrains Mono](fonts/jetbrains-mono/) provides Latin fonts and their OFL-1.1 license. Chinese text uses system PingFang on macOS.
- `fonts/noto-sans.css` defines the bundled Noto Sans fallback loaded by `src/index.html`.

Packaging excludes these Markdown files and the Icon Composer source. Preserve the upstream attribution and bundled font licenses when changing assets.

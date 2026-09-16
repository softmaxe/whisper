# Assets

This build retains application artwork and provider icons from OpenWhispr.

- `icon.icns` is the macOS application icon used by `electron-builder.json`.
- `openwhispr.icon/` is the Apple Icon Composer source bundle. `scripts/compile-macos-icon.js` can compile it to `Assets.car`.
- `iconTemplate@3x.png` is the macOS menu bar template icon.
- `icon.png` is the renderer favicon and a fallback icon. `icon.ico` remains referenced by the shared tray helper.
- `icons/providers/` contains the icons registered by `src/utils/providerIcons.ts`.
- `fonts/jetbrains-mono/` contains the bundled Latin fonts, their source attribution, and the OFL-1.1 license. Chinese text uses system PingFang on macOS.
- `fonts/noto-sans.css` and its WOFF2 files provide the existing global font fallback loaded by `src/index.html`.

Preserve the upstream attribution and bundled font licenses when changing these assets.

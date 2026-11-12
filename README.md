# NovaTerm

A terminal emulator for Android 16 and 17 with its core written in Rust.

- **Rust** (`rust/`): the pseudo-terminal (PTY), the shell process, the VT100/xterm parser
  and the screen buffer. Zero external dependencies; the JNI and libc bindings are hand-written.
- **Kotlin** (`app/`): the UI only — drawing the grid, keyboard, gestures, settings.
  No AndroidX, just platform APIs.

## Features

- 16 / 256 / truecolor, bold, italic, underline, inverse, strikethrough
- alternate screen (vim, less, htop), scroll regions, wide characters, UTF-8
- 5000 lines of scrollback (drag to scroll), resizes on rotation and when the keyboard opens
- extra keys bar: ESC, TAB, CTRL, ALT, arrows, HOME/END, PGUP/PGDN
- hardware keyboards: Ctrl/Alt, F1–F12, arrows
- pinch to change the font size
- long press: copy screen, paste (bracketed paste), settings

## Customization (⚙ on the keys bar, or long press → Settings)

- 9 themes: Nova, Dracula, Gruvbox Dark, Nord, Tokyo Night, Catppuccin Mocha,
  Solarized Dark, Matrix, One Light
- custom text, background and cursor colors (`#RRGGBB`)
- font size
- cursor style (block, underline, vertical bar) and blinking
- show or hide the extra keys bar
- keep screen on
- which shell to start (default `/system/bin/sh`)

To add a theme, append it to `Themes.ALL` in
`app/src/main/kotlin/dev/snova/novaterm/Settings.kt`.

## Building

### GitHub Actions

Every push runs `.github/workflows/build.yml`, which tests the Rust core, builds the APK
and uploads it as the `NovaTerm-debug-apk` artifact on the run's page in the **Actions** tab.

### Locally

Requires JDK 17+, the Android SDK (platform 36) with the NDK, and Rust.

```sh
rustup target add aarch64-linux-android x86_64-linux-android
cargo install cargo-ndk
export ANDROID_HOME=$HOME/Android/Sdk
export ANDROID_NDK_HOME=$ANDROID_HOME/ndk/<version>
./gradlew assembleDebug
adb install app/build/outputs/apk/debug/app-debug.apk
```

The core's tests run on any Linux machine, no Android needed: `cd rust && cargo test`.

## Limitations

- The shell is Android's own (`mksh` + `toybox`), running with the app's permissions:
  no root and no package manager. It is not a full Linux distribution.
- One session only; it ends when the app is closed or killed in the background.
- Lines are not reflowed when the width changes.

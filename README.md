# NovaTerm

Emulator de terminal pentru Android 16 și 17, cu nucleul scris în Rust.

- **Rust** (`rust/`): pseudo-terminalul (PTY), procesul shell, parserul VT100/xterm și
  bufferul de ecran. Zero dependențe externe; legăturile JNI și libc sunt scrise de mână.
- **Kotlin** (`app/`): doar interfața — desenarea grilei, tastatura, gesturile, setările.
  Fără AndroidX, doar API-urile platformei.

## Ce știe

- culori 16 / 256 / truecolor, bold, italic, subliniat, inversat, tăiat
- ecran alternativ (vim, less, htop), regiuni de scroll, caractere late, UTF-8
- istoric de 5000 de linii (glisezi cu degetul), redimensionare la rotire și la tastatură
- bară de taste: ESC, TAB, CTRL, ALT, săgeți, HOME/END, PGUP/PGDN
- tastatură fizică: Ctrl/Alt, F1–F12, săgeți
- ciupire cu două degete pentru mărimea fontului
- apăsare lungă: copiază ecranul, lipește (bracketed paste), setări

## Personalizare (⚙ din bara de taste sau apăsare lungă → Setări)

- 9 teme: Nova, Dracula, Gruvbox Dark, Nord, Tokyo Night, Catppuccin Mocha,
  Solarized Dark, Matrix, One Light
- culori proprii pentru text, fundal și cursor (`#RRGGBB`)
- mărimea fontului
- stilul cursorului (bloc, linie jos, bară verticală) și clipirea lui
- afișarea barei de taste suplimentare
- ecran mereu aprins
- shell-ul pornit (implicit `/system/bin/sh`)

Temele noi se adaugă în `app/src/main/kotlin/dev/snova/novaterm/Settings.kt`, în lista `Themes.ALL`.

## Compilare

### Varianta simplă: GitHub Actions

Urcă proiectul într-un repo pe GitHub. Workflow-ul din `.github/workflows/build.yml`
rulează testele Rust, compilează APK-ul și îl atașează ca artefact (`NovaTerm-debug-apk`)
la rularea din tabul **Actions**.

### Local (Fedora)

Ai nevoie de JDK 17+, Android SDK (platforma 36) cu NDK și Rust.

```sh
rustup target add aarch64-linux-android x86_64-linux-android
cargo install cargo-ndk
export ANDROID_HOME=$HOME/Android/Sdk
export ANDROID_NDK_HOME=$ANDROID_HOME/ndk/<versiune>
./gradlew assembleDebug
adb install app/build/outputs/apk/debug/app-debug.apk
```

Testele nucleului rulează pe orice Linux, fără Android: `cd rust && cargo test`.

## Limite

- Shell-ul este cel al sistemului Android (`mksh` + `toybox`), rulat cu drepturile
  aplicației: fără root și fără manager de pachete. Nu este o distribuție Linux completă.
- O singură sesiune; se închide când aplicația e închisă sau oprită de sistem în fundal.
- Liniile nu se rearanjează (reflow) la schimbarea lățimii.

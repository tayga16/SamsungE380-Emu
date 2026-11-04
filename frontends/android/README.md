# Android frontend

Экспериментальный порт Samsung SGH-E830 для Android. Использует существующую модель устройства из корневого Rust-проекта и библиотеку с карточками в стиле [PocketHLE](https://github.com/j92580498-max/PocketHLE). Поддерживается только `arm64-v8a`, Android 7.0/API 24 и новее.

В APK нет прошивки. В библиотеку импортируются `.cla` и соответствующий `.nsf`, а `.tfs` и физический образ OneNAND можно добавить отдельно. Импорт копирует файлы в приватный каталог приложения и проверяет их тем же кодом, что используется эмулятором; исходники не изменяются. При отсутствии физического образа запускается существующий режим со стёртой флеш-памятью. Достигнутое состояние прошивки не включает главное меню, а ввод с клавиатуры телефона не реализован.

## Сборка

Нужны JDK 17, CMake, Android SDK Platform 35, Build Tools 35.0.0, Android NDK 29.0.14206865, Rust и `cargo-ndk` 4.1.2. Из корня репозитория:

```sh
rustup target add aarch64-linux-android
cargo install cargo-ndk --version 4.1.2 --locked
export ANDROID_HOME="$HOME/Android/Sdk"
export ANDROID_SDK_ROOT="$ANDROID_HOME"
export ANDROID_NDK_HOME="$ANDROID_HOME/ndk/29.0.14206865"
./frontends/android/gradlew -p frontends/android :app:assembleDebug :app:lintDebug
```

Установочный debug APK появится в `frontends/android/app/build/outputs/apk/debug/app-debug.apk`. Workflow [Android APK](../../.github/workflows/android-apk.yml) собирает и проверяет его для push, pull request и ручного запуска, затем публикует APK как артефакт сборки на 14 дней.

Android-экран и используемые ресурсы PocketHLE указаны в `THIRD_PARTY_NOTICES.md`; лицензии приложены к исходникам и APK. Движок Unicorn использует GPL-2.0; в APK включён текст лицензии, а соответствующий исходный код доступен в этом репозитории.

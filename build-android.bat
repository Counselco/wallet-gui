@echo off
set JAVA_HOME=C:\Program Files\Android\Android Studio\jbr
cargo tauri android build --features mobile
echo Android build complete.

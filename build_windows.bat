@echo off
REM Builds Campus Crypt on Windows and creates dist\CampusCrypt\ (exe + assets).
REM Needs Rust from https://rustup.rs (choose the default MSVC option; it will ask to install
REM the Visual Studio C++ Build Tools - accept). Open a NEW terminal after installing.
where cargo >nul 2>nul || (echo Rust not found. Install it from https://rustup.rs & exit /b 1)
cargo build --release || exit /b 1
rmdir /s /q dist 2>nul
mkdir dist\CampusCrypt
copy target\release\campus_crypt.exe dist\CampusCrypt\ >nul
xcopy /E /I /Q assets dist\CampusCrypt\assets >nul
echo Done. Run dist\CampusCrypt\campus_crypt.exe
echo If the Elegoo is plugged in, it appears as a COM port (e.g. COM3); close the Arduino Serial Monitor first.

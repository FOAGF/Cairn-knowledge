# M8 feasibility: a Tauri app built on Linux for Windows

Plan §12, round 1 (D-55). Built on the owner's Linux machine with `cargo-xwin`; no installer (D-56).

    cd poc/j-windows/src-tauri
    CARGO_BUILD_JOBS=8 nice -n 10 cargo tauri build --runner cargo-xwin --target x86_64-pc-windows-msvc --no-bundle

The result is `target/x86_64-pc-windows-msvc/release/cairn-wintest.exe`, zipped as `out/cairn-wintest.zip`. On Windows: unzip,
run the exe, and paste back the text in the window. It reports the WebView2 version, whether Microsoft YaHei is
available, whether the folder next to the exe can be written (the portable data folder), and whether Credential
Manager works.

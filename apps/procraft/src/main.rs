//! Desktop/iOS entry; Android enters via `android_main` in the library.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
fn main() {
    procraft_app::run();
}

// Release builds are GUI apps: no console window next to Nidavellir.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    nidavellir_ui_lib::run()
}

// 桌面程式不需要主控台視窗。
#![windows_subsystem = "windows"]

fn main() {
    std::process::exit(aiwake_app::run());
}

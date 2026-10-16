//! 把圖示、版本資訊與應用程式資訊清單（一般使用者權限、Per-Monitor V2 DPI、視覺樣式）嵌入執行檔。

fn main() {
    println!("cargo:rerun-if-changed=app.manifest");
    println!("cargo:rerun-if-changed=../assets/app.ico");

    let mut resource = winresource::WindowsResource::new();
    resource.set_icon("../assets/app.ico");
    resource.set_manifest_file("app.manifest");
    resource.set_language(0x0404);
    resource.set("ProductName", "AI 倒數喚醒");
    resource.set("FileDescription", "AI 倒數喚醒");
    resource.set("CompanyName", "AI Wake Scheduler");
    resource.set("LegalCopyright", "Copyright © 2026");
    resource.set("OriginalFilename", "AI倒數喚醒.exe");
    resource.compile().expect("無法編譯 Windows 資源");
}

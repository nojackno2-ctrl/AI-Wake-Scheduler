//! 保留原生 IFileOpenDialog 與 IFileSaveDialog；主介面全部由 egui 繪製。
use crate::w32::wide;
use windows::core::PCWSTR;
use windows::Win32::Foundation::HWND;
use windows::Win32::System::Com::{CoCreateInstance, CoTaskMemFree, CLSCTX_INPROC_SERVER};
use windows::Win32::UI::Shell::Common::COMDLG_FILTERSPEC;
use windows::Win32::UI::Shell::{
    FileOpenDialog, FileSaveDialog, IFileOpenDialog, IFileSaveDialog, IShellItem, SHCreateItemFromParsingName,
    FILEOPENDIALOGOPTIONS, SIGDN_FILESYSPATH,
};
fn shell_item_path(item: &IShellItem) -> Option<String> {
    unsafe {
        let raw = item.GetDisplayName(SIGDN_FILESYSPATH).ok()?;
        let path = raw.to_string().ok();
        CoTaskMemFree(Some(raw.0 as *const _));
        path
    }
}

fn open_dialog(
    owner: HWND,
    title: &str,
    options: u32,
    filters: &[(&str, &str)],
    initial_folder: Option<&str>,
) -> Option<String> {
    unsafe {
        let dialog: IFileOpenDialog = CoCreateInstance(&FileOpenDialog, None, CLSCTX_INPROC_SERVER).ok()?;
        let title = wide(title);
        let _ = dialog.SetTitle(PCWSTR(title.as_ptr()));
        if let Ok(current) = dialog.GetOptions() {
            let _ = dialog.SetOptions(FILEOPENDIALOGOPTIONS(current.0 | options));
        }

        let buffers: Vec<(Vec<u16>, Vec<u16>)> = filters.iter().map(|(name, spec)| (wide(name), wide(spec))).collect();
        if !buffers.is_empty() {
            let specs: Vec<COMDLG_FILTERSPEC> = buffers
                .iter()
                .map(|(name, spec)| COMDLG_FILTERSPEC {
                    pszName: PCWSTR(name.as_ptr()),
                    pszSpec: PCWSTR(spec.as_ptr()),
                })
                .collect();
            let _ = dialog.SetFileTypes(&specs);
        }

        if let Some(folder) = initial_folder.filter(|folder| std::path::Path::new(folder).is_dir()) {
            let folder = wide(folder);
            if let Ok(item) = SHCreateItemFromParsingName::<_, _, IShellItem>(PCWSTR(folder.as_ptr()), None) {
                let _ = dialog.SetFolder(&item);
            }
        }

        dialog.Show(Some(owner)).ok()?;
        shell_item_path(&dialog.GetResult().ok()?)
    }
}

/// 選擇資料夾；使用者取消時回傳 None。
pub fn pick_folder(owner: HWND, title: &str, initial_folder: &str) -> Option<String> {
    const FOS_PICKFOLDERS: u32 = 0x20;
    const FOS_FORCEFILESYSTEM: u32 = 0x40;
    open_dialog(owner, title, FOS_PICKFOLDERS | FOS_FORCEFILESYSTEM, &[], Some(initial_folder))
}

/// 選擇既有的檔案；使用者取消時回傳 None。
pub fn pick_file(owner: HWND, title: &str, filters: &[(&str, &str)]) -> Option<String> {
    const FOS_FORCEFILESYSTEM: u32 = 0x40;
    const FOS_FILEMUSTEXIST: u32 = 0x1000;
    open_dialog(owner, title, FOS_FORCEFILESYSTEM | FOS_FILEMUSTEXIST, filters, None)
}

/// 選擇要儲存的檔案位置；使用者取消時回傳 None。
pub fn save_file(
    owner: HWND,
    title: &str,
    default_name: &str,
    default_extension: &str,
    filters: &[(&str, &str)],
) -> Option<String> {
    const FOS_OVERWRITEPROMPT: u32 = 0x2;
    const FOS_FORCEFILESYSTEM: u32 = 0x40;
    unsafe {
        let dialog: IFileSaveDialog = CoCreateInstance(&FileSaveDialog, None, CLSCTX_INPROC_SERVER).ok()?;
        let title = wide(title);
        let _ = dialog.SetTitle(PCWSTR(title.as_ptr()));
        if let Ok(current) = dialog.GetOptions() {
            let _ = dialog.SetOptions(FILEOPENDIALOGOPTIONS(current.0 | FOS_OVERWRITEPROMPT | FOS_FORCEFILESYSTEM));
        }
        let buffers: Vec<(Vec<u16>, Vec<u16>)> = filters.iter().map(|(name, spec)| (wide(name), wide(spec))).collect();
        if !buffers.is_empty() {
            let specs: Vec<COMDLG_FILTERSPEC> = buffers
                .iter()
                .map(|(name, spec)| COMDLG_FILTERSPEC {
                    pszName: PCWSTR(name.as_ptr()),
                    pszSpec: PCWSTR(spec.as_ptr()),
                })
                .collect();
            let _ = dialog.SetFileTypes(&specs);
        }
        let extension = wide(default_extension);
        let _ = dialog.SetDefaultExtension(PCWSTR(extension.as_ptr()));
        let name = wide(default_name);
        let _ = dialog.SetFileName(PCWSTR(name.as_ptr()));

        dialog.Show(Some(owner)).ok()?;
        shell_item_path(&dialog.GetResult().ok()?)
    }
}

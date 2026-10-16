//! 核心層需要的少量 Win32 呼叫。

use std::collections::HashMap;

use windows::Win32::Foundation::{CloseHandle, HANDLE};
use windows::Win32::System::Com::CoCreateGuid;
use windows::Win32::System::Diagnostics::Debug::{GetErrorMode, SetErrorMode, THREAD_ERROR_MODE};
use windows::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W, TH32CS_SNAPPROCESS,
};
use windows::Win32::System::Threading::{OpenProcess, TerminateProcess, PROCESS_TERMINATE};

/// 作業系統啟動子程序失敗時，回傳錯誤碼而不是彈出嚴重錯誤對話框。
pub const SEM_FAILCRITICALERRORS: u32 = 0x0001;
/// 子程序當掉時不要彈出 Windows 錯誤報告對話框。
pub const SEM_NOGPFAULTERRORBOX: u32 = 0x0002;

pub fn to_wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

pub fn set_error_mode(mode: u32) -> u32 {
    unsafe { SetErrorMode(THREAD_ERROR_MODE(mode)).0 }
}

pub fn get_error_mode() -> u32 {
    unsafe { GetErrorMode() }
}

pub fn new_guid() -> u128 {
    match unsafe { CoCreateGuid() } {
        Ok(guid) => guid.to_u128(),
        // CoCreateGuid 幾乎不會失敗；萬一失敗仍需要一個不重複的值。
        Err(_) => {
            let nanos = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|elapsed| elapsed.as_nanos())
                .unwrap_or_default();
            nanos ^ ((std::process::id() as u128) << 96)
        }
    }
}

struct ProcessEntry {
    id: u32,
    parent_id: u32,
    name: String,
}

struct OwnedHandle(HANDLE);

impl Drop for OwnedHandle {
    fn drop(&mut self) {
        unsafe {
            let _ = CloseHandle(self.0);
        }
    }
}

fn snapshot_processes() -> Vec<ProcessEntry> {
    let mut processes = Vec::new();
    unsafe {
        let Ok(snapshot) = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) else {
            return processes;
        };
        let snapshot = OwnedHandle(snapshot);
        let mut entry = PROCESSENTRY32W { dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32, ..Default::default() };
        if Process32FirstW(snapshot.0, &mut entry).is_err() {
            return processes;
        }
        loop {
            let length = entry.szExeFile.iter().position(|&c| c == 0).unwrap_or(entry.szExeFile.len());
            processes.push(ProcessEntry {
                id: entry.th32ProcessID,
                parent_id: entry.th32ParentProcessID,
                name: String::from_utf16_lossy(&entry.szExeFile[..length]),
            });
            if Process32NextW(snapshot.0, &mut entry).is_err() {
                break;
            }
        }
    }
    processes
}

fn terminate(process_id: u32) {
    unsafe {
        if let Ok(handle) = OpenProcess(PROCESS_TERMINATE, false, process_id) {
            let handle = OwnedHandle(handle);
            let _ = TerminateProcess(handle.0, 1);
        }
    }
}

/// 終止指定程序與它的所有子孫程序。
/// 程序可能已自行結束或無權限終止，兩者都不影響結果判定。
pub fn kill_process_tree(process_id: u32) {
    let mut children: HashMap<u32, Vec<u32>> = HashMap::new();
    for process in snapshot_processes() {
        if process.id != 0 && process.id != process.parent_id {
            children.entry(process.parent_id).or_default().push(process.id);
        }
    }

    let mut ordered = vec![process_id];
    let mut index = 0;
    while index < ordered.len() {
        if let Some(list) = children.get(&ordered[index]) {
            for &child in list {
                if !ordered.contains(&child) {
                    ordered.push(child);
                }
            }
        }
        index += 1;
    }

    for id in ordered {
        terminate(id);
    }
}

/// 依程序名稱（不含副檔名）尋找程序識別碼。
pub fn process_ids_by_name(name: &str) -> Vec<u32> {
    snapshot_processes()
        .into_iter()
        .filter(|process| {
            let stem = process.name.strip_suffix(".exe").or_else(|| process.name.strip_suffix(".EXE"));
            stem.unwrap_or(&process.name).eq_ignore_ascii_case(name)
        })
        .map(|process| process.id)
        .collect()
}

/// 展開 `%NAME%` 形式的環境變數；未定義的變數保持原樣。
pub fn expand_environment_variables(text: &str) -> String {
    let mut result = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find('%') {
        result.push_str(&rest[..start]);
        let after = &rest[start + 1..];
        match after.find('%') {
            Some(end) => {
                let name = &after[..end];
                match std::env::var(name) {
                    Ok(value) if !name.is_empty() => {
                        result.push_str(&value);
                        rest = &after[end + 1..];
                    }
                    _ => {
                        result.push('%');
                        rest = after;
                    }
                }
            }
            None => {
                result.push('%');
                rest = after;
            }
        }
    }
    result.push_str(rest);
    result
}

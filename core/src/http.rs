//! 以 WinHTTP 實作的最小同步 HTTP 用戶端，避免為了兩個額度查詢引入額外的網路相依套件。

use std::ffi::c_void;
use std::time::Duration;

use windows::core::PCWSTR;
use windows::Win32::Networking::WinHttp::{
    WinHttpCloseHandle, WinHttpConnect, WinHttpOpen, WinHttpOpenRequest, WinHttpQueryDataAvailable,
    WinHttpQueryHeaders, WinHttpReadData, WinHttpReceiveResponse, WinHttpSendRequest, WinHttpSetOption,
    WinHttpSetTimeouts, WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY, WINHTTP_ACCESS_TYPE_NO_PROXY, WINHTTP_FLAG_SECURE,
    WINHTTP_OPEN_REQUEST_FLAGS, WINHTTP_OPTION_SECURITY_FLAGS, WINHTTP_QUERY_FLAG_NUMBER, WINHTTP_QUERY_RETRY_AFTER,
    WINHTTP_QUERY_STATUS_CODE,
};

use crate::win::to_wide;

const SECURITY_FLAG_IGNORE_UNKNOWN_CA: u32 = 0x0000_0100;
const SECURITY_FLAG_IGNORE_CERT_DATE_INVALID: u32 = 0x0000_2000;
const SECURITY_FLAG_IGNORE_CERT_CN_INVALID: u32 = 0x0000_1000;
const SECURITY_FLAG_IGNORE_CERT_WRONG_USAGE: u32 = 0x0000_0200;

pub struct HttpRequest<'a> {
    pub method: &'a str,
    pub url: &'a str,
    pub headers: &'a [(&'a str, &'a str)],
    pub body: Option<&'a [u8]>,
    pub connect_timeout: Duration,
    pub timeout: Duration,
    /// 略過伺服器憑證驗證；只用於本機 language server 的自簽憑證。
    pub ignore_certificate_errors: bool,
}

#[derive(Debug, Clone)]
pub struct HttpResponse {
    pub status: u32,
    pub body: String,
    pub retry_after: Option<String>,
}

impl HttpResponse {
    pub fn is_success(&self) -> bool {
        (200..300).contains(&self.status)
    }
}

struct Handle(*mut c_void);

impl Handle {
    fn new(raw: *mut c_void, what: &str) -> Result<Self, String> {
        if raw.is_null() {
            Err(format!("{what}：{}", windows::core::Error::from_win32().message()))
        } else {
            Ok(Handle(raw))
        }
    }
}

impl Drop for Handle {
    fn drop(&mut self) {
        unsafe {
            let _ = WinHttpCloseHandle(self.0);
        }
    }
}

struct ParsedUrl {
    secure: bool,
    host: String,
    port: u16,
    path: String,
}

fn parse_url(url: &str) -> Result<ParsedUrl, String> {
    let (secure, rest) = if let Some(rest) = url.strip_prefix("https://") {
        (true, rest)
    } else if let Some(rest) = url.strip_prefix("http://") {
        (false, rest)
    } else {
        return Err(format!("不支援的網址：{url}"));
    };
    let (authority, path) = match rest.find('/') {
        Some(index) => (&rest[..index], &rest[index..]),
        None => (rest, "/"),
    };
    let (host, port) = match authority.rsplit_once(':') {
        Some((host, port)) => (host, port.parse::<u16>().map_err(|_| format!("無效的連接埠：{url}"))?),
        None => (authority, if secure { 443 } else { 80 }),
    };
    Ok(ParsedUrl { secure, host: host.to_string(), port, path: path.to_string() })
}

fn win_error(what: &str, error: windows::core::Error) -> String {
    format!("{what}：{}", error.message())
}

fn millis(duration: Duration) -> i32 {
    duration.as_millis().min(i32::MAX as u128) as i32
}

pub fn send(request: &HttpRequest<'_>) -> Result<HttpResponse, String> {
    let url = parse_url(request.url)?;
    let is_loopback = url.host == "127.0.0.1" || url.host.eq_ignore_ascii_case("localhost");

    unsafe {
        let agent = to_wide(concat!("ai-wake-scheduler/", env!("CARGO_PKG_VERSION")));
        let session = Handle::new(
            WinHttpOpen(
                PCWSTR(agent.as_ptr()),
                if is_loopback { WINHTTP_ACCESS_TYPE_NO_PROXY } else { WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY },
                PCWSTR::null(),
                PCWSTR::null(),
                0,
            ),
            "無法建立 HTTP 工作階段",
        )?;
        let total = millis(request.timeout);
        WinHttpSetTimeouts(session.0, total, millis(request.connect_timeout), total, total)
            .map_err(|e| win_error("無法設定 HTTP 逾時", e))?;

        let host = to_wide(&url.host);
        let connection =
            Handle::new(WinHttpConnect(session.0, PCWSTR(host.as_ptr()), url.port, 0), "無法建立 HTTP 連線")?;

        let verb = to_wide(request.method);
        let path = to_wide(&url.path);
        let handle = Handle::new(
            WinHttpOpenRequest(
                connection.0,
                PCWSTR(verb.as_ptr()),
                PCWSTR(path.as_ptr()),
                PCWSTR::null(),
                PCWSTR::null(),
                std::ptr::null(),
                if url.secure { WINHTTP_FLAG_SECURE } else { WINHTTP_OPEN_REQUEST_FLAGS(0) },
            ),
            "無法建立 HTTP 要求",
        )?;

        if request.ignore_certificate_errors && url.secure {
            let flags = SECURITY_FLAG_IGNORE_UNKNOWN_CA
                | SECURITY_FLAG_IGNORE_CERT_DATE_INVALID
                | SECURITY_FLAG_IGNORE_CERT_CN_INVALID
                | SECURITY_FLAG_IGNORE_CERT_WRONG_USAGE;
            WinHttpSetOption(
                Some(handle.0 as *const c_void),
                WINHTTP_OPTION_SECURITY_FLAGS,
                Some(&flags.to_ne_bytes()),
            )
            .map_err(|e| win_error("無法設定憑證選項", e))?;
        }

        let header_text: String = request.headers.iter().map(|(name, value)| format!("{name}: {value}\r\n")).collect();
        let header_wide: Vec<u16> = header_text.encode_utf16().collect();
        let body = request.body.unwrap_or(&[]);
        WinHttpSendRequest(
            handle.0,
            if header_wide.is_empty() { None } else { Some(&header_wide) },
            if body.is_empty() { None } else { Some(body.as_ptr() as *const c_void) },
            body.len() as u32,
            body.len() as u32,
            0,
        )
        .map_err(|e| win_error("傳送要求失敗", e))?;
        WinHttpReceiveResponse(handle.0, std::ptr::null_mut()).map_err(|e| win_error("接收回應失敗", e))?;

        let mut status = 0u32;
        let mut size = std::mem::size_of::<u32>() as u32;
        WinHttpQueryHeaders(
            handle.0,
            WINHTTP_QUERY_STATUS_CODE | WINHTTP_QUERY_FLAG_NUMBER,
            PCWSTR::null(),
            Some(&mut status as *mut u32 as *mut c_void),
            &mut size,
            std::ptr::null_mut(),
        )
        .map_err(|e| win_error("讀取狀態碼失敗", e))?;

        let mut retry_buffer = [0u16; 128];
        let mut retry_size = (retry_buffer.len() * 2) as u32;
        let retry_after = WinHttpQueryHeaders(
            handle.0,
            WINHTTP_QUERY_RETRY_AFTER,
            PCWSTR::null(),
            Some(retry_buffer.as_mut_ptr() as *mut c_void),
            &mut retry_size,
            std::ptr::null_mut(),
        )
        .ok()
        .map(|_| String::from_utf16_lossy(&retry_buffer[..(retry_size as usize / 2).min(retry_buffer.len())]))
        .map(|text| text.trim_end_matches('\0').trim().to_string())
        .filter(|text| !text.is_empty());

        let mut bytes = Vec::new();
        loop {
            let mut available = 0u32;
            WinHttpQueryDataAvailable(handle.0, &mut available).map_err(|e| win_error("讀取回應失敗", e))?;
            if available == 0 {
                break;
            }
            let start = bytes.len();
            bytes.resize(start + available as usize, 0u8);
            let mut read = 0u32;
            WinHttpReadData(handle.0, bytes[start..].as_mut_ptr() as *mut c_void, available, &mut read)
                .map_err(|e| win_error("讀取回應失敗", e))?;
            bytes.truncate(start + read as usize);
            if read == 0 {
                break;
            }
        }

        Ok(HttpResponse { status, body: String::from_utf8_lossy(&bytes).into_owned(), retry_after })
    }
}

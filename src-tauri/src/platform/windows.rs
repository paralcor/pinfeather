//! E03b 只读目标探针。没有焦点切换、输入注入、剪贴板或窗口正文 API。

use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ImeState {
    NoComposition,
    Composing,
    Unknown,
}

/// IMM 的零长度不是 TSF 未组合的证据。探针没有可证明排除 TSF 的 provider。
fn ime_state(context_present: bool, composition_bytes: Option<i32>, stable: bool) -> ImeState {
    match (context_present, composition_bytes, stable) {
        (true, Some(bytes), true) if bytes > 0 && bytes % 2 == 0 => ImeState::Composing,
        _ => ImeState::Unknown,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PermissionState {
    Normal,
    ManualOnlyHigherPrivilege,
    ManualOnlyUnknown,
}

fn permission_state(own: Option<u32>, target: Option<u32>, elevated: Option<bool>,
    ui_access: Option<bool>) -> PermissionState {
    match (own, target, elevated, ui_access) {
        (Some(own), Some(target), Some(elevated), Some(ui_access)) if own <= 0x2000 => {
            if target > own || target > 0x2000 || elevated || ui_access {
                PermissionState::ManualOnlyHigherPrivilege
            } else {
                PermissionState::Normal
            }
        }
        _ => PermissionState::ManualOnlyUnknown,
    }
}

#[derive(Debug, Serialize)]
pub struct TargetObservation {
    pub hwnd: Option<String>,
    pub root_hwnd: Option<String>,
    pub owner_hwnd: Option<String>,
    pub pid: u32,
    pub thread_id: u32,
    pub process_created_filetime: Option<String>,
    pub executable_name: Option<String>,
    pub process_machine: Option<u16>,
    pub native_machine: Option<u16>,
    pub focus_hwnd: Option<String>,
    pub focus_pid: u32,
    pub focus_thread_id: u32,
    pub identity_stable: bool,
    pub own_integrity_rid: Option<u32>,
    pub target_integrity_rid: Option<u32>,
    pub target_elevated: Option<bool>,
    pub target_ui_access: Option<bool>,
    pub permission: PermissionState,
    pub keyboard_layout: Option<String>,
    pub imm_is_ime: Option<bool>,
    pub imm_context_present: bool,
    pub imm_composition_bytes: Option<i32>,
    pub imm_context_released: bool,
    pub ime_scope: &'static str,
    pub ime_state: ImeState,
}

/// 一次只读采样，结尾复查前台/焦点/进程；观测值不构成回贴许可。
pub fn observe_foreground() -> TargetObservation {
    native::observe_foreground()
}

mod native {
    use super::*;
    use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
    use windows_sys::Win32::Foundation::{HANDLE, HWND, WAIT_TIMEOUT};
    use windows_sys::Win32::Security::{
        GetSidSubAuthority, GetSidSubAuthorityCount, GetTokenInformation, IsValidSid,
        TokenElevation, TokenIntegrityLevel, TokenUIAccess, TOKEN_INFORMATION_CLASS,
        TOKEN_MANDATORY_LABEL, TOKEN_QUERY,
    };
    use windows_sys::Win32::System::Threading::{
        GetCurrentProcess, GetProcessTimes, IsWow64Process2, OpenProcess, OpenProcessToken,
        QueryFullProcessImageNameW, WaitForSingleObject, PROCESS_QUERY_LIMITED_INFORMATION,
        PROCESS_SYNCHRONIZE,
    };
    use windows_sys::Win32::UI::Input::Ime::{
        ImmGetCompositionStringW, ImmGetContext, ImmIsIME, ImmReleaseContext, GCS_COMPSTR,
    };
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::GetKeyboardLayout;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        GetAncestor, GetForegroundWindow, GetGUIThreadInfo, GetWindow, GetWindowThreadProcessId,
        IsWindow, GA_ROOT, GUITHREADINFO, GW_OWNER,
    };

    fn id(handle: HANDLE) -> Option<String> {
        (!handle.is_null()).then(|| format!("0x{:x}", handle as usize))
    }

    fn token(process: HANDLE) -> Option<OwnedHandle> {
        let mut handle = std::ptr::null_mut();
        // process 是存活 OwnedHandle 或 GetCurrentProcess 伪句柄；只读 TOKEN_QUERY。
        (unsafe { OpenProcessToken(process, TOKEN_QUERY, &mut handle) } != 0)
            .then(|| unsafe { OwnedHandle::from_raw_handle(handle) })
    }

    fn token_bool(token: HANDLE, class: TOKEN_INFORMATION_CLASS) -> Option<bool> {
        let mut value = 0u32;
        let mut size = 0u32;
        let ok = unsafe { GetTokenInformation(token, class, (&mut value as *mut u32).cast(),
            std::mem::size_of::<u32>() as u32, &mut size) };
        (ok != 0 && size == 4 && value <= 1).then_some(value != 0)
    }

    fn integrity(token: HANDLE) -> Option<u32> {
        let mut needed = 0u32;
        unsafe { GetTokenInformation(token, TokenIntegrityLevel, std::ptr::null_mut(), 0, &mut needed) };
        if needed < std::mem::size_of::<TOKEN_MANDATORY_LABEL>() as u32 || needed > 16 * 1024 {
            return None;
        }
        // usize 保证 TOKEN_MANDATORY_LABEL 对齐，Windows 在此有界缓冲内写入 label/SID。
        let mut words = vec![0usize; (needed as usize).div_ceil(std::mem::size_of::<usize>())];
        let capacity = (words.len() * std::mem::size_of::<usize>()) as u32;
        if unsafe { GetTokenInformation(token, TokenIntegrityLevel, words.as_mut_ptr().cast(),
            capacity, &mut needed) } == 0 || needed > capacity {
            return None;
        }
        unsafe {
            let label = &*words.as_ptr().cast::<TOKEN_MANDATORY_LABEL>();
            let sid = label.Label.Sid;
            if sid.is_null() || IsValidSid(sid) == 0 { return None; }
            let count = GetSidSubAuthorityCount(sid);
            if count.is_null() || *count == 0 { return None; }
            let rid = GetSidSubAuthority(sid, u32::from(*count) - 1);
            (!rid.is_null()).then(|| *rid)
        }
    }

    fn creation_time(process: HANDLE) -> Option<String> {
        let mut creation = Default::default();
        let mut exit = Default::default();
        let mut kernel = Default::default();
        let mut user = Default::default();
        if unsafe { GetProcessTimes(process, &mut creation, &mut exit, &mut kernel, &mut user) } == 0 {
            return None;
        }
        Some((((creation.dwHighDateTime as u64) << 32) | creation.dwLowDateTime as u64).to_string())
    }

    fn executable_name(process: HANDLE) -> Option<String> {
        let mut name = vec![0u16; 32768];
        let mut size = name.len() as u32;
        if unsafe { QueryFullProcessImageNameW(process, 0, name.as_mut_ptr(), &mut size) } == 0 {
            return None;
        }
        let path = String::from_utf16(&name[..size as usize]).ok()?;
        path.rsplit(['\\', '/']).next().map(str::to_owned)
    }

    fn gui(thread: u32) -> Option<GUITHREADINFO> {
        if thread == 0 { return None; }
        let mut info = GUITHREADINFO { cbSize: std::mem::size_of::<GUITHREADINFO>() as u32,
            ..Default::default() };
        (unsafe { GetGUIThreadInfo(thread, &mut info) } != 0).then_some(info)
    }

    fn ime(focus: HWND) -> (bool, Option<i32>, bool) {
        if focus.is_null() { return (false, None, false); }
        let context = unsafe { ImmGetContext(focus) };
        if context.is_null() { return (false, None, false); }
        // 空指针 + 长度 0 只查询字节数，绝不分配或读取组合字符串。
        let bytes = unsafe { ImmGetCompositionStringW(context, GCS_COMPSTR, std::ptr::null_mut(), 0) };
        let released = unsafe { ImmReleaseContext(focus, context) } != 0;
        (true, Some(bytes), released)
    }

    pub(super) fn observe_foreground() -> TargetObservation {
        let own_token = token(unsafe { GetCurrentProcess() });
        let own_integrity = own_token.as_ref().and_then(|t| integrity(t.as_raw_handle()));
        let hwnd = unsafe { GetForegroundWindow() };
        let mut pid = 0u32;
        let thread = unsafe { GetWindowThreadProcessId(hwnd, &mut pid) };
        let root = unsafe { GetAncestor(hwnd, GA_ROOT) };
        let owner = unsafe { GetWindow(hwnd, GW_OWNER) };
        let raw_process = if pid == 0 { std::ptr::null_mut() } else {
            unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SYNCHRONIZE, 0, pid) }
        };
        let process = (!raw_process.is_null()).then(|| unsafe { OwnedHandle::from_raw_handle(raw_process) });
        let target_token = process.as_ref().and_then(|p| token(p.as_raw_handle()));
        let target_integrity = target_token.as_ref().and_then(|t| integrity(t.as_raw_handle()));
        let elevated = target_token.as_ref().and_then(|t| token_bool(t.as_raw_handle(), TokenElevation));
        let ui_access = target_token.as_ref().and_then(|t| token_bool(t.as_raw_handle(), TokenUIAccess));
        let created = process.as_ref().and_then(|p| creation_time(p.as_raw_handle()));
        let name = process.as_ref().and_then(|p| executable_name(p.as_raw_handle()));
        let mut machine = 0u16;
        let mut native_machine = 0u16;
        let architecture_known = process.as_ref().is_some_and(|p| unsafe {
            IsWow64Process2(p.as_raw_handle(), &mut machine, &mut native_machine) != 0
        });
        let focus = gui(thread).map(|g| g.hwndFocus).unwrap_or(std::ptr::null_mut());
        let mut focus_pid = 0u32;
        let focus_thread = unsafe { GetWindowThreadProcessId(focus, &mut focus_pid) };
        let layout = if focus_thread == 0 { std::ptr::null_mut() } else { unsafe { GetKeyboardLayout(focus_thread) } };
        let imm_is_ime = (!layout.is_null()).then(|| unsafe { ImmIsIME(layout) } != 0);
        let (context_present, composition_bytes, released) = ime(focus);

        let mut after_pid = 0u32;
        let after_thread = unsafe { GetWindowThreadProcessId(hwnd, &mut after_pid) };
        let stable = !hwnd.is_null() && !focus.is_null() && pid != 0 && focus_pid == pid
            && thread == after_thread && pid == after_pid
            && unsafe { IsWindow(hwnd) } != 0 && unsafe { IsWindow(focus) } != 0
            && unsafe { GetForegroundWindow() } == hwnd
            && gui(thread).is_some_and(|g| g.hwndFocus == focus)
            && created.is_some() && process.as_ref().is_some_and(|p| unsafe {
                WaitForSingleObject(p.as_raw_handle(), 0) == WAIT_TIMEOUT
            });
        TargetObservation {
            hwnd: id(hwnd), root_hwnd: id(root), owner_hwnd: id(owner), pid, thread_id: thread,
            process_created_filetime: created, executable_name: name,
            process_machine: architecture_known.then_some(machine),
            native_machine: architecture_known.then_some(native_machine),
            focus_hwnd: id(focus), focus_pid, focus_thread_id: focus_thread, identity_stable: stable,
            own_integrity_rid: own_integrity, target_integrity_rid: target_integrity,
            target_elevated: elevated, target_ui_access: ui_access,
            permission: if stable { permission_state(own_integrity, target_integrity, elevated, ui_access) }
                else { PermissionState::ManualOnlyUnknown },
            keyboard_layout: id(layout), imm_is_ime, imm_context_present: context_present,
            imm_composition_bytes: composition_bytes, imm_context_released: released,
            ime_scope: "imm_only_tsf_unverified",
            ime_state: ime_state(context_present, composition_bytes, stable && released),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn positive_imm_length_is_composing() {
        assert_eq!(ime_state(true, Some(4), true), ImeState::Composing);
    }

    #[test]
    fn zero_null_and_errors_never_prove_no_composition() {
        for (context, length) in [(false, None), (false, Some(0)), (true, Some(0)),
            (true, Some(-1)), (true, Some(-2)), (true, None), (true, Some(3))] {
            assert_eq!(ime_state(context, length, true), ImeState::Unknown);
        }
    }

    #[test]
    fn unstable_target_invalidates_ime_observation() {
        assert_eq!(ime_state(true, Some(4), false), ImeState::Unknown);
    }

    #[test]
    fn higher_or_unknown_privilege_is_manual_only() {
        assert_eq!(permission_state(Some(0x2000), Some(0x2000), Some(false), Some(false)), PermissionState::Normal);
        for (target, elevated, ui) in [(Some(0x3000), Some(true), Some(false)),
            (Some(0x2000), Some(false), Some(true)), (Some(0x2000), Some(true), Some(false))] {
            assert_eq!(permission_state(Some(0x2000), target, elevated, ui), PermissionState::ManualOnlyHigherPrivilege);
        }
        for (own, target, elevated, ui) in [(None, Some(0x2000), Some(false), Some(false)),
            (Some(0x2000), None, Some(false), Some(false)),
            (Some(0x2000), Some(0x2000), None, Some(false)),
            (Some(0x2000), Some(0x2000), Some(false), None),
            (Some(0x3000), Some(0x2000), Some(false), Some(false))] {
            assert_eq!(permission_state(own, target, elevated, ui), PermissionState::ManualOnlyUnknown);
        }
    }
}

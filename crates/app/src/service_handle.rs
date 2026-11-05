#[cfg(windows)]
mod windows {
    use windows_sys::Win32::Foundation::GetLastError;
    use windows_sys::Win32::System::Services::{
        CloseServiceHandle, OpenSCManagerW, OpenServiceW, SC_HANDLE,
    };

    pub(crate) struct ScHandle(SC_HANDLE);

    impl ScHandle {
        pub(crate) fn open_manager(access: u32) -> Result<Self, String> {
            let handle = unsafe { OpenSCManagerW(std::ptr::null(), std::ptr::null(), access) };
            Self::owned(handle).ok_or_else(|| last_error("OpenSCManagerW"))
        }

        pub(crate) fn open_service(&self, name: &[u16], access: u32) -> Result<Self, u32> {
            let handle = unsafe { OpenServiceW(self.0, name.as_ptr(), access) };
            if handle.is_null() {
                Err(error_code())
            } else {
                Ok(Self(handle))
            }
        }

        pub(crate) fn owned(handle: SC_HANDLE) -> Option<Self> {
            (!handle.is_null()).then_some(Self(handle))
        }

        pub(crate) fn raw(&self) -> SC_HANDLE {
            self.0
        }
    }

    impl Drop for ScHandle {
        fn drop(&mut self) {
            unsafe { CloseServiceHandle(self.0) };
        }
    }

    pub(crate) fn error_code() -> u32 {
        unsafe { GetLastError() }
    }

    pub(crate) fn last_error(operation: &str) -> String {
        let code = error_code();
        format!(
            "{operation} failed: {} (code {code})",
            std::io::Error::from_raw_os_error(code as i32)
        )
    }
}

#[cfg(windows)]
pub(crate) use windows::{ScHandle, error_code, last_error};

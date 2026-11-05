#[cfg(windows)]
fn main() -> std::process::ExitCode {
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Foundation::{
        CloseHandle, GENERIC_READ, GENERIC_WRITE, INVALID_HANDLE_VALUE,
    };
    use windows_sys::Win32::Storage::FileSystem::{
        CreateFileW, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING,
    };
    use windows_sys::Win32::System::IO::DeviceIoControl;

    const DEVICE: &str = r"\\?\ACPI#CLV0001#1#{86994c74-ad43-4812-b7e7-0c420b5c5fd7}";
    const IOCTL_ACPI_READ: u32 = 0x0032_2400;
    const REQUEST_LEN: usize = 0x420;
    const RESPONSE_LEN: usize = 0x40c;
    const READ_GUID: [u8; 16] = [
        0xE4, 0x24, 0xF2, 0x93, 0xDC, 0xFB, 0xBF, 0x4B, 0xAD, 0xD6, 0xDB, 0x71, 0xBD, 0xC0, 0xAF,
        0xAD,
    ];

    let args = std::env::args().skip(1).collect::<Vec<_>>();
    let command = args
        .first()
        .and_then(|value| value.parse::<i32>().ok())
        .unwrap_or(12);
    let payload = args
        .get(1)
        .map(|value| decode_hex(value))
        .transpose()
        .unwrap_or_else(|error| {
            eprintln!("invalid payload: {error}");
            std::process::exit(2);
        })
        .unwrap_or_default();

    let wide = OsStr::new(DEVICE)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    let handle = unsafe {
        CreateFileW(
            wide.as_ptr(),
            GENERIC_READ | GENERIC_WRITE,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            std::ptr::null(),
            OPEN_EXISTING,
            0,
            std::ptr::null_mut(),
        )
    };
    if handle == INVALID_HANDLE_VALUE {
        eprintln!("CreateFileW failed: {}", std::io::Error::last_os_error());
        return std::process::ExitCode::FAILURE;
    }

    let mut request = [0_u8; REQUEST_LEN];
    request[0..16].copy_from_slice(&READ_GUID);
    request[20..24].copy_from_slice(b"_DSM");
    request[24..28].copy_from_slice(&command.to_le_bytes());
    request[28..30].copy_from_slice(&0x0104_u16.to_le_bytes());
    request[30..34].copy_from_slice(&0x0100_0002_u32.to_le_bytes());
    let copy_len = payload.len().min(0x100);
    request[34..34 + copy_len].copy_from_slice(&payload[..copy_len]);

    let mut response = [0_u8; RESPONSE_LEN];
    let mut returned = 0_u32;
    let ok = unsafe {
        DeviceIoControl(
            handle,
            IOCTL_ACPI_READ,
            request.as_ptr().cast(),
            REQUEST_LEN as u32,
            response.as_mut_ptr().cast(),
            RESPONSE_LEN as u32,
            &mut returned,
            std::ptr::null_mut(),
        )
    };
    unsafe { CloseHandle(handle) };
    if ok == 0 {
        eprintln!(
            "DeviceIoControl failed: {}",
            std::io::Error::last_os_error()
        );
        return std::process::ExitCode::FAILURE;
    }

    println!("command={command} returned={returned}");
    for (offset, chunk) in response[..returned.min(RESPONSE_LEN as u32) as usize]
        .chunks(32)
        .enumerate()
    {
        let hex = chunk
            .iter()
            .map(|byte| format!("{byte:02X}"))
            .collect::<Vec<_>>()
            .join(" ");
        println!("{:04X}: {hex}", offset * 32);
    }
    std::process::ExitCode::SUCCESS
}

#[cfg(windows)]
fn decode_hex(value: &str) -> Result<Vec<u8>, String> {
    let value = value.trim();
    if !value.len().is_multiple_of(2) {
        return Err("odd number of hex digits".to_owned());
    }
    (0..value.len())
        .step_by(2)
        .map(|index| {
            u8::from_str_radix(&value[index..index + 2], 16).map_err(|error| error.to_string())
        })
        .collect()
}

#[cfg(not(windows))]
fn main() {
    eprintln!("neo-acpi-probe requires Windows");
}

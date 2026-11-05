#[derive(Clone, Debug, Default)]
pub struct GpuSnapshot {
    pub utilization_percent: u32,
    pub temperature_c: u32,
    pub clock_mhz: u32,
    pub max_clock_mhz: u32,
    pub memory_used_bytes: u64,
    pub memory_total_bytes: u64,
}

#[derive(Clone, Debug, Default)]
pub struct BatterySnapshot {
    pub ac_online: bool,
    pub charging: bool,
    pub percent: Option<u8>,
    pub remaining_mwh: u32,
    pub max_capacity_mwh: u32,
    pub rate_mw: u32,
    pub seconds_remaining: Option<u32>,
}

#[derive(Clone, Debug, Default)]
pub struct SystemSnapshot {
    pub cpu_load_percent: f32,
    pub cpu_clock_mhz: u32,
    pub cpu_max_clock_mhz: u32,
    pub memory_used_bytes: u64,
    pub memory_total_bytes: u64,
    pub disk_used_bytes: u64,
    pub disk_total_bytes: u64,
    pub gpu: Option<GpuSnapshot>,
    pub battery: Option<BatterySnapshot>,
}

impl SystemSnapshot {
    pub fn memory_percent(&self) -> u32 {
        percent(self.memory_used_bytes, self.memory_total_bytes)
    }

    pub fn disk_percent(&self) -> u32 {
        percent(self.disk_used_bytes, self.disk_total_bytes)
    }
}

fn percent(used: u64, total: u64) -> u32 {
    if total == 0 {
        return 0;
    }
    ((used as f64 / total as f64) * 100.0)
        .round()
        .clamp(0.0, 100.0) as u32
}

#[cfg(windows)]
pub use windows_impl::SystemSampler;

#[cfg(not(windows))]
pub use demo_impl::SystemSampler;

#[cfg(windows)]
mod windows_impl {
    use super::{BatterySnapshot, GpuSnapshot, SystemSnapshot};
    use std::ffi::c_void;
    use std::os::windows::ffi::OsStrExt;
    use std::path::Path;
    use windows_sys::Win32::Foundation::{FILETIME, FreeLibrary, HMODULE};
    use windows_sys::Win32::Storage::FileSystem::GetDiskFreeSpaceExW;
    use windows_sys::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryW};
    use windows_sys::Win32::System::Performance::{
        PDH_CSTATUS_NEW_DATA, PDH_CSTATUS_VALID_DATA, PDH_FMT_COUNTERVALUE, PDH_FMT_DOUBLE,
        PDH_HCOUNTER, PDH_HQUERY, PdhAddEnglishCounterW, PdhCloseQuery, PdhCollectQueryData,
        PdhGetFormattedCounterValue, PdhOpenQueryW,
    };
    use windows_sys::Win32::System::Power::{
        CallNtPowerInformation, GetSystemPowerStatus, ProcessorInformation, SYSTEM_BATTERY_STATE,
        SYSTEM_POWER_STATUS, SystemBatteryState,
    };
    use windows_sys::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};
    use windows_sys::Win32::System::Threading::{
        ALL_PROCESSOR_GROUPS, GetActiveProcessorCount, GetSystemTimes,
    };

    #[derive(Clone, Copy)]
    struct CpuTimes {
        idle: u64,
        kernel: u64,
        user: u64,
    }

    pub struct SystemSampler {
        previous_cpu: Option<CpuTimes>,
        cpu_frequency: Option<CpuFrequencySampler>,
        nvml: Option<Nvml>,
    }

    impl SystemSampler {
        pub fn new() -> Self {
            Self {
                previous_cpu: None,
                cpu_frequency: CpuFrequencySampler::open(),
                nvml: Nvml::open(),
            }
        }

        pub fn sample(&mut self) -> SystemSnapshot {
            let cpu = read_cpu_times();
            let cpu_load_percent = match (self.previous_cpu, cpu) {
                (Some(previous), Some(current)) => {
                    let idle = current.idle.saturating_sub(previous.idle);
                    let kernel = current.kernel.saturating_sub(previous.kernel);
                    let user = current.user.saturating_sub(previous.user);
                    let total = kernel.saturating_add(user);
                    if total == 0 {
                        0.0
                    } else {
                        ((total.saturating_sub(idle)) as f64 / total as f64 * 100.0)
                            .clamp(0.0, 100.0) as f32
                    }
                }
                _ => 0.0,
            };
            self.previous_cpu = cpu;

            let (memory_used_bytes, memory_total_bytes) = read_memory();
            let (disk_used_bytes, disk_total_bytes) = read_disk();
            let (cpu_clock_mhz, cpu_max_clock_mhz) = self
                .cpu_frequency
                .as_mut()
                .map(CpuFrequencySampler::sample)
                .unwrap_or_else(read_processor_power);

            SystemSnapshot {
                cpu_load_percent,
                cpu_clock_mhz,
                cpu_max_clock_mhz,
                memory_used_bytes,
                memory_total_bytes,
                disk_used_bytes,
                disk_total_bytes,
                gpu: self.nvml.as_ref().and_then(Nvml::sample),
                battery: read_battery(),
            }
        }
    }

    fn read_cpu_times() -> Option<CpuTimes> {
        let mut idle = FILETIME::default();
        let mut kernel = FILETIME::default();
        let mut user = FILETIME::default();
        let ok = unsafe { GetSystemTimes(&mut idle, &mut kernel, &mut user) };
        (ok != 0).then(|| CpuTimes {
            idle: filetime(idle),
            kernel: filetime(kernel),
            user: filetime(user),
        })
    }

    fn filetime(value: FILETIME) -> u64 {
        (u64::from(value.dwHighDateTime) << 32) | u64::from(value.dwLowDateTime)
    }

    fn read_memory() -> (u64, u64) {
        let mut status = MEMORYSTATUSEX {
            dwLength: std::mem::size_of::<MEMORYSTATUSEX>() as u32,
            ..Default::default()
        };
        if unsafe { GlobalMemoryStatusEx(&mut status) } == 0 {
            return (0, 0);
        }
        (
            status.ullTotalPhys.saturating_sub(status.ullAvailPhys),
            status.ullTotalPhys,
        )
    }

    fn read_disk() -> (u64, u64) {
        let mut free = 0_u64;
        let mut total = 0_u64;
        let mut total_free = 0_u64;
        let path: Vec<u16> = std::ffi::OsStr::new(r"C:\")
            .encode_wide()
            .chain(Some(0))
            .collect();
        let ok =
            unsafe { GetDiskFreeSpaceExW(path.as_ptr(), &mut free, &mut total, &mut total_free) };
        if ok == 0 {
            (0, 0)
        } else {
            (total.saturating_sub(total_free), total)
        }
    }

    fn read_battery() -> Option<BatterySnapshot> {
        let mut status = SYSTEM_POWER_STATUS::default();
        if unsafe { GetSystemPowerStatus(&mut status) } == 0 {
            return None;
        }
        if status.BatteryFlag == 255 || status.BatteryFlag & 128 != 0 {
            return None;
        }

        let mut state = SYSTEM_BATTERY_STATE::default();
        let size = std::mem::size_of::<SYSTEM_BATTERY_STATE>() as u32;
        let state_status = unsafe {
            CallNtPowerInformation(
                SystemBatteryState,
                std::ptr::null(),
                0,
                (&mut state as *mut SYSTEM_BATTERY_STATE).cast(),
                size,
            )
        };
        let state_valid = state_status >= 0 && state.BatteryPresent;
        let seconds_remaining = if status.BatteryLifeTime != u32::MAX && status.BatteryLifeTime != 0
        {
            Some(status.BatteryLifeTime)
        } else if state_valid && state.EstimatedTime != 0 && state.EstimatedTime != u32::MAX {
            Some(state.EstimatedTime)
        } else {
            None
        };

        Some(BatterySnapshot {
            ac_online: status.ACLineStatus == 1 || (state_valid && state.AcOnLine),
            charging: status.BatteryFlag & 8 != 0 || (state_valid && state.Charging),
            percent: (status.BatteryLifePercent <= 100).then_some(status.BatteryLifePercent),
            remaining_mwh: if state_valid {
                state.RemainingCapacity
            } else {
                0
            },
            max_capacity_mwh: if state_valid { state.MaxCapacity } else { 0 },
            rate_mw: if state_valid { state.Rate } else { 0 },
            seconds_remaining,
        })
    }

    struct CpuFrequencySampler {
        query: PDH_HQUERY,
        frequency: Option<PDH_HCOUNTER>,
        performance: Option<PDH_HCOUNTER>,
        nominal_mhz: u32,
    }

    impl CpuFrequencySampler {
        fn open() -> Option<Self> {
            let (_, nominal_mhz) = read_processor_power();
            let mut query = std::ptr::null_mut();
            if unsafe { PdhOpenQueryW(std::ptr::null(), 0, &mut query) } != 0 {
                return None;
            }
            let frequency =
                add_english_counter(query, r"\Processor Information(_Total)\Processor Frequency");
            let performance = add_english_counter(
                query,
                r"\Processor Information(_Total)\% Processor Performance",
            );
            if frequency.is_none() && performance.is_none() {
                unsafe { PdhCloseQuery(query) };
                return None;
            }
            unsafe { PdhCollectQueryData(query) };
            Some(Self {
                query,
                frequency,
                performance,
                nominal_mhz,
            })
        }

        fn sample(&mut self) -> (u32, u32) {
            if unsafe { PdhCollectQueryData(self.query) } != 0 {
                return (0, self.nominal_mhz);
            }
            let frequency = self.frequency.and_then(formatted_value).unwrap_or(0.0);
            let performance = self.performance.and_then(formatted_value).unwrap_or(0.0);
            let current = if performance > 0.0 && self.nominal_mhz > 0 {
                (performance / 100.0 * f64::from(self.nominal_mhz)).round() as u32
            } else if frequency > 0.0 {
                frequency.round() as u32
            } else {
                0
            };
            (current, self.nominal_mhz)
        }
    }

    impl Drop for CpuFrequencySampler {
        fn drop(&mut self) {
            unsafe { PdhCloseQuery(self.query) };
        }
    }

    fn add_english_counter(query: PDH_HQUERY, path: &str) -> Option<PDH_HCOUNTER> {
        let path = path
            .encode_utf16()
            .chain(std::iter::once(0))
            .collect::<Vec<_>>();
        let mut counter = std::ptr::null_mut();
        let status = unsafe { PdhAddEnglishCounterW(query, path.as_ptr(), 0, &mut counter) };
        (status == 0).then_some(counter)
    }

    fn formatted_value(counter: PDH_HCOUNTER) -> Option<f64> {
        let mut value = PDH_FMT_COUNTERVALUE::default();
        let mut counter_type = 0_u32;
        let status = unsafe {
            PdhGetFormattedCounterValue(counter, PDH_FMT_DOUBLE, &mut counter_type, &mut value)
        };
        if status != 0
            || (value.CStatus != PDH_CSTATUS_VALID_DATA && value.CStatus != PDH_CSTATUS_NEW_DATA)
        {
            return None;
        }
        let value = unsafe { value.Anonymous.doubleValue };
        value.is_finite().then_some(value)
    }

    #[repr(C)]
    #[derive(Clone, Copy, Default)]
    struct ProcessorPowerInformation {
        number: u32,
        max_mhz: u32,
        current_mhz: u32,
        mhz_limit: u32,
        max_idle_state: u32,
        current_idle_state: u32,
    }

    fn read_processor_power() -> (u32, u32) {
        let count = unsafe { GetActiveProcessorCount(ALL_PROCESSOR_GROUPS) }.max(1) as usize;
        let mut info = vec![ProcessorPowerInformation::default(); count];
        let bytes = std::mem::size_of_val(info.as_slice()) as u32;
        let status = unsafe {
            CallNtPowerInformation(
                ProcessorInformation,
                std::ptr::null(),
                0,
                info.as_mut_ptr().cast(),
                bytes,
            )
        };
        if status < 0 {
            return (0, 0);
        }
        let current = info.iter().map(|item| item.current_mhz).max().unwrap_or(0);
        let maximum = info.iter().map(|item| item.max_mhz).max().unwrap_or(0);
        (current, maximum)
    }

    #[repr(C)]
    #[derive(Default)]
    struct NvmlUtilization {
        gpu: u32,
        memory: u32,
    }

    #[repr(C)]
    #[derive(Default)]
    struct NvmlMemory {
        total: u64,
        free: u64,
        used: u64,
    }

    type Device = *mut c_void;

    struct Nvml {
        module: HMODULE,
        device: Device,
        get_utilization: unsafe extern "system" fn(Device, *mut NvmlUtilization) -> i32,
        get_temperature: unsafe extern "system" fn(Device, i32, *mut u32) -> i32,
        get_clock: unsafe extern "system" fn(Device, i32, *mut u32) -> i32,
        get_memory: unsafe extern "system" fn(Device, *mut NvmlMemory) -> i32,
    }

    impl Nvml {
        fn open() -> Option<Self> {
            let module = load_nvml()?;
            unsafe {
                let init: Option<unsafe extern "system" fn() -> i32> =
                    symbol(module, b"nvmlInit_v2\0");
                let get_count: Option<unsafe extern "system" fn(*mut u32) -> i32> =
                    symbol(module, b"nvmlDeviceGetCount_v2\0");
                let get_handle: Option<unsafe extern "system" fn(u32, *mut Device) -> i32> =
                    symbol(module, b"nvmlDeviceGetHandleByIndex_v2\0");
                let get_utilization = symbol(module, b"nvmlDeviceGetUtilizationRates\0")?;
                let get_temperature = symbol(module, b"nvmlDeviceGetTemperature\0")?;
                let get_clock = symbol(module, b"nvmlDeviceGetClockInfo\0")?;
                let get_memory = symbol(module, b"nvmlDeviceGetMemoryInfo\0")?;

                if init?() != 0 {
                    FreeLibrary(module);
                    return None;
                }
                let mut count = 0_u32;
                if get_count?(&mut count) != 0 || count == 0 {
                    FreeLibrary(module);
                    return None;
                }
                let mut device = std::ptr::null_mut();
                if get_handle?(0, &mut device) != 0 || device.is_null() {
                    FreeLibrary(module);
                    return None;
                }
                Some(Self {
                    module,
                    device,
                    get_utilization,
                    get_temperature,
                    get_clock,
                    get_memory,
                })
            }
        }

        fn sample(&self) -> Option<GpuSnapshot> {
            let mut utilization = NvmlUtilization::default();
            let mut temperature = 0_u32;
            let mut clock = 0_u32;
            let mut max_clock = 0_u32;
            let mut memory = NvmlMemory::default();
            unsafe {
                if (self.get_utilization)(self.device, &mut utilization) != 0
                    || (self.get_temperature)(self.device, 0, &mut temperature) != 0
                    || (self.get_clock)(self.device, 0, &mut clock) != 0
                    || (self.get_clock)(self.device, 1, &mut max_clock) != 0
                    || (self.get_memory)(self.device, &mut memory) != 0
                {
                    return None;
                }
            }
            Some(GpuSnapshot {
                utilization_percent: utilization.gpu.min(100),
                temperature_c: temperature,
                clock_mhz: clock,
                max_clock_mhz: max_clock,
                memory_used_bytes: memory.used,
                memory_total_bytes: memory.total,
            })
        }
    }

    impl Drop for Nvml {
        fn drop(&mut self) {
            unsafe {
                FreeLibrary(self.module);
            }
        }
    }

    unsafe fn symbol<T: Copy>(module: HMODULE, name: &[u8]) -> Option<T> {
        let proc = unsafe { GetProcAddress(module, name.as_ptr()) }?;
        Some(unsafe { std::mem::transmute_copy(&proc) })
    }

    fn load_nvml() -> Option<HMODULE> {
        let mut candidates = vec![
            "nvml.dll".to_owned(),
            r"C:\Program Files\NVIDIA Corporation\NVSMI\nvml.dll".to_owned(),
            r"C:\Windows\System32\nvml.dll".to_owned(),
        ];
        if let Ok(root) = std::env::var("ProgramFiles") {
            candidates.push(
                Path::new(&root)
                    .join("NVIDIA Corporation")
                    .join("NVSMI")
                    .join("nvml.dll")
                    .display()
                    .to_string(),
            );
        }
        for candidate in candidates {
            let mut wide: Vec<u16> = std::ffi::OsStr::new(&candidate)
                .encode_wide()
                .chain(Some(0))
                .collect();
            let module = unsafe { LoadLibraryW(wide.as_mut_ptr()) };
            if !module.is_null() {
                return Some(module);
            }
        }
        None
    }
}

#[cfg(not(windows))]
mod demo_impl {
    use super::SystemSnapshot;
    use std::time::Instant;

    pub struct SystemSampler {
        started: Instant,
    }

    impl SystemSampler {
        pub fn new() -> Self {
            Self {
                started: Instant::now(),
            }
        }

        pub fn sample(&mut self) -> SystemSnapshot {
            let t = self.started.elapsed().as_secs_f32();
            SystemSnapshot {
                cpu_load_percent: 18.0 + (t * 0.9).sin() * 7.0,
                cpu_clock_mhz: 4_260,
                cpu_max_clock_mhz: 5_200,
                memory_used_bytes: 12_884_901_888,
                memory_total_bytes: 34_359_738_368,
                disk_used_bytes: 614_000_000_000,
                disk_total_bytes: 1_024_000_000_000,
                gpu: Some(super::GpuSnapshot {
                    utilization_percent: 34,
                    temperature_c: 55,
                    clock_mhz: 2_100,
                    max_clock_mhz: 2_595,
                    memory_used_bytes: 2_400_000_000,
                    memory_total_bytes: 8_589_934_592,
                }),
                battery: Some(super::BatterySnapshot {
                    ac_online: true,
                    charging: false,
                    percent: Some(78),
                    remaining_mwh: 62_400,
                    max_capacity_mwh: 80_000,
                    rate_mw: 18_500,
                    seconds_remaining: Some(12_000),
                }),
            }
        }
    }
}

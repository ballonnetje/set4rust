//! The process's resources, sampled once a second on a thread of its own while the monitor is
//! shown (nothing runs while it is hidden): memory (the process's and the machine's), CPU time of
//! the process, of each thread and of the whole machine, the GPU (its name and memory from DXGI,
//! this process's dedicated memory and 3D engine use from the `GPU Process Memory` / `GPU Engine`
//! performance counters through PDH). On Linux from `/proc` (memory, CPU of the process, its threads
//! and the machine) and `/sys/class/drm` (the card's PCI ids; its memory where the driver tells it,
//! amdgpu); no per-process GPU numbers.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// One sample.
#[derive(Clone, Debug, Default)]
pub struct Sample {
    /// Working set and private bytes.
    pub working_set: u64,
    pub private: u64,
    /// Dedicated GPU memory of this process (None: no counter).
    pub gpu: Option<u64>,
    /// CPU used since the last sample, in % of one core: the process, then each thread that used
    /// any (name or id), busiest first.
    pub cpu: f32,
    pub threads: Vec<(String, f32)>,
    /// Logical cores (the process can use `100 · cores` %).
    pub cores: u32,
    /// The whole machine's CPU in use, % of all cores.
    pub machine_cpu: f32,
    /// The machine's RAM: installed and free, bytes.
    pub ram_total: u64,
    pub ram_free: u64,
    /// The GPU (the DXGI adapter with the most dedicated memory): its name and memory, bytes.
    pub gpu_name: String,
    pub vram_total: u64,
    /// This process's 3D engine use, % (None: no counter).
    pub gpu_3d: Option<f32>,
}

/// The sampler thread; stops when dropped.
pub struct Sampler {
    stop: Arc<AtomicBool>,
    pub latest: Arc<Mutex<Option<Sample>>>,
}

impl Sampler {
    pub fn start() -> Sampler {
        let stop = Arc::new(AtomicBool::new(false));
        let latest = Arc::new(Mutex::new(None));
        let (s, l) = (stop.clone(), latest.clone());
        let _ = std::thread::Builder::new().name("monitor".into()).spawn(move || {
            let mut probe = Probe::new();
            while !s.load(Ordering::Relaxed) {
                let t = Instant::now();
                let sample = probe.sample();
                *l.lock().unwrap_or_else(|e| e.into_inner()) = Some(sample);
                // A second apart, but wake early to stop.
                while t.elapsed() < Duration::from_secs(1) && !s.load(Ordering::Relaxed) {
                    std::thread::sleep(Duration::from_millis(50));
                }
            }
        });
        Sampler { stop, latest }
    }
}

impl Drop for Sampler {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

#[cfg(windows)]
use win::Probe;

#[cfg(target_os = "linux")]
use linux::Probe;

#[cfg(not(any(windows, target_os = "linux")))]
struct Probe;

#[cfg(not(any(windows, target_os = "linux")))]
impl Probe {
    fn new() -> Probe {
        Probe
    }
    fn sample(&mut self) -> Sample {
        Sample { cores: std::thread::available_parallelism().map_or(1, |n| n.get() as u32), ..Default::default() }
    }
}

#[cfg(windows)]
mod win {
    use super::Sample;
    use std::collections::HashMap;
    use std::time::Instant;
    use windows_sys::Win32::Foundation::{CloseHandle, FILETIME, INVALID_HANDLE_VALUE, LocalFree};
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{CreateToolhelp32Snapshot, TH32CS_SNAPTHREAD, THREADENTRY32, Thread32First, Thread32Next};
    use windows_sys::Win32::System::Performance::{PDH_FMT, PDH_FMT_COUNTERVALUE_ITEM_W, PDH_FMT_DOUBLE, PDH_FMT_LARGE, PdhAddEnglishCounterW, PdhCloseQuery, PdhCollectQueryData, PdhGetFormattedCounterArrayW, PdhOpenQueryW};
    use windows_sys::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};
    use windows_sys::Win32::System::ProcessStatus::{K32GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS, PROCESS_MEMORY_COUNTERS_EX};
    use windows_sys::Win32::System::Threading::{GetCurrentProcess, GetProcessTimes, GetSystemTimes, GetThreadDescription, GetThreadTimes, OpenThread, THREAD_QUERY_LIMITED_INFORMATION};

    /// The DXGI adapter with the most dedicated memory (not a software one): its name and that
    /// memory. DXGI's COM calls by their vtable slots: `IDXGIFactory1::EnumAdapters1` (12),
    /// `IDXGIAdapter1::GetDesc1` (10), `Release` (2).
    fn gpu_adapter() -> Option<(String, u64)> {
        #[repr(C)]
        struct Guid(u32, u16, u16, [u8; 8]);
        #[repr(C)]
        struct Desc1 {
            description: [u16; 128],
            vendor: u32,
            device: u32,
            subsys: u32,
            revision: u32,
            dedicated_video: usize,
            dedicated_system: usize,
            shared_system: usize,
            luid: (u32, i32),
            flags: u32,
        }
        #[link(name = "dxgi")]
        unsafe extern "system" {
            fn CreateDXGIFactory1(riid: *const Guid, factory: *mut *mut std::ffi::c_void) -> i32;
        }
        type Vt = *const *const std::ffi::c_void;
        const IID_FACTORY1: Guid = Guid(0x770aae78, 0xf26f, 0x4dba, [0xa8, 0x29, 0x25, 0x3c, 0x83, 0xd1, 0xb3, 0x87]);
        let mut best: Option<(String, u64)> = None;
        // SAFETY: COM objects from DXGI used through their documented vtable layouts (IUnknown 3,
        // IDXGIObject 4, IDXGIFactory 5 then EnumAdapters1; IDXGIAdapter 3 then GetDesc1), each
        // released once; out-pointers to locals.
        unsafe {
            let mut f: *mut std::ffi::c_void = std::ptr::null_mut();
            if CreateDXGIFactory1(&IID_FACTORY1, &mut f) < 0 || f.is_null() {
                return None;
            }
            let fvt = *(f as *const Vt);
            let enum1: extern "system" fn(*mut std::ffi::c_void, u32, *mut *mut std::ffi::c_void) -> i32 = std::mem::transmute(*fvt.add(12));
            let release_f: extern "system" fn(*mut std::ffi::c_void) -> u32 = std::mem::transmute(*fvt.add(2));
            for i in 0..16 {
                let mut a: *mut std::ffi::c_void = std::ptr::null_mut();
                if enum1(f, i, &mut a) < 0 || a.is_null() {
                    break;
                }
                let avt = *(a as *const Vt);
                let desc1: extern "system" fn(*mut std::ffi::c_void, *mut Desc1) -> i32 = std::mem::transmute(*avt.add(10));
                let release_a: extern "system" fn(*mut std::ffi::c_void) -> u32 = std::mem::transmute(*avt.add(2));
                let mut d: Desc1 = std::mem::zeroed();
                if desc1(a, &mut d) >= 0 && d.flags & 2 == 0 {
                    let len = d.description.iter().position(|&c| c == 0).unwrap_or(128);
                    let name = String::from_utf16_lossy(&d.description[..len]);
                    if best.as_ref().is_none_or(|b| d.dedicated_video as u64 > b.1) {
                        best = Some((name, d.dedicated_video as u64));
                    }
                }
                release_a(a);
            }
            release_f(f);
        }
        best
    }

    /// The machine's CPU times: (idle, kernel + user), 100 ns.
    fn system_times() -> (u64, u64) {
        let z = FILETIME { dwLowDateTime: 0, dwHighDateTime: 0 };
        let (mut i, mut k, mut u) = (z, z, z);
        // SAFETY: out-pointers to locals.
        if unsafe { GetSystemTimes(&mut i, &mut k, &mut u) } == 0 {
            return (0, 0);
        }
        (ticks(i), ticks(k) + ticks(u))
    }

    fn ticks(f: FILETIME) -> u64 {
        (f.dwHighDateTime as u64) << 32 | f.dwLowDateTime as u64
    }

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain([0]).collect()
    }

    /// The previous CPU times (100 ns) and when they were read.
    pub struct Probe {
        at: Instant,
        process: u64,
        threads: HashMap<u32, u64>,
        names: HashMap<u32, String>,
        /// The PDH query and its counters: dedicated GPU memory, GPU engine use (0: none).
        pdh: (isize, isize, isize),
        cores: u32,
        /// The machine's CPU times at the last sample.
        system: (u64, u64),
        gpu: Option<(String, u64)>,
    }

    impl Drop for Probe {
        fn drop(&mut self) {
            if self.pdh.0 != 0 {
                // SAFETY: a query this probe opened.
                unsafe { PdhCloseQuery(self.pdh.0) };
            }
        }
    }

    impl Probe {
        pub fn new() -> Probe {
            let (mut q, mut mem, mut eng) = (0isize, 0isize, 0isize);
            // SAFETY: out-pointers to locals; the paths are NUL-terminated wide strings.
            let pdh = unsafe {
                if PdhOpenQueryW(std::ptr::null(), 0, &mut q) == 0 {
                    if PdhAddEnglishCounterW(q, wide("\\GPU Process Memory(*)\\Dedicated Usage").as_ptr(), 0, &mut mem) != 0 {
                        mem = 0;
                    }
                    if PdhAddEnglishCounterW(q, wide("\\GPU Engine(*)\\Utilization Percentage").as_ptr(), 0, &mut eng) != 0 {
                        eng = 0;
                    }
                    PdhCollectQueryData(q);
                }
                (q, mem, eng)
            };
            let cores = std::thread::available_parallelism().map_or(1, |n| n.get() as u32);
            let mut p = Probe { at: Instant::now(), process: 0, threads: HashMap::new(), names: HashMap::new(), pdh, cores, system: system_times(), gpu: gpu_adapter() };
            p.process = p.process_time();
            p.threads = p.thread_times();
            p
        }

        fn process_time(&self) -> u64 {
            let z = FILETIME { dwLowDateTime: 0, dwHighDateTime: 0 };
            let (mut c, mut e, mut k, mut u) = (z, z, z, z);
            // SAFETY: the pseudo handle and out-pointers to locals.
            let ok = unsafe { GetProcessTimes(GetCurrentProcess(), &mut c, &mut e, &mut k, &mut u) };
            if ok == 0 { 0 } else { ticks(k) + ticks(u) }
        }

        /// Every thread of this process: its CPU time (and its name, read once).
        fn thread_times(&mut self) -> HashMap<u32, u64> {
            let mut out = HashMap::new();
            let pid = std::process::id();
            // SAFETY: a snapshot handle checked before use and closed after; THREADENTRY32 with
            // its size set; thread handles opened for query, checked and closed.
            unsafe {
                let snap = CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0);
                if snap == INVALID_HANDLE_VALUE {
                    return out;
                }
                let mut te: THREADENTRY32 = std::mem::zeroed();
                te.dwSize = std::mem::size_of::<THREADENTRY32>() as u32;
                let mut more = Thread32First(snap, &mut te) != 0;
                while more {
                    if te.th32OwnerProcessID == pid {
                        let tid = te.th32ThreadID;
                        let h = OpenThread(THREAD_QUERY_LIMITED_INFORMATION, 0, tid);
                        if h != 0 {
                            let z = FILETIME { dwLowDateTime: 0, dwHighDateTime: 0 };
                            let (mut c, mut e, mut k, mut u) = (z, z, z, z);
                            if GetThreadTimes(h, &mut c, &mut e, &mut k, &mut u) != 0 {
                                out.insert(tid, ticks(k) + ticks(u));
                            }
                            self.names.entry(tid).or_insert_with(|| {
                                let mut p = std::ptr::null_mut();
                                let mut name = String::new();
                                if GetThreadDescription(h, &mut p) >= 0 && !p.is_null() {
                                    let len = (0..).take_while(|&i| *p.add(i) != 0).count();
                                    name = String::from_utf16_lossy(std::slice::from_raw_parts(p, len));
                                    LocalFree(p as _);
                                }
                                name
                            });
                            CloseHandle(h);
                        }
                    }
                    more = Thread32Next(snap, &mut te) != 0;
                }
                CloseHandle(snap);
            }
            out
        }

        fn memory(&self) -> (u64, u64) {
            // SAFETY: the pseudo handle and a struct of the size given.
            unsafe {
                let mut c: PROCESS_MEMORY_COUNTERS_EX = std::mem::zeroed();
                c.cb = std::mem::size_of::<PROCESS_MEMORY_COUNTERS_EX>() as u32;
                if K32GetProcessMemoryInfo(GetCurrentProcess(), &mut c as *mut _ as *mut PROCESS_MEMORY_COUNTERS, c.cb) == 0 {
                    return (0, 0);
                }
                (c.WorkingSetSize as u64, c.PrivateUsage as u64)
            }
        }

        /// The sum over this process's `pid_<pid>_…` instances of counter `c` (those whose name
        /// also contains `part`), as `fmt` (large or double).
        fn per_process(&self, c: isize, fmt: PDH_FMT, part: &str) -> Option<f64> {
            if c == 0 {
                return None;
            }
            let prefix: Vec<u16> = format!("pid_{}_", std::process::id()).encode_utf16().collect();
            let part: Vec<u16> = part.encode_utf16().collect();
            // SAFETY: the counter this probe made; the item buffer sized as PDH asked (u64-aligned),
            // its names read up to their NUL inside it.
            unsafe {
                let (mut size, mut count) = (0u32, 0u32);
                PdhGetFormattedCounterArrayW(c, fmt, &mut size, &mut count, std::ptr::null_mut());
                if size == 0 {
                    return None;
                }
                let mut buf = vec![0u64; (size as usize).div_ceil(8)];
                let items = buf.as_mut_ptr() as *mut PDH_FMT_COUNTERVALUE_ITEM_W;
                if PdhGetFormattedCounterArrayW(c, fmt, &mut size, &mut count, items) != 0 {
                    return None;
                }
                let mut sum = 0f64;
                for i in 0..count as usize {
                    let it = &*items.add(i);
                    let name = it.szName;
                    if name.is_null() || !(0..prefix.len()).all(|k| *name.add(k) == prefix[k]) {
                        continue;
                    }
                    let len = (0..).take_while(|&k| *name.add(k) != 0).count();
                    let name = std::slice::from_raw_parts(name, len);
                    if !part.is_empty() && !name.windows(part.len()).any(|w| w == part.as_slice()) {
                        continue;
                    }
                    sum += if fmt == PDH_FMT_LARGE { it.FmtValue.Anonymous.largeValue.max(0) as f64 } else { it.FmtValue.Anonymous.doubleValue.max(0.0) };
                }
                Some(sum)
            }
        }

        pub fn sample(&mut self) -> Sample {
            let now = Instant::now();
            let wall = now.duration_since(self.at).as_secs_f64().max(1e-3) * 1e7;
            self.at = now;
            let p = self.process_time();
            let cpu = (p.saturating_sub(self.process) as f64 / wall * 100.0) as f32;
            self.process = p;
            let t = self.thread_times();
            let mut threads: Vec<(String, f32)> = t
                .iter()
                .filter_map(|(tid, &v)| {
                    let d = v.saturating_sub(self.threads.get(tid).copied().unwrap_or(v));
                    let name = self.names.get(tid).filter(|n| !n.is_empty()).cloned().unwrap_or_else(|| format!("#{tid}"));
                    (d > 0).then(|| (name, (d as f64 / wall * 100.0) as f32))
                })
                .collect();
            threads.sort_by(|a, b| b.1.total_cmp(&a.1));
            self.threads = t;
            self.names.retain(|k, _| self.threads.contains_key(k));
            let (working_set, private) = self.memory();
            let (idle, busy) = system_times();
            let (d_idle, d_all) = (idle.saturating_sub(self.system.0), busy.saturating_sub(self.system.1));
            self.system = (idle, busy);
            // kernel time includes idle time.
            let machine_cpu = if d_all == 0 { 0.0 } else { (d_all.saturating_sub(d_idle) as f64 / d_all as f64 * 100.0) as f32 };
            // SAFETY: the query this probe opened.
            let collected = self.pdh.0 != 0 && unsafe { PdhCollectQueryData(self.pdh.0) } == 0;
            let gpu = if collected { self.per_process(self.pdh.1, PDH_FMT_LARGE, "").map(|v| v as u64) } else { None };
            let gpu_3d = if collected { self.per_process(self.pdh.2, PDH_FMT_DOUBLE, "engtype_3D").map(|v| v as f32) } else { None };
            // SAFETY: a struct of the size given.
            let (ram_total, ram_free) = unsafe {
                let mut m: MEMORYSTATUSEX = std::mem::zeroed();
                m.dwLength = std::mem::size_of::<MEMORYSTATUSEX>() as u32;
                if GlobalMemoryStatusEx(&mut m) == 0 { (0, 0) } else { (m.ullTotalPhys, m.ullAvailPhys) }
            };
            let (gpu_name, vram_total) = self.gpu.clone().unwrap_or_default();
            Sample { working_set, private, gpu, cpu, threads, cores: self.cores, machine_cpu, ram_total, ram_free, gpu_name, vram_total, gpu_3d }
        }
    }
}

#[cfg(target_os = "linux")]
mod linux {
    use super::Sample;
    use std::collections::HashMap;
    use std::time::Instant;

    /// `/proc` times are in clock ticks: `sysconf(_SC_CLK_TCK)`, 100 on every Linux the game runs on
    /// ([inf]; no libc call for it).
    const TICKS_PER_S: f64 = 100.0;

    /// utime + stime (clock ticks) of a `/proc/.../stat` line: fields 14 and 15, counted after the
    /// `(comm)` (which may hold spaces).
    fn stat_ticks(stat: &str) -> Option<u64> {
        let rest = &stat[stat.rfind(')')? + 1..];
        let f: Vec<&str> = rest.split_whitespace().collect();
        Some(f.get(11)?.parse::<u64>().ok()? + f.get(12)?.parse::<u64>().ok()?)
    }

    /// A `name:  N kB` line of `/proc/.../status` or `/proc/meminfo`, in bytes.
    fn kb(text: &str, name: &str) -> u64 {
        text.lines().find_map(|l| l.strip_prefix(name)?.strip_prefix(':')?.split_whitespace().next()?.parse::<u64>().ok()).unwrap_or(0) * 1024
    }

    /// The whole machine's (idle, all) clock ticks (`/proc/stat`'s `cpu` line; idle + iowait idle).
    fn system_times() -> (u64, u64) {
        let s = std::fs::read_to_string("/proc/stat").unwrap_or_default();
        let v: Vec<u64> = s.lines().next().unwrap_or("").split_whitespace().skip(1).filter_map(|x| x.parse().ok()).collect();
        let idle = v.get(3).copied().unwrap_or(0) + v.get(4).copied().unwrap_or(0);
        (idle, v.iter().take(8).sum())
    }

    /// The first display card under `/sys/class/drm` (`cardN`): vendor and device id as its name,
    /// and the memory amdgpu reports (`mem_info_vram_total`; 0 elsewhere).
    fn gpu() -> (String, u64) {
        let Ok(rd) = std::fs::read_dir("/sys/class/drm") else { return Default::default() };
        let mut cards: Vec<std::path::PathBuf> = rd.flatten().map(|e| e.path()).filter(|p| p.file_name().is_some_and(|n| n.to_string_lossy().strip_prefix("card").is_some_and(|k| k.bytes().all(|b| b.is_ascii_digit())))).collect();
        cards.sort();
        for c in cards {
            let read = |f: &str| std::fs::read_to_string(c.join("device").join(f)).map(|s| s.trim().to_string()).unwrap_or_default();
            let (vendor, device) = (read("vendor"), read("device"));
            if vendor.is_empty() {
                continue;
            }
            let maker = match vendor.as_str() {
                "0x1002" => "AMD".to_string(),
                "0x10de" => "NVIDIA".to_string(),
                "0x8086" => "Intel".to_string(),
                v => format!("vendor {v}"),
            };
            return (format!("{maker} {device}"), read("mem_info_vram_total").parse().unwrap_or(0));
        }
        Default::default()
    }

    pub struct Probe {
        at: Instant,
        process: u64,
        threads: HashMap<String, u64>,
        system: (u64, u64),
        cores: u32,
        gpu: (String, u64),
    }

    impl Probe {
        pub fn new() -> Probe {
            let mut p = Probe { at: Instant::now(), process: 0, threads: HashMap::new(), system: system_times(), cores: std::thread::available_parallelism().map_or(1, |n| n.get() as u32), gpu: gpu() };
            p.process = std::fs::read_to_string("/proc/self/stat").ok().as_deref().and_then(stat_ticks).unwrap_or(0);
            p.threads = Self::thread_ticks();
            p
        }

        /// Each thread's `name (tid)` and clock ticks.
        fn thread_ticks() -> HashMap<String, u64> {
            let Ok(rd) = std::fs::read_dir("/proc/self/task") else { return HashMap::new() };
            rd.flatten()
                .filter_map(|e| {
                    let tid = e.file_name().to_string_lossy().into_owned();
                    let ticks = stat_ticks(&std::fs::read_to_string(e.path().join("stat")).ok()?)?;
                    let name = std::fs::read_to_string(e.path().join("comm")).map(|s| s.trim().to_string()).unwrap_or_default();
                    Some((if name.is_empty() { format!("#{tid}") } else { format!("{name} ({tid})") }, ticks))
                })
                .collect()
        }

        pub fn sample(&mut self) -> Sample {
            let now = Instant::now();
            let wall = now.duration_since(self.at).as_secs_f64().max(1e-3) * TICKS_PER_S;
            self.at = now;
            let p = std::fs::read_to_string("/proc/self/stat").ok().as_deref().and_then(stat_ticks).unwrap_or(self.process);
            let cpu = (p.saturating_sub(self.process) as f64 / wall * 100.0) as f32;
            self.process = p;
            let t = Self::thread_ticks();
            let mut threads: Vec<(String, f32)> = t
                .iter()
                .filter_map(|(name, &v)| {
                    let d = v.saturating_sub(self.threads.get(name).copied().unwrap_or(v));
                    (d > 0).then(|| (name.clone(), (d as f64 / wall * 100.0) as f32))
                })
                .collect();
            threads.sort_by(|a, b| b.1.total_cmp(&a.1));
            self.threads = t;
            let status = std::fs::read_to_string("/proc/self/status").unwrap_or_default();
            let (idle, all) = system_times();
            let (d_idle, d_all) = (idle.saturating_sub(self.system.0), all.saturating_sub(self.system.1));
            self.system = (idle, all);
            let machine_cpu = if d_all == 0 { 0.0 } else { (d_all.saturating_sub(d_idle) as f64 / d_all as f64 * 100.0) as f32 };
            let mem = std::fs::read_to_string("/proc/meminfo").unwrap_or_default();
            Sample {
                working_set: kb(&status, "VmRSS"),
                private: kb(&status, "RssAnon"),
                gpu: None,
                cpu,
                threads,
                cores: self.cores,
                machine_cpu,
                ram_total: kb(&mem, "MemTotal"),
                ram_free: kb(&mem, "MemAvailable"),
                gpu_name: self.gpu.0.clone(),
                vram_total: self.gpu.1,
                gpu_3d: None,
            }
        }
    }

    #[cfg(test)]
    mod tests {
        #[test]
        fn proc_fields() {
            assert_eq!(super::stat_ticks("123 (a b) c) R 1 2 3 4 5 6 7 8 9 10 11 12 13"), Some(11 + 12));
            assert_eq!(super::kb("VmRSS:\t  2048 kB\nRssAnon: 10 kB\n", "RssAnon"), 10 * 1024);
            let s = super::Probe::new().sample();
            assert!(s.working_set > 0 && s.ram_total > 0 && s.cores > 0, "{s:?}");
        }
    }
}

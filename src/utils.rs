use std::arch::asm;

use log::*;
use nix::{time::{clock_gettime, ClockId}, sched::{CpuSet, sched_setaffinity}, sys::mman, unistd::Pid};

pub const NANOS_IN_SEC: i64 = 1_000_000_000;
pub static mut TSC_FREQUENCY: f64 = 0f64;
pub static mut TIME_OFFSET: i64 = 0i64;


pub type TimeFunc = fn() -> i64;


#[derive(Debug)]
pub struct ProgramArgs {
    pub duration_seconds: i64,
    pub report_interval_millis: i64,
    pub cpus: Vec<u32>,
    pub time_func: TimeFunc,
    pub mlock_enabled: bool,
    pub lapic_enabled: bool,
    pub influx_url: String,
    pub influx_db: String,
    pub local_hostname: String,
}

impl Default for ProgramArgs {
    fn default() -> ProgramArgs {
        ProgramArgs {
            duration_seconds: 0,
            report_interval_millis: 0,
            cpus: Vec::default(),
            time_func: clock_realtime,
            mlock_enabled: false,
            lapic_enabled: true,
            influx_url: String::default(),
            influx_db: String::default(),
            local_hostname: String::default(),
        }
    }
}

pub fn clock_realtime() -> i64 {
    let time_spec = clock_gettime(ClockId::CLOCK_REALTIME).unwrap();
    return time_spec.tv_sec() * NANOS_IN_SEC + time_spec.tv_nsec();
}


pub fn clock_monotonic() -> i64 {
    let time_spec = clock_gettime(ClockId::CLOCK_MONOTONIC).unwrap();
    unsafe {
        return time_spec.tv_sec() * NANOS_IN_SEC + time_spec.tv_nsec() + TIME_OFFSET;
    }
}


pub fn clock_rdtsc() -> i64 {
    unsafe {
        (rdtsc() as f64 / TSC_FREQUENCY) as i64 + TIME_OFFSET
    }
}


//noinspection ALL
#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
pub fn rdtsc() -> i64 {
    let upper: i64;
    let lower: i64;

    unsafe {
        asm!(
        "rdtsc",
        out("rax") lower,
        out("rdx") upper,
        // options(pure, readonly, nostack)
        )
    }

    upper << 32 | lower
}


pub fn affinitize_to_cpu(cpu: u32) {
    let mut cpus = CpuSet::new();
    cpus.set(cpu as usize).expect("Unable to set target CPU in cpuset");
    sched_setaffinity(Pid::from_raw(0), &cpus).expect(&format!("Unable to set CPU affinity to cpu: {}", cpu));
}


pub fn mlock() {
    info!("Mlocking pages to RAM");
    let result = mman::mlockall(mman::MlockAllFlags::MCL_CURRENT | mman::MlockAllFlags::MCL_FUTURE);
    if result.is_err() {
        panic!("Unable to mlock program pages: {}", result.unwrap_err());
    }
}


#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
pub fn disable_lapic() {
    unsafe { asm!("cli", options(nomem)) }
}


#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
pub fn enable_lapic() {
    unsafe { asm!("sti", options(nomem)) }
}


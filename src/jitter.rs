use log::{info, warn};

use crate::{utils::{ProgramArgs, NANOS_IN_SEC, disable_lapic, enable_lapic}, influx::publish_results};


#[derive(Debug, Clone, Copy)]
pub struct Jitter {
    pub ts: i64,
    pub latency: i64,
}


pub fn capture_jitter(cpu: u32, program_args: &ProgramArgs) {
    info!("Affinitizing jitter sampler thread to cpu: {}", cpu);
    crate::utils::affinitize_to_cpu(cpu);

    if !program_args.lapic_enabled {
        warn!("Disabling local APIC interrupts on cpu: {}. This may result in the whole machine becoming unresponsive", cpu);
        disable_lapic();
    }
    
    let sample_count = (program_args.duration_seconds * 1000 / program_args.report_interval_millis) as usize;
    let mut results: Vec<Jitter> = vec![Jitter { ts: 0, latency: 0 }; sample_count];
    busy_loop(program_args, &mut results);
    
    if !program_args.lapic_enabled {
        info!("Re-enabling local APIC interrupts on cpu: {}", cpu);
        enable_lapic();
    }

    publish_results(program_args, cpu, results);
}


fn busy_loop(program_args: &ProgramArgs, jitter: &mut Vec<Jitter>) {
    let mut previous = (program_args.time_func)();
    let deadline = previous + program_args.duration_seconds * NANOS_IN_SEC;
    let mut next_report = previous + program_args.report_interval_millis * 1_000_000;

    let mut max = i64::MIN;
    let mut idx = 0;

    while previous < deadline {
        let mut now = (program_args.time_func)();
        let latency = now - previous;
        if latency > max {
            max = latency
        }

        if now > next_report {
            next_report = now + program_args.report_interval_millis * 1_000_000;
            jitter[idx].ts = now;
            jitter[idx].latency = max;
            max = i64::MIN;
            idx += 1;
            now = (program_args.time_func)();
        }

        previous = now;
    }
}
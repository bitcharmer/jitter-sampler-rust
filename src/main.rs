use clap::{App, AppSettings, Arg, ArgMatches};
use nix::sched::*;
use nix::time::*;
use std::arch::asm;
use std::fmt::Debug;
use std::net::UdpSocket;
use std::str::FromStr;

const NANOS_IN_SEC: i64 = 1_000_000_000;

static mut TSC_FREQUENCY: f64 = 0f64;
static mut TSC_OFFSET: i64 = 0;

type TimeFunc = fn() -> i64;

#[derive(Debug, Clone, Copy)]
struct Jitter {
    ts: i64,
    latency: i64,
}

#[derive(Debug)]
struct ProgramArgs {
    duration_seconds: i64,
    report_interval_millis: i64,
    cpu: i64,
    time_func: TimeFunc,
}

impl Default for ProgramArgs {
    fn default() -> ProgramArgs {
        ProgramArgs {
            duration_seconds: 0,
            report_interval_millis: 0,
            cpu: -1,
            time_func: clock_realtime,
        }
    }
}

fn main() {
    let program_args = parse_program_args();
    println!("{:?}", program_args);
    affinitize_to_cpu(program_args.cpu);

    let sample_count =
        (program_args.duration_seconds * 1000 / program_args.report_interval_millis) as usize;
    let mut jitter: Vec<Jitter> = vec![Jitter { ts: 0, latency: 0 }; sample_count];

    capture_jitter(&program_args, &mut jitter);
    publish_results(&jitter)
}

fn capture_jitter(program_args: &ProgramArgs, jitter: &mut Vec<Jitter>) {
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
            jitter[idx] = Jitter {
                ts: now,
                latency: max,
            };
            max = i64::MIN;
            idx += 1;
            now = (program_args.time_func)();
        }

        previous = now;
    }
}

fn publish_results(jitter: &Vec<Jitter>) {
    let socket = UdpSocket::bind("0.0.0.0:0").expect("Unable to bind to influx udp socket");
    for data in jitter {
        if data.ts == 0 {
            continue;
        }

        let str: String = format!("jitter latency={} {}", data.latency, data.ts);
        println!("{}", str);
        socket
            .send_to(str.as_bytes(), "127.0.0.1:8089")
            .expect("Error while sending datagram");
    }
}

fn clock_realtime() -> i64 {
    let time_spec = clock_gettime(ClockId::CLOCK_REALTIME).unwrap();
    return time_spec.tv_sec() * NANOS_IN_SEC + time_spec.tv_nsec();
}

fn rdtsc_realtime() -> i64 {
    unsafe { (rdtsc() as f64 / TSC_FREQUENCY) as i64 - TSC_OFFSET }
}

//noinspection ALL
#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
fn rdtsc() -> i64 {
    let upper: i64;
    let lower: i64;

    unsafe {
        asm!(
        "rdtsc",
        out("rax") lower,
        out("rdx") upper,
        options(pure, readonly, nostack)
        )
    }

    upper << 32 | lower
}

fn calibrate_tsc_offset(tsc_frequency: f64) -> i64 {
    let mut min_diff = i64::MAX;

    for _n in 1..1_000_000 {
        let diff = (rdtsc() as f64 / tsc_frequency) as i64 - clock_realtime();
        if diff < min_diff {
            min_diff = diff;
        }
    }

    return min_diff;
}

fn affinitize_to_cpu(cpu: i64) {
    let pid = nix::unistd::Pid::this();
    let mut cpus = CpuSet::new();
    cpus.set(cpu as usize)
        .expect("Unable to set target CPU in cpuset");
    sched_setaffinity(pid, &cpus).expect(&format!("Unable to set CPU affinity to cpu: {}", cpu));
}

fn parse_program_args() -> ProgramArgs {
    let matches = App::new("Platform jitter sampler")
        .term_width(250)
        .version("1.0")
        .author("Wojciech Kudla")
        .about("Captures instruction execution stalls and reports as time series")
        .arg(
            Arg::new("duration_seconds")
                .short('d')
                .long("duration")
                .value_name("seconds")
                .about("How long to keep running for")
                .takes_value(true)
                .required(true),
        )
        .arg(
            Arg::new("report_interval_millis")
                .short('r')
                .long("report-interval")
                .value_name("milliseconds")
                .about("How frequently to retain worse observed latency")
                .takes_value(true)
                .required(true),
        )
        .arg(
            Arg::new("cpu")
                .short('c')
                .long("cpu")
                .value_name("target cpu")
                .about("CPU to affinitise the program thread to")
                .takes_value(true)
                .required(true),
        )
        .arg(
            Arg::new("tsc_frequency")
                .short('t')
                .long("tsc-frequency")
                .value_name("GHz")
                .about("Frequency of TSC as a decimal number. When passed this uses offset and scaled rdtsc instead of clock_gettime for capturing stall durations")
                .takes_value(true)
                .required(false),
        )
        .setting(AppSettings::ArgRequiredElseHelp)
        .setting(AppSettings::ColoredHelp)
        .get_matches();

    let time_func: TimeFunc = if !matches.is_present("tsc_frequency") {
        clock_realtime
    } else {
        let tsc_frequency: f64 =
            parse_program_arg(&matches, "tsc_frequency").expect("Incorrect value for frequency");
        unsafe {
            TSC_FREQUENCY = tsc_frequency;
            TSC_OFFSET = calibrate_tsc_offset(tsc_frequency);
            rdtsc_realtime
        }
    };

    let program_args = ProgramArgs {
        duration_seconds: parse_program_arg(&matches, "duration_seconds")
            .expect("Unable to parse duration argument"),
        report_interval_millis: parse_program_arg(&matches, "report_interval_millis")
            .expect("Incorrect value for reporting interval"),
        cpu: parse_program_arg(&matches, "cpu").expect("Incorrect value for cpu"),
        time_func,
    };

    return program_args;
}

fn parse_program_arg<T: FromStr>(matches: &ArgMatches, arg_name: &str) -> Result<T, String> {
    if let Some(s) = matches.value_of(arg_name) {
        T::from_str(s).or(Err(format!(
            "Unable to parse argument {} with value {}",
            arg_name, s
        )))
    } else {
        Err(format!("Option {} not present", arg_name))
    }
}

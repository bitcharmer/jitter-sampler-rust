mod utils;
mod jitter;
mod influx;

use std::iter::FromIterator;

use env_logger::Env;
use log::{info, error};
use nix::libc;
use utils::*;
use jitter::*;
use clap::{Arg, ArgMatches, Command, ArgAction};


fn main() {
    env_logger::Builder::from_env(Env::default().default_filter_or("info")).init();

    let program_args = parse_program_args();
    info!("Running with args:\n{:#?}", program_args);

    if program_args.mlock_enabled {
        mlock()
    }

    if program_args.lapic_disabled {
        unsafe { 
            if libc::iopl(3) != 0 {
                error!("Error while changing privilege level of the process with iopl(). Unable to turn off LAPIC.");
                std::process::exit(1);
            }
        }
    }

    let cpus = program_args.cpus.clone();
    let args = &program_args;

    crossbeam::scope(|s| {
        for cpu in cpus {
            s.spawn(move |_| { capture_jitter(cpu, args); });
        }
    }).unwrap();
}


pub fn parse_program_args() -> ProgramArgs {
    let matches = match_arguments();
    
    let program_args = ProgramArgs {
        duration_seconds: *matches.get_one::<i64>("duration_seconds").expect("Unable to parse duration argument"),
        report_interval_millis: *matches.get_one::<i64>("report_interval_millis").expect("Incorrect value for reporting interval"),
        cpus: parse_cpu_list(matches.get_one::<String>("cpus").expect("Unable to extract cpu list from arg: cpus")),
        time_func: configure_clock(&matches),
        mlock_enabled: *matches.get_one::<bool>("mlock").unwrap(),
        lapic_disabled: *matches.get_one::<bool>("lapic").unwrap(),
        influx_url: matches.get_one::<String>("influx_url").expect("Unable to extract InfluxDB url from program args").clone(),
        influx_db: matches.get_one::<String>("influx_db").expect("Unable to extract Influx database name from program args").clone(),
        local_hostname: gethostname::gethostname().into_string().expect("Unable to obtain local hostname"),
    };

    program_args
}


fn configure_clock(matches: &ArgMatches) -> fn() -> i64 {
    if matches.contains_id("tsc_frequency") {
        unsafe {
            utils::TSC_FREQUENCY = *matches.get_one::<f64>("tsc_frequency").expect("Unable to parse TSC frequency");
        }
    }
    
    let time_func: TimeFunc = match matches.get_one::<String>("time_source").map(|s| { s.as_str() }) {
        Some(clock_type) => match clock_type {
            "clock_realtime" => clock_realtime,
            "clock_monotonic" => clock_monotonic,
            "rdtsc" => clock_rdtsc,
            _ => panic!("Unrecognized clock type")
        },
        None => clock_realtime
    };
    
    if time_func != clock_realtime {
        unsafe {
            TIME_OFFSET = clock_realtime() - time_func();
        }
    }

    time_func
}


fn match_arguments() -> ArgMatches {
    let matches = Command::new("Platform jitter sampler")
        .term_width(250)
        .version("1.0.1")
        .author("Wojciech Kudla")
        .about("Runs for <duration> seconds on select <cpus> and for each <report-interval> stores worst instruction execution latency along with its associated timestamp. At the end of program execution it publishes all data points to InfluxDB")
        .arg(
            Arg::new("duration_seconds")
                .short('d')
                .long("duration")
                .value_name("seconds")
                .help("How long to keep running for")
                .default_value("10")
                .value_parser(clap::value_parser!(i64))
        )
        .arg(
            Arg::new("report_interval_millis")
                .short('r')
                .long("report-interval")
                .value_name("milliseconds")
                .help("Sampling interval")
                .default_value("100")
                .value_parser(clap::value_parser!(i64))
        )
        .arg(
            Arg::new("mlock")
                .short('m')
                .long("mlock")
                .help("Mlock jitter data pages to RAM")
                .required(false)
                .action(ArgAction::SetTrue)
                .default_value("false")
        )
        .arg(
            Arg::new("lapic")
                .short('l')
                .long("lapic")
                .help("Disable local APIC interrupts (requires superuser privileges).")
                .required(false)
                .action(ArgAction::SetTrue)
                .default_value("false")
        )
        .arg(
            Arg::new("cpus")
                .short('c')
                .long("cpus")
                .value_name("target cpus")
                .help("CPU to affinitise the program thread(s) to; can be passed as list of ranges, eg: '1,4-6,8-12,15'")
                .default_value("0")
        )
        .arg(
            Arg::new("tsc_frequency")
                .short('f')
                .long("tsc-frequency")
                .value_name("GHz")
                .help("Frequency of TSC as a decimal number")
                .value_parser(clap::value_parser!(f64))
        )
        .arg(
            Arg::new("time_source")
                .short('t')
                .long("time-source")
                .help("Implementation to use for measuring elapsed time: clock_realtime | clock_monotonic | rdtsc")
                .default_value("clock_realtime")
        )
        .arg(
            Arg::new("influx_url")
                .short('i')
                .long("influx-url")
                .value_name("URL")
                .help("Influx database url (eg: http://foo.bar.com:8086)")
                .required(true),
        )
        .arg(
            Arg::new("influx_db")
                .short('b')
                .long("influx-db")
                .help("Influx database name")
                .required(true),
        )
        .get_matches();
    
    matches
}


fn parse_cpu_list(cpu_list_str: &str) -> Vec<u32> {
    let mut result: Vec<u32> = Vec::default();
    let elements = cpu_list_str.trim().split(',');
    for element in elements {
        if element.contains('-') {
            let range = Vec::from_iter(element.split('-'));
            let begin = range[0].parse::<u32>().expect(format!("Unable to parse cpu: {}", range[0]).as_str());
            let end = range[1].parse::<u32>().expect(format!("Unable to parse cpu: {}", range[0]).as_str());
            for cpu in begin..end + 1 {
                result.push(cpu);
            }
        } else {
            result.push(element.parse::<u32>().expect(format!("Unable to parse cpu: {}", element).as_str()));
        }
    }
    
    result
}

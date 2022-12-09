mod utils;
mod jitter;
mod influx;

use std::{str::FromStr, iter::FromIterator};

use log::info;
use utils::*;
use jitter::*;
use clap::{App, Arg, AppSettings, ArgMatches};

use crate::influx::publish_results;



fn main() {
    env_logger::init();

    let program_args = parse_program_args();
    info!("Running with args:\n{:#?}", program_args);

    if program_args.mlock_enabled {
        mlock()
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
            Arg::new("mlock")
                .short('m')
                .long("mlock")
                .about("Enable mlocking jitter sampler pages to RAM")
                .takes_value(false)
                .required(false),
        )
        .arg(
            Arg::new("lapic")
                .short('l')
                .long("lapic")
                .about("Disable local APIC interrupts")
                .takes_value(false)
                .required(false),
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
            Arg::new("cpus")
                .short('c')
                .long("cpus")
                .value_name("target cpus")
                .about("CPU to affinitise the program thread to (can be passed as list of ranges, eg:")
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
        .arg(
            Arg::new("influx_url")
                .short('i')
                .long("influx-url")
                .value_name("URL")
                .about("Influx DB url (eg: http://foo.bar.com:8086)")
                .takes_value(true)
                .required(true),
        )
        .arg(
            Arg::new("influx_db")
                .short('b')
                .long("influx-db")
                .about("Influx database name")
                .takes_value(true)
                .required(true),
        )
        .setting(AppSettings::ArgRequiredElseHelp)
        .setting(AppSettings::ColoredHelp)
        .get_matches();

    let time_func: TimeFunc = if !matches.is_present("tsc_frequency") {
        clock_realtime
    } else {
        clock_realtime
        // let tsc_frequency: f64 = 0;
        //     parse_program_arg(&matches, "tsc_frequency").expect("Incorrect value for frequency");
        // unsafe {
        //     TSC_FREQUENCY = tsc_frequency;
        //     TSC_OFFSET = calibrate_tsc_offset(tsc_frequency);
        //     rdtsc_realtime
        // }
    };
   
    let cpu_list_str = matches.value_of("cpus").expect("Unable to extract cpu list from arg: cpus");

    let program_args = ProgramArgs {
        duration_seconds: parse_program_arg(&matches, "duration_seconds")
            .expect("Unable to parse duration argument"),
        report_interval_millis: parse_program_arg(&matches, "report_interval_millis")
            .expect("Incorrect value for reporting interval"),
        cpus: parse_cpu_list(cpu_list_str),
        time_func,
        mlock_enabled: matches.is_present("mlock"),
        lapic_enabled: !matches.is_present("lapic"),
        influx_url: String::from(matches.value_of("influx_url").expect("Unable to extract InfluxDB url from program args")),
        influx_db: String::from(matches.value_of("influx_db").expect("Unable to extract Influx database name from program args")),
        local_hostname: gethostname::gethostname().into_string().expect("Unable to obtain local hostname"),
    };

    return program_args;
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
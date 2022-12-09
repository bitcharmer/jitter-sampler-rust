use crate::{jitter::Jitter, utils::ProgramArgs};

const BATCH_PUBLISH_THRESHOLD_BYTES: usize = 768 * 1024;

pub fn publish_results(program_args: &ProgramArgs, cpu: u32, results: Vec<Jitter>) {
    let mut body: String = String::default();

    for data_point in results {
        body.push_str(format!("jitter,host={},cpu={} jitter={} {}\n", program_args.local_hostname, cpu, data_point.latency, data_point.ts).as_str());
        if body.len() >= BATCH_PUBLISH_THRESHOLD_BYTES {
            post_batch(&program_args, &body);
            body.clear();
        }
    }

    post_batch(&program_args, &body);
}

pub fn post_batch(program_args: &ProgramArgs, batch: &String) {
    let url = format!("{}/write?db={}", program_args.influx_url, program_args.influx_db);
    isahc::post(url, batch.as_str());
}
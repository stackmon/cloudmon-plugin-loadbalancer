use regex::Regex;
use serde::Deserialize;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::sleep;
use std::time::{Duration, Instant};

use statsd::Client;

use env_logger::Env;

// Use Jemalloc only for musl-64 bits platforms
#[cfg(all(target_env = "musl", target_pointer_width = "64"))]
#[global_allocator]
static ALLOC: jemallocator::Jemalloc = jemallocator::Jemalloc;

#[derive(Debug, Deserialize)]
struct Config {
    #[serde(default = "default_connect_timeout")]
    timeout: u8,
    #[serde(default = "default_check_interval")]
    interval: u8,
    loadbalancers: Vec<Loadbalancer>,
    statsd: Statsd,
}

#[derive(Debug, Deserialize)]
struct Loadbalancer {
    address: String,
    name: String,
    listeners: Vec<Listener>,
}

#[derive(Debug, Deserialize)]
struct Listener {
    port: u16,
    mode: String,
}

#[derive(Debug, Deserialize)]
struct Statsd {
    server: String,
    prefix: String,
}

fn default_connect_timeout() -> u8 {
    2
}
fn default_check_interval() -> u8 {
    5
}

fn main() {
    let env = Env::default()
        .filter_or("CLOUDMON_LOG_LEVEL", "debug")
        .write_style_or("CLOUDMON_LOG_STYLE", "always");

    env_logger::init_from_env(env);
    log::info!("Starting cloudmon-plugin-loadbalancer");

    let f = std::fs::File::open("config.yaml").expect("Could not open file.");
    let config: Config = serde_yaml::from_reader(f).expect("Could not read values.");

    let term = Arc::new(AtomicBool::new(false));
    signal_hook::flag::register(signal_hook::consts::SIGINT, Arc::clone(&term)).unwrap();
    signal_hook::flag::register(signal_hook::consts::SIGTERM, Arc::clone(&term)).unwrap();

    let statsd_server = config.statsd.server.clone();
    let statsd_prefix = config.statsd.prefix.clone();
    let statsd_client: Client = Client::new(statsd_server, &statsd_prefix).unwrap();
    let re = Regex::new(r"^instance-\d+-(.*)$").unwrap();
    let interval = Duration::from_secs(config.interval as u64);
    /* Client builder with disabled TLS verification, regular timeout and pool_idle_timeout to
     * avoid pooling */
    let req_client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(config.timeout as u64))
        .danger_accept_invalid_certs(true)
        .pool_idle_timeout(Duration::from_secs(1))
        .build()
        .unwrap();
    while !term.load(Ordering::Relaxed) {
        for lb in config.loadbalancers.iter() {
            log::debug!("Checking Loadbalancer {}", lb.name);
            for lsnr in lb.listeners.iter() {
                log::debug!("Checking port {}", lsnr.port);
                let start = Instant::now();
                match req_client
                    .get(format!(
                        "http{}://{}:{}",
                        if lsnr.mode == "https" { "s" } else { "" },
                        lb.address,
                        lsnr.port
                    ))
                    .send()
                {
                    Ok(rsp) => match rsp.headers().get("Backend-Server") {
                        Some(srv) => match re.captures(&srv.to_str().unwrap()) {
                            Some(m) => {
                                log::debug!("res = {}", &m[1]);
                                let duration = start.elapsed();
                                statsd_client
                                    .timer(
                                        &format!("loadbalancer.{}.{}.{}", lb.name, lsnr.mode, &m[1]),
                                        duration.as_millis() as f64,
                                );
                            },
                            None => log::error!("Cannot detect response AZ"),
                        },
                        None => log::error!("Cannot detect response AZ"),
                    },
                    Err(e) => {
                        log::error!("{}", e);
                        statsd_client
                            .incr(&format!("loadbalancer.{}.{}.failed", lb.name, lsnr.mode));
                    }
                }
                /* Increase attempted at the end to avoid timeout data shift */
                statsd_client
                    .incr(&format!("loadbalancer.{}.{}.attempted", lb.name, lsnr.mode));
            }
        }
        sleep(interval);
    }

    log::info!("Stopped cloudmon-plugin-loadbalancer");
}

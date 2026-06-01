use std::env;
use std::num::NonZeroUsize;
use std::thread;

pub const MAX_WORKERS_ENV: &str = "VEGVISIR_MAX_WORKERS";
pub const RESERVED_CORES_ENV: &str = "VEGVISIR_RESERVED_CORES";

const DEFAULT_RESERVED_CORES: usize = 1;
const ABSOLUTE_MAX_WORKERS: usize = 256;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParallelismConfig {
    pub available_parallelism: usize,
    pub reserved_cores: usize,
    pub max_workers: usize,
    pub source: ParallelismSource,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ParallelismSource {
    Auto,
    Env { variable: String, value: String },
    InvalidEnv { variable: String, value: String },
}

impl ParallelismConfig {
    pub fn detect() -> Self {
        let available = available_parallelism();
        let reserved = env_usize(RESERVED_CORES_ENV)
            .unwrap_or(DEFAULT_RESERVED_CORES)
            .min(available.saturating_sub(1));

        match env::var(MAX_WORKERS_ENV) {
            Ok(raw) => match parse_worker_count(&raw, available, reserved) {
                Some(max_workers) => Self {
                    available_parallelism: available,
                    reserved_cores: reserved,
                    max_workers,
                    source: ParallelismSource::Env {
                        variable: MAX_WORKERS_ENV.to_string(),
                        value: raw,
                    },
                },
                None => Self {
                    available_parallelism: available,
                    reserved_cores: reserved,
                    max_workers: auto_worker_count(available, reserved),
                    source: ParallelismSource::InvalidEnv {
                        variable: MAX_WORKERS_ENV.to_string(),
                        value: raw,
                    },
                },
            },
            Err(_) => Self {
                available_parallelism: available,
                reserved_cores: reserved,
                max_workers: auto_worker_count(available, reserved),
                source: ParallelismSource::Auto,
            },
        }
    }

    pub fn constrained_workers(&self, requested: usize) -> usize {
        requested.clamp(1, self.max_workers.max(1))
    }

    pub fn source_label(&self) -> String {
        match &self.source {
            ParallelismSource::Auto => "auto".to_string(),
            ParallelismSource::Env { variable, value } => format!("{variable}={value}"),
            ParallelismSource::InvalidEnv { variable, value } => {
                format!("auto; ignored invalid {variable}={value}")
            }
        }
    }
}

impl Default for ParallelismConfig {
    fn default() -> Self {
        Self::detect()
    }
}

pub fn available_parallelism() -> usize {
    thread::available_parallelism()
        .map(NonZeroUsize::get)
        .unwrap_or(1)
        .max(1)
}

pub fn default_worker_count() -> usize {
    ParallelismConfig::detect().max_workers
}

pub fn run_parallel_ordered<T, R, F>(items: Vec<T>, max_workers: usize, worker: F) -> Vec<R>
where
    T: Send + 'static,
    R: Send + 'static,
    F: Fn(T) -> R + Send + Sync + 'static,
{
    let len = items.len();
    if len == 0 {
        return Vec::new();
    }
    let workers = max_workers.clamp(1, len);
    if workers == 1 || len == 1 {
        return items.into_iter().map(worker).collect();
    }

    let worker = std::sync::Arc::new(worker);
    let (job_tx, job_rx) = std::sync::mpsc::channel::<(usize, T)>();
    let job_rx = std::sync::Arc::new(std::sync::Mutex::new(job_rx));
    let (result_tx, result_rx) = std::sync::mpsc::channel::<(usize, R)>();

    let mut handles = Vec::with_capacity(workers);
    for _ in 0..workers {
        let job_rx = std::sync::Arc::clone(&job_rx);
        let result_tx = result_tx.clone();
        let worker = std::sync::Arc::clone(&worker);
        handles.push(thread::spawn(move || {
            loop {
                let next = match job_rx.lock() {
                    Ok(rx) => rx.recv(),
                    Err(_) => return,
                };
                let Ok((index, item)) = next else {
                    return;
                };
                let result = worker(item);
                if result_tx.send((index, result)).is_err() {
                    return;
                }
            }
        }));
    }
    drop(result_tx);

    for (index, item) in items.into_iter().enumerate() {
        if job_tx.send((index, item)).is_err() {
            break;
        }
    }
    drop(job_tx);

    let mut results: Vec<Option<R>> = (0..len).map(|_| None).collect();
    for _ in 0..len {
        if let Ok((index, result)) = result_rx.recv() {
            results[index] = Some(result);
        }
    }

    for handle in handles {
        let _ = handle.join();
    }

    results
        .into_iter()
        .map(|result| result.expect("parallel worker produced one result per input"))
        .collect()
}

fn env_usize(name: &str) -> Option<usize> {
    env::var(name)
        .ok()
        .and_then(|value| value.trim().parse::<usize>().ok())
}

fn parse_worker_count(raw: &str, available: usize, reserved: usize) -> Option<usize> {
    let value = raw.trim();
    if value.eq_ignore_ascii_case("auto") {
        return Some(auto_worker_count(available, reserved));
    }
    value
        .parse::<usize>()
        .ok()
        .filter(|count| *count > 0)
        .map(|count| count.min(ABSOLUTE_MAX_WORKERS))
}

fn auto_worker_count(available: usize, reserved: usize) -> usize {
    available
        .saturating_sub(reserved)
        .max(1)
        .min(ABSOLUTE_MAX_WORKERS)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn auto_worker_count_keeps_at_least_one_worker() {
        assert_eq!(auto_worker_count(1, 1), 1);
        assert_eq!(auto_worker_count(2, 1), 1);
        assert_eq!(auto_worker_count(8, 1), 7);
    }

    #[test]
    fn parse_worker_count_accepts_auto_and_positive_numbers() {
        assert_eq!(parse_worker_count("auto", 8, 1), Some(7));
        assert_eq!(parse_worker_count("4", 8, 1), Some(4));
        assert_eq!(parse_worker_count("0", 8, 1), None);
        assert_eq!(parse_worker_count("nope", 8, 1), None);
    }

    #[test]
    fn constrained_workers_clamps_to_detected_limit() {
        let config = ParallelismConfig {
            available_parallelism: 16,
            reserved_cores: 1,
            max_workers: 6,
            source: ParallelismSource::Auto,
        };
        assert_eq!(config.constrained_workers(0), 1);
        assert_eq!(config.constrained_workers(3), 3);
        assert_eq!(config.constrained_workers(99), 6);
    }

    #[test]
    fn run_parallel_ordered_preserves_input_order() {
        let values = vec![1, 2, 3, 4, 5, 6];
        let doubled = run_parallel_ordered(values, 3, |value| value * 2);
        assert_eq!(doubled, vec![2, 4, 6, 8, 10, 12]);
    }
}

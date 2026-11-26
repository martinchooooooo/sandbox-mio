//! The one-minute rollup of a sandbox's live samples, as written to its history file. Pure.

use mio_domain::types::ProviderId;

/// The share of the limit a sample counts as under memory pressure in the rollups. Equal to
/// `STRAINED_MEMORY` in `src/lib/sandbox/health.ts`.
const PRESSURE_FRACTION: f64 = 0.9;

/// One live reading of one sandbox.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResourceSample {
    pub at_ms: u64,
    /// Fraction (0 to 1) of the allotted CPUs used since the previous sample; `None` for the first
    /// sample after start, or while the allotted CPU count is unknown.
    pub cpu: Option<f64>,
    pub memory_used: u64,
    pub memory_limit: u64,
}

/// One minute of one sandbox, as written to its history file. Memory is in MiB and CPU is rounded
/// to [`CPU_DECIMALS`] places, which keeps a line near 110 bytes.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Rollup {
    /// Unix seconds at the end of the minute.
    pub ts: u64,
    pub provider: ProviderId,
    /// Mean CPU fraction; absent when no sample in the minute had a CPU reading.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cpu: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cpu_max: Option<f64>,
    /// Mean memory in use.
    pub mem: u64,
    pub mem_max: u64,
    pub limit: u64,
    /// Samples at or above [`PRESSURE_FRACTION`] of the limit.
    pub high: u32,
    /// Which memory definition the provider reported (Resolved Decision 5); written only when true.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub includes_cache: bool,
}

/// One minute of the Podman machine, as written to `_podman-machine.jsonl`, memory in MiB.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PoolRollup {
    pub ts: u64,
    pub cpus: u32,
    pub total: u64,
    /// The kernel's free memory, which counts page cache as used.
    pub free: u64,
}

const CPU_DECIMALS: i32 = 4;

fn round_cpu(fraction: f64) -> f64 {
    let scale = 10f64.powi(CPU_DECIMALS);
    (fraction * scale).round() / scale
}

pub(super) fn mib(bytes: u64) -> u64 {
    (bytes + (1 << 19)) >> 20
}

#[derive(Default)]
pub(super) struct MinuteAcc {
    cpu_sum: f64,
    cpu_n: u32,
    cpu_max: f64,
    mem_sum: u64,
    mem_max: u64,
    mem_limit: u64,
    n: u32,
    high: u32,
}

impl MinuteAcc {
    pub(super) fn add(&mut self, sample: &ResourceSample) {
        if let Some(cpu) = sample.cpu {
            self.cpu_sum += cpu;
            self.cpu_n += 1;
            self.cpu_max = self.cpu_max.max(cpu);
        }
        self.mem_sum += sample.memory_used;
        self.mem_max = self.mem_max.max(sample.memory_used);
        self.mem_limit = sample.memory_limit;
        self.n += 1;
        if sample.memory_limit > 0
            && sample.memory_used as f64 >= PRESSURE_FRACTION * sample.memory_limit as f64
        {
            self.high += 1;
        }
    }

    /// The minute's rollup, resetting the accumulator; `None` for a minute without samples.
    pub(super) fn take(&mut self, ts: u64, provider: ProviderId, includes_cache: bool) -> Option<Rollup> {
        let acc = std::mem::take(self);
        (acc.n > 0).then(|| Rollup {
            ts,
            provider,
            cpu: (acc.cpu_n > 0).then(|| round_cpu(acc.cpu_sum / acc.cpu_n as f64)),
            cpu_max: (acc.cpu_n > 0).then(|| round_cpu(acc.cpu_max)),
            mem: mib(acc.mem_sum / acc.n as u64),
            mem_max: mib(acc.mem_max),
            limit: mib(acc.mem_limit),
            high: acc.high,
            includes_cache,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MIB: u64 = 1 << 20;

    fn sample(cpu: Option<f64>, used_mib: u64) -> ResourceSample {
        ResourceSample { at_ms: 0, cpu, memory_used: used_mib * MIB, memory_limit: 100 * MIB }
    }

    #[test]
    fn a_minute_rolls_up_means_peaks_and_pressure() {
        let mut acc = MinuteAcc::default();
        acc.add(&sample(None, 50));
        acc.add(&sample(Some(0.2), 90));
        acc.add(&sample(Some(0.4), 95));
        let r = acc.take(60, ProviderId::Podman, false).unwrap();
        assert_eq!(r.high, 2);
        assert_eq!(r.mem, 78);
        assert_eq!(r.mem_max, 95);
        assert_eq!(r.limit, 100);
        assert_eq!(r.cpu, Some(0.3));
        assert_eq!(r.cpu_max, Some(0.4));
        assert!(acc.take(120, ProviderId::Podman, false).is_none(), "the accumulator resets");
    }

    #[test]
    fn rollups_round_trip_through_a_history_line() {
        let r = Rollup {
            ts: 1,
            provider: ProviderId::Podman,
            cpu: None,
            cpu_max: None,
            mem: 2,
            mem_max: 3,
            limit: 4,
            high: 0,
            includes_cache: false,
        };
        let line = serde_json::to_string(&r).unwrap();
        assert_eq!(line, r#"{"ts":1,"provider":"podman","mem":2,"memMax":3,"limit":4,"high":0}"#);
        assert_eq!(serde_json::from_str::<Rollup>(&line).unwrap(), r);
    }
}

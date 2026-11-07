#!/bin/bash
# Compare the checked-out converter with a Git revision using identical inputs.
set -euo pipefail
repo_root="$(cd "$(dirname "$0")/.." && pwd)"
baseline_revision="${1:-HEAD}"
benchmark_dir="$(mktemp -d "${TMPDIR:-/tmp}/whisper-audio-bench.XXXXXX")"
trap 'rm -rf "$benchmark_dir"' EXIT
cd "$repo_root"
git show "$baseline_revision:src/audio/wav.rs" > "$benchmark_dir/before.rs"
cp src/audio/wav.rs "$benchmark_dir/after.rs"
cat > "$benchmark_dir/main.rs" <<'RUST'
#[path = "before.rs"]
mod before;
#[path = "after.rs"]
mod after;

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering};

struct CountingAllocator;
static LIVE: AtomicUsize = AtomicUsize::new(0);
static PEAK: AtomicUsize = AtomicUsize::new(0);

fn allocated(size: usize) {
    let live = LIVE.fetch_add(size, Ordering::Relaxed) + size;
    PEAK.fetch_max(live, Ordering::Relaxed);
}

unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let ptr = unsafe { System.alloc(layout) };
        if !ptr.is_null() { allocated(layout.size()); }
        ptr
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        LIVE.fetch_sub(layout.size(), Ordering::Relaxed);
        unsafe { System.dealloc(ptr, layout); }
    }
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        let ptr = unsafe { System.realloc(ptr, layout, size) };
        if !ptr.is_null() {
            LIVE.fetch_sub(layout.size(), Ordering::Relaxed);
            allocated(size);
        }
        ptr
    }
}

#[global_allocator]
static ALLOCATOR: CountingAllocator = CountingAllocator;

type Encoder = fn(&[f32], u32) -> Vec<u8>;

fn measure(encode: Encoder, input: &[f32], rate: u32) -> (f64, usize) {
    let baseline = LIVE.load(Ordering::Relaxed);
    PEAK.store(baseline, Ordering::Relaxed);
    let started = std::time::Instant::now();
    let output = encode(std::hint::black_box(input), rate);
    let elapsed = started.elapsed().as_secs_f64() * 1000.0;
    let peak = PEAK.load(Ordering::Relaxed).saturating_sub(baseline);
    std::hint::black_box(output);
    (elapsed, peak)
}

fn report(rate: u32, seconds: usize, variant: &str, phase: &str, values: &mut [(f64, usize)]) {
    values.sort_by(|a,b| a.0.total_cmp(&b.0));
    let runs = values.len();
    let peak = values.iter().map(|v| v.1).max().unwrap();
    let p95 = (runs as f64 * 0.95).ceil() as usize - 1;
    println!("{rate},{seconds},{variant},{phase},{runs},{:.6},{:.6},{peak}", values[runs / 2].0, values[p95].0);
}

fn main() {
    println!("rate,seconds,variant,phase,runs,p50_ms,p95_ms,peak_alloc_bytes");
    for rate in [16_000, 44_100, 48_000] {
        for seconds in [1, 10, 60] {
            let input: Vec<f32> = (0..rate as usize * seconds)
                .map(|i| (i as f32 * 0.07).sin() * 0.3).collect();
            if seconds == 1 {
                report(rate, seconds, "before", "first_call", &mut [measure(before::encode_wav_16k, &input, rate)]);
                report(rate, seconds, "after", "first_call", &mut [measure(after::encode_wav_16k, &input, rate)]);
            }
            let mut before_times = Vec::with_capacity(30);
            let mut after_times = Vec::with_capacity(30);
            for run in 0..30 {
                // Alternate ordering to distribute thermal/load drift fairly.
                if run % 2 == 0 {
                    before_times.push(measure(before::encode_wav_16k, &input, rate));
                    after_times.push(measure(after::encode_wav_16k, &input, rate));
                } else {
                    after_times.push(measure(after::encode_wav_16k, &input, rate));
                    before_times.push(measure(before::encode_wav_16k, &input, rate));
                }
            }
            report(rate, seconds, "before", "warm", &mut before_times);
            report(rate, seconds, "after", "warm", &mut after_times);
        }
    }
}
RUST
rustc --edition=2024 -O -A dead_code "$benchmark_dir/main.rs" -o "$benchmark_dir/bench"
"$benchmark_dir/bench"

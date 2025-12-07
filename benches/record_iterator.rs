use std::{
    hint::black_box,
    sync::{Arc, atomic::AtomicBool},
};

use criterion::{Criterion, criterion_group, criterion_main};
use csvlens::bench_api::{CsvBaseConfig, CsvConfig, CsvlensRecordIterator};

const PERF_DATA: &str = "benches/data/random_100k.csv";

fn run_iterator(streaming: bool) {
    let stream_active = if streaming {
        Some(Arc::new(AtomicBool::new(true)))
    } else {
        None
    };

    let base_config = CsvBaseConfig::new(b',', false);
    let config = CsvConfig::new(PERF_DATA, stream_active.clone(), base_config);
    let record_iterator = CsvlensRecordIterator::new(Arc::new(config)).unwrap();

    stream_active
        .as_ref()
        .map(|x| x.store(false, std::sync::atomic::Ordering::Relaxed));

    for record in record_iterator {
        let record = record.unwrap();
        black_box(record);
    }
}

fn criterion_benchmark(c: &mut Criterion) {
    c.bench_function("built_in_record_iterator", |b| {
        b.iter(|| run_iterator(false))
    });
    c.bench_function("streaming_record_iterator", |b| {
        b.iter(|| run_iterator(true))
    });
}

criterion_group!(benches, criterion_benchmark);
criterion_main!(benches);

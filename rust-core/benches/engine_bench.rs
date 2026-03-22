//! Criterion benchmarks for the AppScale Engine.
//!
//! Covers:
//!   1. IR throughput — JSON parse + `apply_commit`
//!   2. Layout computation time for varying tree sizes
//!   3. Event dispatch latency

use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion};

use appscale_core::ir::{decode_batch, encode_batch, IrBatch, IrCommand};
use appscale_core::platform::{
    NativeHandle, PlatformBridge, PlatformCapability, PlatformError, PlatformId, PropsDiff,
    PropValue, ScreenSize, TextMetrics, TextStyle, ViewType,
};
use appscale_core::tree::NodeId;
use appscale_core::Engine;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Lightweight bench platform (no mutex overhead on the hot path)
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

struct BenchPlatform {
    next_handle: Mutex<u64>,
}

impl BenchPlatform {
    fn new() -> Self {
        Self {
            next_handle: Mutex::new(1),
        }
    }
}

impl PlatformBridge for BenchPlatform {
    fn platform_id(&self) -> PlatformId {
        PlatformId::Web
    }
    fn create_view(&self, _vt: ViewType, _nid: NodeId) -> NativeHandle {
        let mut h = self.next_handle.lock().unwrap();
        let handle = NativeHandle(*h);
        *h += 1;
        handle
    }
    fn update_view(&self, _h: NativeHandle, _p: &PropsDiff) -> Result<(), PlatformError> {
        Ok(())
    }
    fn remove_view(&self, _h: NativeHandle) {}
    fn insert_child(&self, _parent: NativeHandle, _child: NativeHandle, _idx: usize) {}
    fn remove_child(&self, _parent: NativeHandle, _child: NativeHandle) {}
    fn measure_text(&self, text: &str, style: &TextStyle, _max: f32) -> TextMetrics {
        let fs = style.font_size.unwrap_or(14.0);
        TextMetrics {
            width: fs * 0.6 * text.len() as f32,
            height: fs * 1.2,
            baseline: fs,
            line_count: 1,
        }
    }
    fn screen_size(&self) -> ScreenSize {
        ScreenSize {
            width: 390.0,
            height: 844.0,
        }
    }
    fn scale_factor(&self) -> f32 {
        3.0
    }
    fn supports(&self, _cap: PlatformCapability) -> bool {
        false
    }
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Helpers
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

fn make_batch(n: u64) -> IrBatch {
    let mut batch = IrBatch::new(1);
    // Create root and set it
    batch.push(IrCommand::CreateNode {
        id: NodeId(0),
        view_type: ViewType::Container,
        props: HashMap::new(),
        style: Default::default(),
    });
    batch.push(IrCommand::SetRootNode { id: NodeId(0) });
    for i in 1..=n {
        batch.push(IrCommand::CreateNode {
            id: NodeId(i),
            view_type: ViewType::Container,
            props: HashMap::new(),
            style: Default::default(),
        });
        batch.push(IrCommand::AppendChild {
            parent: NodeId(0),
            child: NodeId(i),
        });
    }
    batch
}

fn make_batch_with_text(n: u64) -> IrBatch {
    let mut batch = IrBatch::new(1);
    batch.push(IrCommand::CreateNode {
        id: NodeId(0),
        view_type: ViewType::Container,
        props: HashMap::new(),
        style: Default::default(),
    });
    batch.push(IrCommand::SetRootNode { id: NodeId(0) });
    for i in 1..=n {
        let mut props = HashMap::new();
        props.insert("text".into(), PropValue::String(format!("Hello {}", i)));
        batch.push(IrCommand::CreateNode {
            id: NodeId(i),
            view_type: ViewType::Text,
            props,
            style: Default::default(),
        });
        batch.push(IrCommand::AppendChild {
            parent: NodeId(0),
            child: NodeId(i),
        });
    }
    batch
}

fn make_tree_batch(depth: u64, breadth: u64) -> IrBatch {
    let mut batch = IrBatch::new(1);
    let mut next_id = 1u64;

    // Create root
    let root_id = next_id;
    batch.push(IrCommand::CreateNode {
        id: NodeId(root_id),
        view_type: ViewType::Container,
        props: HashMap::new(),
        style: Default::default(),
    });
    batch.push(IrCommand::SetRootNode { id: NodeId(root_id) });
    next_id += 1;

    fn add_children(
        batch: &mut IrBatch,
        parent_id: u64,
        depth: u64,
        breadth: u64,
        next_id: &mut u64,
    ) {
        if depth == 0 {
            return;
        }
        for _ in 0..breadth {
            let child_id = *next_id;
            *next_id += 1;
            batch.push(IrCommand::CreateNode {
                id: NodeId(child_id),
                view_type: ViewType::Container,
                props: HashMap::new(),
                style: Default::default(),
            });
            batch.push(IrCommand::AppendChild {
                parent: NodeId(parent_id),
                child: NodeId(child_id),
            });
            add_children(batch, child_id, depth - 1, breadth, next_id);
        }
    }

    add_children(&mut batch, root_id, depth, breadth, &mut next_id);
    batch
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// 1. IR Throughput — JSON parse + apply_commit
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

fn bench_ir_json_decode(c: &mut Criterion) {
    let mut group = c.benchmark_group("ir_json_decode");

    for size in [10, 100, 500, 1000] {
        let batch = make_batch(size);
        let bytes = encode_batch(&batch).unwrap();

        group.bench_with_input(BenchmarkId::from_parameter(size), &bytes, |b, data| {
            b.iter(|| {
                black_box(decode_batch(data).unwrap());
            });
        });
    }
    group.finish();
}

fn bench_ir_json_encode(c: &mut Criterion) {
    let mut group = c.benchmark_group("ir_json_encode");

    for size in [10, 100, 500, 1000] {
        let batch = make_batch(size);

        group.bench_with_input(BenchmarkId::from_parameter(size), &batch, |b, batch| {
            b.iter(|| {
                black_box(encode_batch(batch).unwrap());
            });
        });
    }
    group.finish();
}

fn bench_apply_commit(c: &mut Criterion) {
    let mut group = c.benchmark_group("apply_commit");

    for size in [10, 100, 500] {
        let batch = make_batch(size);

        group.bench_with_input(BenchmarkId::from_parameter(size), &batch, |b, batch| {
            b.iter_with_setup(
                || Engine::new(Arc::new(BenchPlatform::new())),
                |mut engine| {
                    engine.apply_commit(black_box(batch)).unwrap();
                },
            );
        });
    }
    group.finish();
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// 2. Layout computation for varying tree sizes
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

fn bench_tree_apply(c: &mut Criterion) {
    let mut group = c.benchmark_group("tree_apply");

    // (depth, breadth) → approximate total nodes
    let configs: &[(u64, u64, &str)] = &[
        (3, 3, "39_nodes"),    // 3^0 + 3^1 + 3^2 + 3^3 = 40
        (4, 3, "121_nodes"),   // ~121
        (3, 5, "156_nodes"),   // ~156
        (5, 3, "364_nodes"),   // ~364
    ];

    for &(depth, breadth, label) in configs {
        let batch = make_tree_batch(depth, breadth);

        group.bench_with_input(BenchmarkId::new("tree", label), &batch, |b, batch| {
            b.iter_with_setup(
                || Engine::new(Arc::new(BenchPlatform::new())),
                |mut engine| {
                    engine.apply_commit(black_box(batch)).unwrap();
                },
            );
        });
    }
    group.finish();
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// 3. Event dispatch latency
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

fn bench_event_dispatch(c: &mut Criterion) {
    use appscale_core::events::InputEvent;

    let mut group = c.benchmark_group("event_dispatch");

    // Build a small tree to dispatch events against
    let batch = make_tree_batch(3, 3);
    let platform = Arc::new(BenchPlatform::new());
    let mut engine = Engine::new(platform);
    engine.apply_commit(&batch).unwrap();

    let tap = InputEvent::Tap {
        x: 100.0,
        y: 200.0,
        target: Some(NodeId(2)),
    };

    group.bench_function("tap_dispatch", |b| {
        b.iter(|| {
            black_box(engine.handle_event(tap.clone()));
        });
    });

    group.finish();
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// 4. Full pipeline: decode + apply + process_frame
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

fn bench_full_pipeline(c: &mut Criterion) {
    let mut group = c.benchmark_group("full_pipeline");

    for size in [10, 100, 500] {
        let batch = make_batch(size);
        let bytes = encode_batch(&batch).unwrap();

        group.bench_with_input(
            BenchmarkId::from_parameter(size),
            &bytes,
            |b, data| {
                b.iter_with_setup(
                    || Engine::new(Arc::new(BenchPlatform::new())),
                    |mut engine| {
                        let batch = decode_batch(data).unwrap();
                        engine.apply_commit(black_box(&batch)).unwrap();
                        let _ = engine.process_frame();
                    },
                );
            },
        );
    }
    group.finish();
}

criterion_group!(
    benches,
    bench_ir_json_decode,
    bench_ir_json_encode,
    bench_apply_commit,
    bench_tree_apply,
    bench_event_dispatch,
    bench_full_pipeline,
);
criterion_main!(benches);

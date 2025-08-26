use criterion::{black_box, criterion_group, criterion_main, Criterion};
use nova_core::events::{EngineStartEvent, EventSystem, SimpleEventHandler};
use nova_core::resources::{AudioResource, ResourceManager, TextureResource};
use nova_core::threading::{TaskPriority, ThreadEngine};

fn benchmark_event_dispatch(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();

    c.bench_function("event_dispatch", |b| {
        let event_system = EventSystem::new(10000, 5);
        let handler = SimpleEventHandler::new(|_event: &EngineStartEvent| Ok(()));
        event_system.register_handler(handler);

        b.iter(|| {
            let result = event_system.dispatch(black_box(EngineStartEvent));
            assert!(result.is_ok());
        });
    });
}

fn benchmark_resource_creation(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();
    
    c.bench_function("resource_creation", |b| {
        let resource_manager = rt.block_on(async { ResourceManager::new("1GB", 0.8).await }).unwrap();

        b.iter(|| {
            let texture = TextureResource {
                width: black_box(256),
                height: black_box(256),
                data: vec![0u8; 256 * 256 * 4],
                format: "RGBA8".to_string(),
            };

            let handle = rt.block_on(resource_manager.create_resource(texture)).unwrap();
            black_box(handle);
        });
    });
}

fn benchmark_task_submission(c: &mut Criterion) {
    c.bench_function("task_submission", |b| {
        let thread_engine = ThreadEngine::new(4, None).unwrap();

        b.iter(|| {
            let task_id = thread_engine
                .submit_function(
                    "benchmark_task".to_string(),
                    TaskPriority::Normal,
                    || Ok(()),
                )
                .unwrap();
            black_box(task_id);
        });
    });
}

fn benchmark_memory_allocation(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();
    
    c.bench_function("memory_allocation", |b| {
        let resource_manager = rt.block_on(async { ResourceManager::new("2GB", 0.9).await }).unwrap();

        b.iter(|| {
            let audio = AudioResource {
                sample_rate: black_box(44100),
                channels: black_box(2),
                samples: vec![0.0f32; 44100 * 2], // 1 second of audio
            };

            let handle = rt.block_on(resource_manager.create_resource(audio)).unwrap();
            rt.block_on(resource_manager.unload_resource(handle)).unwrap();
        });
    });
}

criterion_group!(
    benches,
    benchmark_event_dispatch,
    benchmark_resource_creation,
    benchmark_task_submission,
    benchmark_memory_allocation
);
criterion_main!(benches);

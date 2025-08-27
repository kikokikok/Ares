use crate::error::{NovaError, NovaResult};
use crossbeam::channel::{unbounded, Receiver, Sender};
use parking_lot::Mutex;
use rayon::{ThreadPool, ThreadPoolBuilder};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use std::thread;
use std::time::{Duration, Instant};
use uuid::Uuid;

/// Task priority levels
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum TaskPriority {
    Low = 0,
    Normal = 1,
    High = 2,
    Critical = 3,
}

/// Task trait for work units
pub trait Task: Send + Sync {
    fn execute(&self) -> NovaResult<()>;
    fn priority(&self) -> TaskPriority {
        TaskPriority::Normal
    }
    fn name(&self) -> &str {
        "UnnamedTask"
    }
}

/// Simple function-based task
pub struct FunctionTask<F>
where
    F: Fn() -> NovaResult<()> + Send + Sync,
{
    function: F,
    name: String,
    priority: TaskPriority,
}

impl<F> FunctionTask<F>
where
    F: Fn() -> NovaResult<()> + Send + Sync,
{
    pub fn new(name: String, priority: TaskPriority, function: F) -> Self {
        Self {
            function,
            name,
            priority,
        }
    }
}

impl<F> Task for FunctionTask<F>
where
    F: Fn() -> NovaResult<()> + Send + Sync,
{
    fn execute(&self) -> NovaResult<()> {
        (self.function)()
    }

    fn priority(&self) -> TaskPriority {
        self.priority
    }

    fn name(&self) -> &str {
        &self.name
    }
}

/// Task wrapper for internal scheduling
struct TaskWrapper {
    #[allow(dead_code)]
    id: Uuid,
    task: Box<dyn Task>,
    priority: TaskPriority,
    submitted_at: Instant,
}

/// Thread pool statistics
#[derive(Debug, Clone)]
pub struct ThreadingStats {
    pub total_tasks_submitted: u64,
    pub total_tasks_completed: u64,
    pub total_tasks_failed: u64,
    pub active_threads: usize,
    pub pending_tasks: usize,
    pub avg_execution_time_us: f64,
}

/// Threading engine for efficient multi-threaded execution and task scheduling
pub struct ThreadEngine {
    /// Task queue sender
    task_sender: Sender<TaskWrapper>,
    /// Worker thread pool
    thread_pool: Arc<ThreadPool>,
    /// Engine running state
    running: Arc<AtomicBool>,
    /// Statistics
    stats: Arc<Mutex<ThreadingStats>>,
    /// Task queue for priority handling
    #[allow(dead_code)]
    task_queue: Arc<Mutex<std::collections::BinaryHeap<TaskWrapper>>>,
}

impl PartialEq for TaskWrapper {
    fn eq(&self, other: &Self) -> bool {
        self.priority == other.priority && self.submitted_at == other.submitted_at
    }
}

impl Eq for TaskWrapper {}

impl PartialOrd for TaskWrapper {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for TaskWrapper {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        // Higher priority first, then earlier submission time
        self.priority
            .cmp(&other.priority)
            .then_with(|| other.submitted_at.cmp(&self.submitted_at))
    }
}

impl ThreadEngine {
    /// Create a new threading engine
    pub fn new(worker_threads: usize, main_thread_affinity: Option<usize>) -> NovaResult<Self> {
        if worker_threads == 0 {
            return Err(NovaError::threading("Worker thread count cannot be zero"));
        }

        let (task_sender, task_receiver) = unbounded::<TaskWrapper>();

        let thread_pool = Arc::new(
            ThreadPoolBuilder::new()
                .num_threads(worker_threads)
                .thread_name(|index| format!("nova-worker-{}", index))
                .build()
                .map_err(|e| {
                    NovaError::threading(format!("Failed to create thread pool: {}", e))
                })?,
        );

        let running = Arc::new(AtomicBool::new(true));
        let stats = Arc::new(Mutex::new(ThreadingStats {
            total_tasks_submitted: 0,
            total_tasks_completed: 0,
            total_tasks_failed: 0,
            active_threads: worker_threads,
            pending_tasks: 0,
            avg_execution_time_us: 0.0,
        }));

        let task_queue = Arc::new(Mutex::new(std::collections::BinaryHeap::new()));

        // Start the scheduler thread
        let scheduler = ThreadScheduler::new(
            task_receiver,
            thread_pool.clone(),
            running.clone(),
            stats.clone(),
            task_queue.clone(),
        );

        thread::Builder::new()
            .name("nova-scheduler".to_string())
            .spawn(move || {
                scheduler.run();
            })
            .map_err(|e| {
                NovaError::threading(format!("Failed to start scheduler thread: {}", e))
            })?;

        // Set main thread affinity if specified
        if let Some(affinity) = main_thread_affinity {
            Self::set_thread_affinity(affinity)?;
        }

        log::info!(
            "Threading engine started with {} worker threads",
            worker_threads
        );

        Ok(Self {
            task_sender,
            thread_pool,
            running,
            stats,
            task_queue,
        })
    }

    /// Submit a task for execution
    pub fn submit_task<T: Task + 'static>(&self, task: T) -> NovaResult<Uuid> {
        let task_id = Uuid::new_v4();
        let priority = task.priority();

        let wrapper = TaskWrapper {
            id: task_id,
            task: Box::new(task),
            priority,
            submitted_at: Instant::now(),
        };

        self.task_sender
            .send(wrapper)
            .map_err(|_| NovaError::threading("Failed to submit task - engine shutting down"))?;

        // Update statistics
        {
            let mut stats = self.stats.lock();
            stats.total_tasks_submitted += 1;
            stats.pending_tasks += 1;
        }

        log::trace!("Submitted task {} with priority {:?}", task_id, priority);
        Ok(task_id)
    }

    /// Submit a function as a task
    pub fn submit_function<F>(
        &self,
        name: String,
        priority: TaskPriority,
        function: F,
    ) -> NovaResult<Uuid>
    where
        F: Fn() -> NovaResult<()> + Send + Sync + 'static,
    {
        let task = FunctionTask::new(name, priority, function);
        self.submit_task(task)
    }

    /// Execute a batch of tasks in parallel
    pub fn execute_batch<T>(&self, tasks: Vec<T>) -> NovaResult<()>
    where
        T: Task + 'static,
    {
        let task_ids: Vec<Uuid> = tasks
            .into_iter()
            .map(|task| self.submit_task(task))
            .collect::<NovaResult<Vec<_>>>()?;

        log::debug!("Submitted batch of {} tasks", task_ids.len());
        Ok(())
    }

    /// Get threading statistics
    pub fn get_stats(&self) -> ThreadingStats {
        self.stats.lock().clone()
    }

    /// Shutdown the threading engine
    pub fn shutdown(&self) -> NovaResult<()> {
        log::info!("Shutting down threading engine...");

        self.running.store(false, Ordering::SeqCst);

        // Wait for pending tasks to complete
        let start_time = Instant::now();
        let timeout = Duration::from_secs(10);

        while start_time.elapsed() < timeout {
            let stats = self.stats.lock();
            if stats.pending_tasks == 0 {
                break;
            }
            drop(stats);
            thread::sleep(Duration::from_millis(10));
        }

        log::info!("Threading engine shutdown complete");
        Ok(())
    }

    /// Set CPU affinity for the current thread (platform-specific)
    fn set_thread_affinity(cpu_id: usize) -> NovaResult<()> {
        // This is a simplified implementation
        // In a real game engine, you'd use platform-specific APIs
        log::debug!("Setting thread affinity to CPU {}", cpu_id);
        Ok(())
    }

    /// Get the number of available CPU cores
    pub fn num_cores() -> usize {
        num_cpus::get()
    }

    /// Get the number of worker threads
    pub fn worker_count(&self) -> usize {
        self.thread_pool.current_num_threads()
    }
}

/// Internal scheduler for managing task execution
struct ThreadScheduler {
    task_receiver: Receiver<TaskWrapper>,
    thread_pool: Arc<ThreadPool>,
    running: Arc<AtomicBool>,
    stats: Arc<Mutex<ThreadingStats>>,
    task_queue: Arc<Mutex<std::collections::BinaryHeap<TaskWrapper>>>,
}

impl ThreadScheduler {
    fn new(
        task_receiver: Receiver<TaskWrapper>,
        thread_pool: Arc<ThreadPool>,
        running: Arc<AtomicBool>,
        stats: Arc<Mutex<ThreadingStats>>,
        task_queue: Arc<Mutex<std::collections::BinaryHeap<TaskWrapper>>>,
    ) -> Self {
        Self {
            task_receiver,
            thread_pool,
            running,
            stats,
            task_queue,
        }
    }

    fn run(self) {
        log::debug!("Task scheduler started");

        while self.running.load(Ordering::SeqCst) {
            // Receive new tasks
            while let Ok(task_wrapper) = self.task_receiver.try_recv() {
                let mut queue = self.task_queue.lock();
                queue.push(task_wrapper);
            }

            // Process high-priority tasks first
            let task_to_execute = {
                let mut queue = self.task_queue.lock();
                queue.pop()
            };

            if let Some(task_wrapper) = task_to_execute {
                let stats = self.stats.clone();
                let running = self.running.clone();

                self.thread_pool.spawn(move || {
                    if !running.load(Ordering::SeqCst) {
                        return;
                    }

                    let start_time = Instant::now();
                    let task_name = task_wrapper.task.name().to_string();

                    match task_wrapper.task.execute() {
                        Ok(()) => {
                            let execution_time = start_time.elapsed();
                            let mut stats = stats.lock();
                            stats.total_tasks_completed += 1;
                            stats.pending_tasks = stats.pending_tasks.saturating_sub(1);
                            stats.avg_execution_time_us = (stats.avg_execution_time_us
                                + execution_time.as_micros() as f64)
                                / 2.0;

                            log::trace!("Task '{}' completed in {:?}", task_name, execution_time);
                        }
                        Err(e) => {
                            let mut stats = stats.lock();
                            stats.total_tasks_failed += 1;
                            stats.pending_tasks = stats.pending_tasks.saturating_sub(1);

                            log::error!("Task '{}' failed: {}", task_name, e);
                        }
                    }
                });
            } else {
                // No tasks available, sleep briefly
                thread::sleep(Duration::from_millis(1));
            }
        }

        log::debug!("Task scheduler stopped");
    }
}

// Sample task implementations
pub struct ComputeTask {
    name: String,
    work_amount: usize,
}

impl ComputeTask {
    pub fn new(name: String, work_amount: usize) -> Self {
        Self { name, work_amount }
    }
}

impl Task for ComputeTask {
    fn execute(&self) -> NovaResult<()> {
        // Simulate computational work
        let start = Instant::now();
        let mut sum = 0u64;
        for i in 0..self.work_amount {
            sum = sum.wrapping_add(i as u64);
        }

        let duration = start.elapsed();
        log::trace!(
            "Compute task '{}' processed {} items in {:?} (sum: {})",
            self.name,
            self.work_amount,
            duration,
            sum
        );

        Ok(())
    }

    fn name(&self) -> &str {
        &self.name
    }
}

pub struct IOTask {
    name: String,
    file_path: std::path::PathBuf,
}

impl IOTask {
    pub fn new(name: String, file_path: std::path::PathBuf) -> Self {
        Self { name, file_path }
    }
}

impl Task for IOTask {
    fn execute(&self) -> NovaResult<()> {
        // Simulate I/O work
        match std::fs::metadata(&self.file_path) {
            Ok(metadata) => {
                log::trace!(
                    "IO task '{}' read file {:?} (size: {} bytes)",
                    self.name,
                    self.file_path,
                    metadata.len()
                );
            }
            Err(e) => {
                log::warn!(
                    "IO task '{}' failed to read {:?}: {}",
                    self.name,
                    self.file_path,
                    e
                );
            }
        }

        Ok(())
    }

    fn name(&self) -> &str {
        &self.name
    }

    fn priority(&self) -> TaskPriority {
        TaskPriority::High // I/O tasks typically have higher priority
    }
}

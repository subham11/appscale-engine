//! Scheduler — coordinates React commits with Rust layout and platform mount.
//!
//! The fundamental tension: React batches updates asynchronously (concurrent mode),
//! but native UI must be updated on the main thread synchronously within a 16ms frame.
//!
//! The scheduler resolves this by:
//! 1. Accepting IR batches from JS at any time
//! 2. Coalescing multiple batches within a single frame
//! 3. Running layout on a background thread
//! 4. Dispatching mount operations to the main thread at vsync
//!
//! Priority lanes (matching React's scheduler priorities):
//! - Immediate: user input responses (touch feedback, text input)
//! - UserBlocking: discrete interactions (button press, toggle)
//! - Normal: data fetches, state updates
//! - Low: prefetching, analytics
//! - Idle: cleanup, cache warming

use crate::ir::IrBatch;
use std::collections::VecDeque;
use std::sync::{Arc, Mutex, Condvar};
use std::time::{Duration, Instant};

/// Frame budget: 16.67ms for 60fps, 8.33ms for 120fps.
/// We aim for 60fps by default; platform bridge can override.
const DEFAULT_FRAME_BUDGET: Duration = Duration::from_micros(16_667);

/// Maximum batches to coalesce per frame before forcing a flush.
/// Prevents starvation under high update load.
const MAX_COALESCE_COUNT: usize = 8;

/// Priority lanes for scheduling commits.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Priority {
    /// User input (touch, keyboard) — process immediately, skip coalescing.
    Immediate = 0,
    /// Discrete user interactions — process within current frame.
    UserBlocking = 1,
    /// Normal state updates — can be coalesced across frames.
    Normal = 2,
    /// Low priority — prefetch, background sync.
    Low = 3,
    /// Idle work — only when nothing else is pending.
    Idle = 4,
}

/// A scheduled work item: an IR batch with priority and timing metadata.
#[derive(Debug)]
struct WorkItem {
    batch: IrBatch,
    priority: Priority,
    enqueued_at: Instant,
}

/// The scheduler manages the work queue and coordinates frame timing.
pub struct Scheduler {
    /// Pending work items, sorted by priority (highest first).
    queue: Arc<Mutex<VecDeque<WorkItem>>>,

    /// Signal to wake the processing thread when work arrives.
    notify: Arc<Condvar>,

    /// Frame budget (can be adjusted for 120fps displays).
    frame_budget: Duration,

    /// Backpressure: if true, Rust is still processing the previous frame.
    /// JS should coalesce more aggressively.
    processing: Arc<Mutex<bool>>,

    /// Frame statistics for DevTools.
    stats: Arc<Mutex<FrameStats>>,
}

/// Per-frame timing statistics (exposed to DevTools).
#[derive(Debug, Clone, Default)]
pub struct FrameStats {
    pub frame_count: u64,
    pub last_frame_duration: Duration,
    pub last_layout_duration: Duration,
    pub last_mount_duration: Duration,
    pub batches_coalesced: u32,
    pub frames_dropped: u32,
}

impl Scheduler {
    pub fn new() -> Self {
        Self {
            queue: Arc::new(Mutex::new(VecDeque::new())),
            notify: Arc::new(Condvar::new()),
            frame_budget: DEFAULT_FRAME_BUDGET,
            processing: Arc::new(Mutex::new(false)),
            stats: Arc::new(Mutex::new(FrameStats::default())),
        }
    }

    /// Set frame budget (e.g., 8.33ms for 120fps ProMotion displays).
    pub fn set_frame_budget(&mut self, budget: Duration) {
        self.frame_budget = budget;
    }

    /// Enqueue an IR batch for processing.
    /// Called from JS thread via JSI.
    pub fn enqueue(&self, batch: IrBatch, priority: Priority) {
        let item = WorkItem {
            batch,
            priority,
            enqueued_at: Instant::now(),
        };

        {
            let mut queue = self.queue.lock().unwrap();

            // Insert in priority order (highest priority = lowest ordinal = front)
            let pos = queue.iter()
                .position(|existing| existing.priority > priority)
                .unwrap_or(queue.len());
            queue.insert(pos, item);
        }

        // Wake the processing thread
        self.notify.notify_one();
    }

    /// Check if the engine is currently processing a frame.
    /// JS scheduler uses this for backpressure — if true, coalesce more.
    pub fn is_processing(&self) -> bool {
        *self.processing.lock().unwrap()
    }

    /// Drain pending work items for the current frame.
    /// Returns batches to process, coalescing multiple Normal/Low priority
    /// batches into a single processing round.
    ///
    /// Rules:
    /// - Immediate priority: return immediately (one at a time)
    /// - UserBlocking: return all UserBlocking items in queue
    /// - Normal/Low: coalesce up to MAX_COALESCE_COUNT
    /// - Idle: only return if no other priority is pending
    pub fn drain_frame(&self) -> Vec<IrBatch> {
        let mut queue = self.queue.lock().unwrap();

        if queue.is_empty() {
            return vec![];
        }

        let mut batches = Vec::new();

        // Check highest priority in queue
        let top_priority = queue.front().map(|w| w.priority).unwrap_or(Priority::Idle);

        match top_priority {
            Priority::Immediate => {
                // Process one immediate item right now
                if let Some(item) = queue.pop_front() {
                    batches.push(item.batch);
                }
            }
            Priority::UserBlocking => {
                // Drain all user-blocking items
                while let Some(item) = queue.front() {
                    if item.priority <= Priority::UserBlocking {
                        batches.push(queue.pop_front().unwrap().batch);
                    } else {
                        break;
                    }
                }
            }
            _ => {
                // Coalesce normal/low/idle items
                let count = queue.len().min(MAX_COALESCE_COUNT);
                for _ in 0..count {
                    if let Some(item) = queue.pop_front() {
                        batches.push(item.batch);
                    }
                }
            }
        }

        batches
    }

    /// Record frame timing (called by Engine after processing).
    pub fn record_frame(
        &self,
        layout_duration: Duration,
        mount_duration: Duration,
        batches_processed: u32,
    ) {
        let mut stats = self.stats.lock().unwrap();
        stats.frame_count += 1;
        stats.last_layout_duration = layout_duration;
        stats.last_mount_duration = mount_duration;
        stats.last_frame_duration = layout_duration + mount_duration;
        stats.batches_coalesced = batches_processed;

        if stats.last_frame_duration > self.frame_budget {
            stats.frames_dropped += 1;
        }
    }

    /// Get current frame statistics (for DevTools).
    pub fn stats(&self) -> FrameStats {
        self.stats.lock().unwrap().clone()
    }

    /// Check if there's pending work.
    pub fn has_pending_work(&self) -> bool {
        !self.queue.lock().unwrap().is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::IrBatch;

    #[test]
    fn test_priority_ordering() {
        let sched = Scheduler::new();

        sched.enqueue(IrBatch::new(1), Priority::Normal);
        sched.enqueue(IrBatch::new(2), Priority::Immediate);
        sched.enqueue(IrBatch::new(3), Priority::Low);

        let batches = sched.drain_frame();
        // Immediate should come first and alone
        assert_eq!(batches.len(), 1);
        assert_eq!(batches[0].commit_id, 2);

        // Next drain gets Normal
        let batches = sched.drain_frame();
        assert!(batches.iter().any(|b| b.commit_id == 1));
    }

    #[test]
    fn test_backpressure() {
        let sched = Scheduler::new();
        assert!(!sched.is_processing());

        *sched.processing.lock().unwrap() = true;
        assert!(sched.is_processing());
    }
}

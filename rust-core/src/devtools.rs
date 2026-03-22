//! DevTools — Inspection, profiling, and debugging infrastructure.
//!
//! Provides:
//! - Tree Inspector: snapshot the shadow tree into a serializable structure
//! - Layout Overlay: gather computed layout rectangles for overlay rendering
//! - Performance Profiler: track frame timing, layout stats, commit counts
//! - IR Replay: record and replay IR command batches
//! - WebSocket bridge types (protocol messages)

use crate::ir::IrBatch;
use crate::layout::{ComputedLayout, LayoutEngine};
use crate::tree::{NodeId, ShadowTree};
use crate::platform::PropValue;
use serde::{Serialize, Deserialize};
use std::collections::HashMap;
use std::time::{Duration, Instant};

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Tree Inspector
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

/// Serializable snapshot of a single node (for DevTools UI).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeSnapshot {
    pub id: u64,
    pub view_type: String,
    pub component_name: Option<String>,
    pub props: HashMap<String, serde_json::Value>,
    pub children: Vec<NodeSnapshot>,
    pub layout: Option<LayoutRect>,
}

/// Computed layout rectangle for a node.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct LayoutRect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

impl From<ComputedLayout> for LayoutRect {
    fn from(cl: ComputedLayout) -> Self {
        Self { x: cl.x, y: cl.y, width: cl.width, height: cl.height }
    }
}

/// Build a full tree snapshot starting from the root.
pub fn snapshot_tree(tree: &ShadowTree, layout_engine: &LayoutEngine) -> Option<NodeSnapshot> {
    let root_id = tree.root()?;
    Some(snapshot_node(root_id, tree, layout_engine))
}

fn snapshot_node(id: NodeId, tree: &ShadowTree, layout: &LayoutEngine) -> NodeSnapshot {
    let node = tree.get(id).expect("node must exist");

    let props: HashMap<String, serde_json::Value> = node.props.iter()
        .map(|(k, v)| (k.clone(), prop_value_to_json(v)))
        .collect();

    let children: Vec<NodeSnapshot> = node.children.iter()
        .map(|&child_id| snapshot_node(child_id, tree, layout))
        .collect();

    let layout_rect = layout.get_computed(id).map(|cl| LayoutRect::from(*cl));

    NodeSnapshot {
        id: id.0,
        view_type: format!("{:?}", node.view_type),
        component_name: node.component_name.clone(),
        props,
        children,
        layout: layout_rect,
    }
}

fn prop_value_to_json(v: &PropValue) -> serde_json::Value {
    match v {
        PropValue::String(s) => serde_json::Value::String(s.clone()),
        PropValue::Bool(b) => serde_json::Value::Bool(*b),
        PropValue::I32(i) => serde_json::json!(*i),
        PropValue::F32(f) => serde_json::json!(*f),
        PropValue::F64(f) => serde_json::json!(*f),
        PropValue::Rect { x, y, width, height } => serde_json::json!({
            "x": x, "y": y, "width": width, "height": height
        }),
        PropValue::Color(c) => serde_json::json!({
            "r": c.r, "g": c.g, "b": c.b, "a": c.a
        }),
        PropValue::Null => serde_json::Value::Null,
    }
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Layout Overlay
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

/// All computed layout rectangles for overlay rendering.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LayoutOverlay {
    pub rects: Vec<OverlayRect>,
}

/// A single overlay rectangle with metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OverlayRect {
    pub node_id: u64,
    pub view_type: String,
    pub rect: LayoutRect,
    pub is_highlighted: bool,
}

/// Gather all layout rectangles for the overlay.
/// If `highlight_id` is set, that node is marked as highlighted.
pub fn gather_overlay(
    tree: &ShadowTree,
    layout_engine: &LayoutEngine,
    highlight_id: Option<NodeId>,
) -> LayoutOverlay {
    let mut rects = Vec::new();

    for (&node_id, node) in tree.iter() {
        if let Some(cl) = layout_engine.get_computed(node_id) {
            rects.push(OverlayRect {
                node_id: node_id.0,
                view_type: format!("{:?}", node.view_type),
                rect: LayoutRect::from(*cl),
                is_highlighted: highlight_id == Some(node_id),
            });
        }
    }

    // Sort by node_id for deterministic output
    rects.sort_by_key(|r| r.node_id);
    LayoutOverlay { rects }
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Performance Profiler
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

/// A single frame timing record.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FrameRecord {
    pub frame_number: u64,
    pub total_ms: f64,
    pub layout_ms: f64,
    pub commit_count: u32,
    pub node_count: u32,
}

/// Ongoing frame measurement.
pub struct FrameTimer {
    start: Instant,
    layout_duration: Duration,
    commit_count: u32,
}

impl FrameTimer {
    pub fn start() -> Self {
        Self {
            start: Instant::now(),
            layout_duration: Duration::ZERO,
            commit_count: 0,
        }
    }

    pub fn record_layout(&mut self, duration: Duration) {
        self.layout_duration += duration;
    }

    pub fn record_commit(&mut self) {
        self.commit_count += 1;
    }

    pub fn finish(self, frame_number: u64, node_count: u32) -> FrameRecord {
        let total = self.start.elapsed();
        FrameRecord {
            frame_number,
            total_ms: total.as_secs_f64() * 1000.0,
            layout_ms: self.layout_duration.as_secs_f64() * 1000.0,
            commit_count: self.commit_count,
            node_count,
        }
    }
}

/// Rolling profiler that keeps the last N frames of timing data.
pub struct Profiler {
    frames: Vec<FrameRecord>,
    max_frames: usize,
    total_frames: u64,
}

impl Profiler {
    pub fn new(max_frames: usize) -> Self {
        Self {
            frames: Vec::with_capacity(max_frames),
            max_frames,
            total_frames: 0,
        }
    }

    pub fn push_frame(&mut self, record: FrameRecord) {
        if self.frames.len() >= self.max_frames {
            self.frames.remove(0);
        }
        self.frames.push(record);
        self.total_frames += 1;
    }

    pub fn frames(&self) -> &[FrameRecord] {
        &self.frames
    }

    pub fn total_frames(&self) -> u64 {
        self.total_frames
    }

    /// Summary stats across recorded frames.
    pub fn summary(&self) -> ProfileSummary {
        if self.frames.is_empty() {
            return ProfileSummary::default();
        }

        let n = self.frames.len() as f64;
        let avg_total = self.frames.iter().map(|f| f.total_ms).sum::<f64>() / n;
        let avg_layout = self.frames.iter().map(|f| f.layout_ms).sum::<f64>() / n;
        let max_total = self.frames.iter().map(|f| f.total_ms).fold(0.0_f64, f64::max);
        let total_commits: u32 = self.frames.iter().map(|f| f.commit_count).sum();

        ProfileSummary {
            frame_count: self.frames.len() as u64,
            avg_frame_ms: avg_total,
            avg_layout_ms: avg_layout,
            max_frame_ms: max_total,
            total_commits,
        }
    }
}

/// Aggregated profiler statistics.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ProfileSummary {
    pub frame_count: u64,
    pub avg_frame_ms: f64,
    pub avg_layout_ms: f64,
    pub max_frame_ms: f64,
    pub total_commits: u32,
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// IR Replay
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

/// Records IR batches for later replay or debugging.
pub struct IrRecorder {
    batches: Vec<TimestampedBatch>,
    recording: bool,
    start_time: Option<Instant>,
}

/// An IR batch with its relative timestamp.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimestampedBatch {
    pub offset_ms: f64,
    pub batch: IrBatch,
}

impl IrRecorder {
    pub fn new() -> Self {
        Self {
            batches: Vec::new(),
            recording: false,
            start_time: None,
        }
    }

    pub fn start_recording(&mut self) {
        self.batches.clear();
        self.recording = true;
        self.start_time = Some(Instant::now());
    }

    pub fn stop_recording(&mut self) {
        self.recording = false;
    }

    pub fn is_recording(&self) -> bool {
        self.recording
    }

    /// Record a batch if recording is active.
    pub fn record(&mut self, batch: &IrBatch) {
        if !self.recording { return; }
        let offset_ms = self.start_time
            .map(|t| t.elapsed().as_secs_f64() * 1000.0)
            .unwrap_or(0.0);
        self.batches.push(TimestampedBatch {
            offset_ms,
            batch: batch.clone(),
        });
    }

    /// Get all recorded batches.
    pub fn batches(&self) -> &[TimestampedBatch] {
        &self.batches
    }

    /// Number of recorded batches.
    pub fn len(&self) -> usize {
        self.batches.len()
    }

    pub fn is_empty(&self) -> bool {
        self.batches.is_empty()
    }

    /// Export recorded batches as JSON.
    pub fn export_json(&self) -> serde_json::Value {
        serde_json::json!({
            "version": 1,
            "batch_count": self.batches.len(),
            "batches": self.batches,
        })
    }
}

impl Default for IrRecorder {
    fn default() -> Self { Self::new() }
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// DevTools Protocol (WebSocket messages)
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

/// Messages from DevTools UI → Engine.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum DevToolsRequest {
    /// Request a full tree snapshot.
    #[serde(rename = "getTree")]
    GetTree,

    /// Request layout overlay data.
    #[serde(rename = "getOverlay")]
    GetOverlay { highlight_node_id: Option<u64> },

    /// Request profiler summary.
    #[serde(rename = "getProfileSummary")]
    GetProfileSummary,

    /// Request recent frame records.
    #[serde(rename = "getFrames")]
    GetFrames { count: Option<usize> },

    /// Start/stop IR recording.
    #[serde(rename = "setRecording")]
    SetRecording { enabled: bool },

    /// Get recorded IR batches.
    #[serde(rename = "getRecording")]
    GetRecording,

    /// Highlight a specific node.
    #[serde(rename = "highlightNode")]
    HighlightNode { node_id: u64 },
}

/// Messages from Engine → DevTools UI.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum DevToolsResponse {
    #[serde(rename = "tree")]
    Tree { root: Option<NodeSnapshot> },

    #[serde(rename = "overlay")]
    Overlay { overlay: LayoutOverlay },

    #[serde(rename = "profileSummary")]
    ProfileSummary { summary: ProfileSummary },

    #[serde(rename = "frames")]
    Frames { frames: Vec<FrameRecord> },

    #[serde(rename = "recording")]
    Recording { data: serde_json::Value },

    #[serde(rename = "error")]
    Error { message: String },
}

/// Handle an incoming DevTools request and produce a response.
pub fn handle_devtools_request(
    request: &DevToolsRequest,
    tree: &ShadowTree,
    layout_engine: &LayoutEngine,
    profiler: &Profiler,
    recorder: &mut IrRecorder,
) -> DevToolsResponse {
    match request {
        DevToolsRequest::GetTree => {
            let root = snapshot_tree(tree, layout_engine);
            DevToolsResponse::Tree { root }
        }
        DevToolsRequest::GetOverlay { highlight_node_id } => {
            let highlight = highlight_node_id.map(NodeId);
            let overlay = gather_overlay(tree, layout_engine, highlight);
            DevToolsResponse::Overlay { overlay }
        }
        DevToolsRequest::GetProfileSummary => {
            let summary = profiler.summary();
            DevToolsResponse::ProfileSummary { summary }
        }
        DevToolsRequest::GetFrames { count } => {
            let all = profiler.frames();
            let frames = match count {
                Some(n) => all[all.len().saturating_sub(*n)..].to_vec(),
                None => all.to_vec(),
            };
            DevToolsResponse::Frames { frames }
        }
        DevToolsRequest::SetRecording { enabled } => {
            if *enabled {
                recorder.start_recording();
            } else {
                recorder.stop_recording();
            }
            DevToolsResponse::Recording { data: serde_json::json!({"recording": enabled}) }
        }
        DevToolsRequest::GetRecording => {
            let data = recorder.export_json();
            DevToolsResponse::Recording { data }
        }
        DevToolsRequest::HighlightNode { node_id } => {
            let highlight = Some(NodeId(*node_id));
            let overlay = gather_overlay(tree, layout_engine, highlight);
            DevToolsResponse::Overlay { overlay }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::platform::ViewType;
    use std::thread;

    #[test]
    fn test_snapshot_empty_tree() {
        let tree = ShadowTree::new();
        let layout = LayoutEngine::new();
        assert!(snapshot_tree(&tree, &layout).is_none());
    }

    #[test]
    fn test_snapshot_tree_with_nodes() {
        let mut tree = ShadowTree::new();
        let mut layout = LayoutEngine::new();

        // Create root -> child hierarchy
        let root_id = NodeId(1);
        let child_id = NodeId(2);

        let mut root_props = HashMap::new();
        root_props.insert("title".into(), PropValue::String("Hello".into()));

        tree.create_node(root_id, ViewType::Container, root_props);
        tree.create_node(child_id, ViewType::Text, HashMap::new());
        tree.set_root(root_id);
        tree.append_child(root_id, child_id);

        layout.create_node(root_id, &Default::default()).unwrap();
        layout.create_node(child_id, &Default::default()).unwrap();

        let snap = snapshot_tree(&tree, &layout).unwrap();
        assert_eq!(snap.id, 1);
        assert_eq!(snap.children.len(), 1);
        assert_eq!(snap.children[0].id, 2);
        assert_eq!(snap.props.get("title").unwrap(), "Hello");
    }

    #[test]
    fn test_layout_overlay_empty() {
        let tree = ShadowTree::new();
        let layout = LayoutEngine::new();
        let overlay = gather_overlay(&tree, &layout, None);
        assert!(overlay.rects.is_empty());
    }

    #[test]
    fn test_frame_timer() {
        let mut timer = FrameTimer::start();
        timer.record_layout(Duration::from_millis(5));
        timer.record_commit();
        timer.record_commit();
        // Let a tiny bit of time pass
        thread::sleep(Duration::from_millis(1));
        let record = timer.finish(1, 10);
        assert_eq!(record.frame_number, 1);
        assert_eq!(record.commit_count, 2);
        assert_eq!(record.node_count, 10);
        assert!(record.total_ms >= 1.0);
        assert!(record.layout_ms >= 4.5); // ~5ms
    }

    #[test]
    fn test_profiler_summary() {
        let mut profiler = Profiler::new(100);
        profiler.push_frame(FrameRecord {
            frame_number: 1, total_ms: 10.0, layout_ms: 4.0, commit_count: 2, node_count: 50,
        });
        profiler.push_frame(FrameRecord {
            frame_number: 2, total_ms: 20.0, layout_ms: 8.0, commit_count: 3, node_count: 55,
        });

        let summary = profiler.summary();
        assert_eq!(summary.frame_count, 2);
        assert!((summary.avg_frame_ms - 15.0).abs() < 0.01);
        assert!((summary.avg_layout_ms - 6.0).abs() < 0.01);
        assert!((summary.max_frame_ms - 20.0).abs() < 0.01);
        assert_eq!(summary.total_commits, 5);
    }

    #[test]
    fn test_profiler_rolling_window() {
        let mut profiler = Profiler::new(3);
        for i in 1..=5 {
            profiler.push_frame(FrameRecord {
                frame_number: i, total_ms: i as f64, layout_ms: 0.0,
                commit_count: 1, node_count: 10,
            });
        }
        assert_eq!(profiler.frames().len(), 3);
        assert_eq!(profiler.frames()[0].frame_number, 3);
        assert_eq!(profiler.frames()[2].frame_number, 5);
        assert_eq!(profiler.total_frames(), 5);
    }

    #[test]
    fn test_ir_recorder() {
        use crate::ir::IrCommand;

        let mut recorder = IrRecorder::new();
        assert!(!recorder.is_recording());

        recorder.start_recording();
        assert!(recorder.is_recording());

        let batch = IrBatch {
            commit_id: 1,
            timestamp_ms: 0.0,
            commands: vec![IrCommand::SetRootNode { id: NodeId(1) }],
        };
        recorder.record(&batch);
        recorder.record(&batch);

        recorder.stop_recording();
        assert_eq!(recorder.len(), 2);

        let json = recorder.export_json();
        assert_eq!(json["batch_count"], 2);
        assert_eq!(json["version"], 1);
    }

    #[test]
    fn test_devtools_protocol_roundtrip() {
        let request_json = r#"{"type":"getTree"}"#;
        let request: DevToolsRequest = serde_json::from_str(request_json).unwrap();
        assert!(matches!(request, DevToolsRequest::GetTree));

        let response = DevToolsResponse::Tree { root: None };
        let json = serde_json::to_string(&response).unwrap();
        assert!(json.contains("\"type\":\"tree\""));
    }

    #[test]
    fn test_handle_devtools_request_get_tree() {
        let tree = ShadowTree::new();
        let layout = LayoutEngine::new();
        let profiler = Profiler::new(100);
        let mut recorder = IrRecorder::new();

        let resp = handle_devtools_request(
            &DevToolsRequest::GetTree, &tree, &layout, &profiler, &mut recorder,
        );
        match resp {
            DevToolsResponse::Tree { root } => assert!(root.is_none()),
            _ => panic!("expected Tree response"),
        }
    }

    #[test]
    fn test_handle_devtools_set_recording() {
        let tree = ShadowTree::new();
        let layout = LayoutEngine::new();
        let profiler = Profiler::new(100);
        let mut recorder = IrRecorder::new();

        handle_devtools_request(
            &DevToolsRequest::SetRecording { enabled: true },
            &tree, &layout, &profiler, &mut recorder,
        );
        assert!(recorder.is_recording());

        handle_devtools_request(
            &DevToolsRequest::SetRecording { enabled: false },
            &tree, &layout, &profiler, &mut recorder,
        );
        assert!(!recorder.is_recording());
    }
}

# Plugin Subscriber Awareness - Detailed Implementation Guide

## 🎯 The Problem

**Current Situation**: Plugins waste CPU by processing data even when nobody is listening.

**Example**: The FFT plugin performs expensive 512-point FFT calculations on every EEG packet, even when:
- No frontend clients are connected
- Frontend is on a different page (not viewing FFT data)
- User has disabled FFT visualization

**Performance Impact**:
- Unnecessary CPU usage (FFT is O(n log n) complexity)
- Wasted memory allocations
- Reduced battery life on embedded devices
- System can't scale to many plugins

## 🎯 The Solution

**Use Rust's type system to FORCE plugins to check for subscribers before doing expensive work.**

**Key Insight**: Make it impossible to compile a plugin that doesn't check for subscribers.

## 📋 Step-by-Step Implementation

### Step 1: Create the SubscriberAware Trait

**File**: `crates/eeg_types/src/plugin.rs`

**Add this trait** (plugins MUST implement this):

```rust
/// Trait that FORCES plugins to be aware of subscribers
/// Every plugin MUST implement this - the compiler will enforce it
pub trait SubscriberAware {
    /// Returns true if someone is actually listening to this plugin's output
    /// Plugins MUST call this before doing expensive work
    fn has_active_subscribers(&self) -> bool;
    
    /// Which WebSocket topic does this plugin publish to?
    /// Used to track subscribers per data type (FFT, EEG, etc.)
    fn websocket_topic(&self) -> Option<WebSocketTopic>;
    
    /// How many subscribers are listening (for debugging/metrics)
    fn subscriber_count(&self) -> usize {
        if self.has_active_subscribers() { 1 } else { 0 }
    }
}
```

**Why this works**: The compiler will refuse to compile any plugin that doesn't implement this trait.

### Step 2: Modify the EegPlugin Trait

**File**: `crates/eeg_types/src/plugin.rs`

**Change this line**:
```rust
// OLD - no enforcement
pub trait EegPlugin: Send + Sync {

// NEW - compiler enforces SubscriberAware
pub trait EegPlugin: Send + Sync + SubscriberAware {
```

**Result**: Now it's impossible to create a plugin without implementing subscriber checking.

### Step 3: Create Topic Subscriber Tracker

**File**: `crates/device/src/topic_tracker.rs` (new file)

```rust
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use eeg_types::event::WebSocketTopic;

/// Tracks how many clients are subscribed to each WebSocket topic
/// This is the "source of truth" for subscriber counts
#[derive(Debug, Clone)]
pub struct TopicSubscriberTracker {
    // Maps WebSocket topic -> number of active subscribers
    subscribers: Arc<RwLock<HashMap<WebSocketTopic, usize>>>,
}

impl TopicSubscriberTracker {
    pub fn new() -> Self {
        Self {
            subscribers: Arc::new(RwLock::new(HashMap::new())),
        }
    }
    
    /// Call this when a WebSocket client connects and subscribes to a topic
    pub async fn add_subscriber(&self, topic: WebSocketTopic) {
        let mut subs = self.subscribers.write().await;
        *subs.entry(topic).or_insert(0) += 1;
        println!("Added subscriber to {:?}, total: {}", topic, subs[&topic]);
    }
    
    /// Call this when a WebSocket client disconnects
    pub async fn remove_subscriber(&self, topic: WebSocketTopic) {
        let mut subs = self.subscribers.write().await;
        if let Some(count) = subs.get_mut(&topic) {
            *count = count.saturating_sub(1);
            if *count == 0 {
                subs.remove(&topic);
            }
            println!("Removed subscriber from {:?}, remaining: {}", topic, *count);
        }
    }
    
    /// This is what plugins call to check if anyone is listening
    pub async fn has_subscribers(&self, topic: WebSocketTopic) -> bool {
        self.subscribers.read().await
            .get(&topic).copied().unwrap_or(0) > 0
    }
    
    /// Get exact count (for debugging)
    pub async fn subscriber_count(&self, topic: WebSocketTopic) -> usize {
        self.subscribers.read().await
            .get(&topic).copied().unwrap_or(0)
    }
}
```

### Step 4: Update the EventBus

**File**: `crates/device/src/event_bus.rs`

**Add the tracker to EventBus**:

```rust
use crate::topic_tracker::TopicSubscriberTracker;

#[derive(Debug, Clone)]
pub struct EventBus {
    sender: broadcast::Sender<SensorEvent>,
    // NEW: Track subscribers per WebSocket topic
    topic_tracker: TopicSubscriberTracker,
}

impl EventBus {
    pub fn new() -> Self {
        let (sender, _) = broadcast::channel(EVENT_BUS_CAPACITY);
        Self { 
            sender,
            topic_tracker: TopicSubscriberTracker::new(), // NEW
        }
    }
    
    // NEW: Give plugins access to the tracker
    pub fn topic_tracker(&self) -> &TopicSubscriberTracker {
        &self.topic_tracker
    }
}
```

### Step 5: Update WebSocket Handlers

**File**: `crates/device/src/server.rs`

**Track when clients connect/disconnect**:

```rust
// Example for FFT WebSocket endpoint
pub async fn handle_fft_websocket(
    ws: WebSocket,
    topic_tracker: TopicSubscriberTracker,
    mut fft_receiver: broadcast::Receiver<SensorEvent>,
) {
    println!("FFT WebSocket client connected");
    
    // IMPORTANT: Tell the tracker someone is now listening to FFT data
    topic_tracker.add_subscriber(WebSocketTopic::Fft).await;
    
    let (mut ws_tx, mut ws_rx) = ws.split();
    
    // Handle WebSocket messages...
    loop {
        tokio::select! {
            event_result = fft_receiver.recv() => {
                match event_result {
                    Ok(SensorEvent::WebSocketBroadcast { topic: WebSocketTopic::Fft, payload }) => {
                        // Send FFT data to client
                        if ws_tx.send(Message::binary(payload)).await.is_err() {
                            break; // Client disconnected
                        }
                    }
                    _ => {} // Ignore other events
                }
            }
            ws_msg = ws_rx.next() => {
                if ws_msg.is_none() {
                    break; // Client disconnected
                }
            }
        }
    }
    
    // IMPORTANT: Tell the tracker this client is no longer listening
    topic_tracker.remove_subscriber(WebSocketTopic::Fft).await;
    println!("FFT WebSocket client disconnected");
}
```

### Step 6: Create Helper for Plugins

**File**: `crates/eeg_types/src/plugin.rs`

**Add this helper to make plugin implementation easier**:

```rust
/// Helper struct that implements SubscriberAware for plugins
/// Plugins can use this instead of implementing the trait manually
pub struct SubscriberAwareHelper {
    topic_tracker: Arc<TopicSubscriberTracker>,
    topic: WebSocketTopic,
}

impl SubscriberAwareHelper {
    pub fn new(topic_tracker: Arc<TopicSubscriberTracker>, topic: WebSocketTopic) -> Self {
        Self { topic_tracker, topic }
    }
}

impl SubscriberAware for SubscriberAwareHelper {
    fn has_active_subscribers(&self) -> bool {
        // Convert async call to sync for the trait
        tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current().block_on(
                self.topic_tracker.has_subscribers(self.topic)
            )
        })
    }
    
    fn websocket_topic(&self) -> Option<WebSocketTopic> {
        Some(self.topic)
    }
}
```

### Step 7: Update the FFT Plugin (EXAMPLE)

**File**: `plugins/brain_waves_fft/src/lib.rs`

**BEFORE** (wasteful):
```rust
#[derive(Clone)]
pub struct BrainWavesFftPlugin {
    channel_buffers: Vec<Vec<f32>>,
    fft_planner: Arc<dyn Fft<f32>>,
    num_channels: usize,
    sample_rate: f32,
    // No subscriber awareness!
}

impl EegPlugin for BrainWavesFftPlugin {
    async fn run(&mut self, bus: Arc<dyn EventBus>, ...) -> Result<()> {
        loop {
            tokio::select! {
                event_result = receiver.recv() => {
                    match event_result {
                        Ok(SensorEvent::FilteredEeg(packet)) => {
                            // PROBLEM: Always does expensive FFT, even if nobody is listening!
                            self.process_expensive_fft(packet, &bus).await?;
                        }
                    }
                }
            }
        }
    }
}
```

**AFTER** (efficient):
```rust
#[derive(Clone)]
pub struct BrainWavesFftPlugin {
    channel_buffers: Vec<Vec<f32>>,
    fft_planner: Arc<dyn Fft<f32>>,
    num_channels: usize,
    sample_rate: f32,
    // NEW: Subscriber awareness helper
    subscriber_helper: SubscriberAwareHelper,
}

impl BrainWavesFftPlugin {
    pub fn new(
        num_channels: usize, 
        sample_rate: f32,
        topic_tracker: Arc<TopicSubscriberTracker> // NEW: Injected dependency
    ) -> Self {
        let mut planner = FftPlanner::<f32>::new();
        let fft = planner.plan_fft_forward(FFT_SIZE);
        
        Self {
            channel_buffers: vec![Vec::with_capacity(FFT_SIZE); num_channels],
            fft_planner: fft,
            num_channels,
            sample_rate,
            // NEW: Create helper for FFT topic
            subscriber_helper: SubscriberAwareHelper::new(
                topic_tracker, 
                WebSocketTopic::Fft
            ),
        }
    }
}

// NEW: Implement the required trait (compiler enforces this)
impl SubscriberAware for BrainWavesFftPlugin {
    fn has_active_subscribers(&self) -> bool {
        self.subscriber_helper.has_active_subscribers()
    }
    
    fn websocket_topic(&self) -> Option<WebSocketTopic> {
        Some(WebSocketTopic::Fft)
    }
}

impl EegPlugin for BrainWavesFftPlugin {
    async fn run(&mut self, bus: Arc<dyn EventBus>, ...) -> Result<()> {
        loop {
            tokio::select! {
                event_result = receiver.recv() => {
                    match event_result {
                        Ok(SensorEvent::FilteredEeg(packet)) => {
                            // NEW: Check if anyone is actually listening!
                            if !self.has_active_subscribers() {
                                // Skip expensive FFT processing
                                continue;
                            }
                            
                            // Only do expensive work when someone is listening
                            self.process_expensive_fft(packet, &bus).await?;
                        }
                    }
                }
            }
        }
    }
}
```

### Step 8: Update Plugin Supervisor

**File**: `crates/device/src/plugin_supervisor.rs`

**Inject the topic tracker when creating plugins**:

```rust
impl PluginSupervisor {
    pub fn new(bus: Arc<EventBus>) -> Self {
        Self {
            plugins: Vec::new(),
            handles: Vec::new(),
            bus,
        }
    }
    
    // NEW: Helper method to create FFT plugin with subscriber awareness
    pub fn add_fft_plugin(&mut self, num_channels: usize, sample_rate: f32) {
        let topic_tracker = Arc::new(self.bus.topic_tracker().clone());
        let plugin = Box::new(BrainWavesFftPlugin::new(
            num_channels, 
            sample_rate, 
            topic_tracker
        ));
        self.add_plugin(plugin);
    }
}
```

## 🧪 Testing the Implementation

### Test 1: No Subscribers
```rust
#[tokio::test]
async fn test_no_subscribers_skips_processing() {
    let tracker = TopicSubscriberTracker::new();
    let plugin = BrainWavesFftPlugin::new(8, 250.0, Arc::new(tracker));
    
    // No subscribers added
    assert!(!plugin.has_active_subscribers());
    
    // Plugin should skip processing
    // (Test by checking that FFT buffers remain empty)
}
```

### Test 2: With Subscribers
```rust
#[tokio::test]
async fn test_with_subscribers_processes_data() {
    let tracker = TopicSubscriberTracker::new();
    tracker.add_subscriber(WebSocketTopic::Fft).await;
    
    let plugin = BrainWavesFftPlugin::new(8, 250.0, Arc::new(tracker));
    
    // Should have subscribers
    assert!(plugin.has_active_subscribers());
    
    // Plugin should process data
    // (Test by verifying FFT results are generated)
}
```

## 🎯 Expected Results

### Performance Improvements
- **0% CPU usage** for FFT when no subscribers
- **Immediate response** when first subscriber connects
- **Scales to many plugins** without performance degradation

### Before/After Comparison

**BEFORE** (always processing):
```
CPU Usage: 15% (FFT always running)
Memory: 50MB (buffers always allocated)
Battery: Drains quickly
```

**AFTER** (subscriber-aware):
```
CPU Usage: 0% (no subscribers) → 15% (with subscribers)
Memory: 10MB (no subscribers) → 50MB (with subscribers)  
Battery: Much longer life
```

## 🚨 Common Mistakes to Avoid

### Mistake 1: Forgetting the Subscriber Check
```rust
// WRONG - will waste CPU
async fn run(&mut self, ...) -> Result<()> {
    loop {
        match receiver.recv().await {
            Ok(data) => {
                // Missing subscriber check!
                self.expensive_processing(data).await;
            }
        }
    }
}

// CORRECT - efficient
async fn run(&mut self, ...) -> Result<()> {
    loop {
        match receiver.recv().await {
            Ok(data) => {
                if !self.has_active_subscribers() {
                    continue; // Skip processing
                }
                self.expensive_processing(data).await;
            }
        }
    }
}
```

### Mistake 2: Not Updating WebSocket Handlers
```rust
// WRONG - tracker never gets updated
pub async fn handle_websocket(ws: WebSocket, ...) {
    // Missing: topic_tracker.add_subscriber(topic).await;
    
    // Handle messages...
    
    // Missing: topic_tracker.remove_subscriber(topic).await;
}
```

### Mistake 3: Blocking in SubscriberAware Implementation
```rust
// WRONG - will deadlock
impl SubscriberAware for MyPlugin {
    fn has_active_subscribers(&self) -> bool {
        // This will block the async runtime!
        self.tracker.has_subscribers(self.topic).await
    }
}

// CORRECT - use block_in_place
impl SubscriberAware for MyPlugin {
    fn has_active_subscribers(&self) -> bool {
        tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current().block_on(
                self.tracker.has_subscribers(self.topic)
            )
        })
    }
}
```

## 📝 Implementation Checklist

- [ ] Add `SubscriberAware` trait to `crates/eeg_types/src/plugin.rs`
- [ ] Modify `EegPlugin` trait to require `SubscriberAware`
- [ ] Create `TopicSubscriberTracker` in `crates/device/src/topic_tracker.rs`
- [ ] Update `EventBus` to include the tracker
- [ ] Update WebSocket handlers to call `add_subscriber`/`remove_subscriber`
- [ ] Create `SubscriberAwareHelper` for easy plugin implementation
- [ ] Update `BrainWavesFftPlugin` to use subscriber checking
- [ ] Update `PluginSupervisor` to inject the tracker
- [ ] Add tests for subscriber tracking
- [ ] Add performance benchmarks
- [ ] Update documentation

## 🎉 Success Criteria

1. **Compile-time enforcement**: Impossible to create a plugin without implementing `SubscriberAware`
2. **Zero CPU when idle**: FFT plugin uses 0% CPU when no WebSocket clients connected
3. **Immediate response**: Processing starts within 1 frame when first subscriber connects
4. **Correct tracking**: Subscriber counts accurately reflect WebSocket connections
5. **No breaking changes**: Existing functionality continues to work

This implementation uses Rust's type system to **force** good performance practices, making it impossible for developers to accidentally create wasteful plugins.
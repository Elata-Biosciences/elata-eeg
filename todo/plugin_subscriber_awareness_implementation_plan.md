# Plugin Subscriber Awareness Implementation Plan

## Overview
Implement type-enforced subscriber awareness to prevent plugins from performing expensive processing when no clients are subscribed to their output data.

## Architecture Decision
**WebSocket-level per-topic tracking** - Each plugin checks if their specific WebSocket topic has active subscribers.

## Implementation Phases

### Phase 1: Core Trait Definition

#### 1.1 Add SubscriberAware trait to `crates/eeg_types/src/plugin.rs`

```rust
/// Trait for plugins to check if they have active subscribers
/// This trait MUST be implemented by all EegPlugin implementations
pub trait SubscriberAware {
    /// Returns true if there are active subscribers for this plugin's output
    /// Plugins MUST check this before performing expensive operations
    fn has_active_subscribers(&self) -> bool;
    
    /// Get the number of active subscribers (for metrics/debugging)
    fn subscriber_count(&self) -> usize {
        if self.has_active_subscribers() { 1 } else { 0 }
    }
    
    /// Get the WebSocket topic this plugin publishes to
    fn websocket_topic(&self) -> Option<WebSocketTopic>;
}
```

#### 1.2 Modify EegPlugin trait to require SubscriberAware

```rust
/// Core trait that all EEG processing plugins must implement
#[async_trait]
pub trait EegPlugin: Send + Sync + SubscriberAware {
    // ... existing methods unchanged ...
}
```

### Phase 2: WebSocket Topic Subscriber Tracking

#### 2.1 Create TopicSubscriberTracker in `crates/device/src/topic_tracker.rs`

```rust
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use eeg_types::event::WebSocketTopic;

/// Tracks active subscribers per WebSocket topic
#[derive(Debug, Clone)]
pub struct TopicSubscriberTracker {
    subscribers: Arc<RwLock<HashMap<WebSocketTopic, usize>>>,
}

impl TopicSubscriberTracker {
    pub fn new() -> Self {
        Self {
            subscribers: Arc::new(RwLock::new(HashMap::new())),
        }
    }
    
    /// Increment subscriber count for a topic
    pub async fn add_subscriber(&self, topic: WebSocketTopic) {
        let mut subs = self.subscribers.write().await;
        *subs.entry(topic).or_insert(0) += 1;
    }
    
    /// Decrement subscriber count for a topic
    pub async fn remove_subscriber(&self, topic: WebSocketTopic) {
        let mut subs = self.subscribers.write().await;
        if let Some(count) = subs.get_mut(&topic) {
            *count = count.saturating_sub(1);
            if *count == 0 {
                subs.remove(&topic);
            }
        }
    }
    
    /// Get subscriber count for a topic
    pub async fn subscriber_count(&self, topic: WebSocketTopic) -> usize {
        self.subscribers.read().await
            .get(&topic).copied().unwrap_or(0)
    }
    
    /// Check if topic has any subscribers
    pub async fn has_subscribers(&self, topic: WebSocketTopic) -> bool {
        self.subscriber_count(topic).await > 0
    }
}
```

#### 2.2 Integrate tracker into EventBus

```rust
// In crates/device/src/event_bus.rs
use crate::topic_tracker::TopicSubscriberTracker;

#[derive(Debug, Clone)]
pub struct EventBus {
    sender: broadcast::Sender<SensorEvent>,
    topic_tracker: TopicSubscriberTracker,
}

impl EventBus {
    pub fn new() -> Self {
        let (sender, _) = broadcast::channel(EVENT_BUS_CAPACITY);
        Self { 
            sender,
            topic_tracker: TopicSubscriberTracker::new(),
        }
    }
    
    pub fn topic_tracker(&self) -> &TopicSubscriberTracker {
        &self.topic_tracker
    }
}
```

### Phase 3: WebSocket Integration

#### 3.1 Modify WebSocket handlers to track topic subscriptions

```rust
// In crates/device/src/server.rs
// Add topic subscription/unsubscription logic to WebSocket handlers

pub async fn handle_websocket_with_topic(
    ws: WebSocket,
    topic: WebSocketTopic,
    topic_tracker: TopicSubscriberTracker,
    mut receiver: broadcast::Receiver<SensorEvent>,
) {
    // Add subscriber when client connects
    topic_tracker.add_subscriber(topic).await;
    
    let (mut ws_tx, mut ws_rx) = ws.split();
    
    // Handle WebSocket communication...
    
    // Remove subscriber when client disconnects
    topic_tracker.remove_subscriber(topic).await;
}
```

### Phase 4: Plugin Implementation Updates

#### 4.1 Create SubscriberAwarePlugin helper

```rust
// In crates/eeg_types/src/plugin.rs
/// Helper struct for plugins to implement SubscriberAware
pub struct SubscriberAwarePlugin {
    topic_tracker: Arc<TopicSubscriberTracker>,
    topic: WebSocketTopic,
}

impl SubscriberAwarePlugin {
    pub fn new(topic_tracker: Arc<TopicSubscriberTracker>, topic: WebSocketTopic) -> Self {
        Self { topic_tracker, topic }
    }
}

#[async_trait]
impl SubscriberAware for SubscriberAwarePlugin {
    fn has_active_subscribers(&self) -> bool {
        // Use blocking version for sync trait
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

#### 4.2 Update BrainWavesFftPlugin

```rust
// In plugins/brain_waves_fft/src/lib.rs
#[derive(Clone)]
pub struct BrainWavesFftPlugin {
    // ... existing fields ...
    subscriber_aware: SubscriberAwarePlugin,
}

impl BrainWavesFftPlugin {
    pub fn new(
        num_channels: usize, 
        sample_rate: f32,
        topic_tracker: Arc<TopicSubscriberTracker>
    ) -> Self {
        // ... existing initialization ...
        Self {
            // ... existing fields ...
            subscriber_aware: SubscriberAwarePlugin::new(
                topic_tracker, 
                WebSocketTopic::Fft
            ),
        }
    }
}

impl SubscriberAware for BrainWavesFftPlugin {
    fn has_active_subscribers(&self) -> bool {
        self.subscriber_aware.has_active_subscribers()
    }
    
    fn websocket_topic(&self) -> Option<WebSocketTopic> {
        Some(WebSocketTopic::Fft)
    }
}

#[async_trait]
impl EegPlugin for BrainWavesFftPlugin {
    async fn run(&mut self, bus: Arc<dyn EventBus>, ...) -> Result<()> {
        loop {
            tokio::select! {
                event_result = receiver.recv() => {
                    match event_result {
                        Ok(SensorEvent::FilteredEeg(packet)) => {
                            // TYPE-ENFORCED SUBSCRIBER CHECK
                            if !self.has_active_subscribers() {
                                // Skip expensive FFT processing
                                continue;
                            }
                            
                            // Only perform expensive operations when needed
                            self.process_fft_analysis(packet, &bus).await?;
                        }
                    }
                }
            }
        }
    }
}
```

### Phase 5: Plugin Supervisor Updates

#### 5.1 Inject TopicSubscriberTracker into plugins

```rust
// In crates/device/src/plugin_supervisor.rs
impl PluginSupervisor {
    pub fn new(bus: Arc<EventBus>) -> Self {
        Self {
            plugins: Vec::new(),
            handles: Vec::new(),
            bus,
        }
    }
    
    pub fn add_plugin_with_tracker(&mut self, plugin: Box<dyn EegPlugin>) {
        // Plugins now receive the topic tracker during construction
        self.plugins.push(plugin);
    }
}
```

## Benefits

### Performance Benefits
- **Zero CPU overhead** when no FFT subscribers
- **Memory efficiency** - no unnecessary buffer allocations
- **Battery savings** for embedded devices
- **Scalable** - supports many plugins without resource waste

### Type Safety Benefits
- **Compile-time enforcement** - impossible to implement EegPlugin without SubscriberAware
- **Clear contracts** - subscriber awareness is part of the plugin API
- **Self-documenting** - the trait requirement makes the expectation explicit

### Developer Experience
- **Helper structs** reduce boilerplate
- **Clear patterns** for implementing subscriber awareness
- **Gradual migration** - can be implemented incrementally

## Migration Strategy

### Step 1: Add traits (non-breaking)
- Add SubscriberAware trait
- Add helper implementations
- Update documentation

### Step 2: Update EventBus (non-breaking)
- Add TopicSubscriberTracker
- Maintain backward compatibility

### Step 3: Update plugins (breaking)
- Modify EegPlugin trait to require SubscriberAware
- Update existing plugins
- Provide migration guide

### Step 4: WebSocket integration
- Update WebSocket handlers
- Add topic subscription tracking
- Test end-to-end functionality

## Testing Strategy

### Unit Tests
- TopicSubscriberTracker functionality
- SubscriberAware implementations
- Plugin behavior with/without subscribers

### Integration Tests
- WebSocket subscription/unsubscription
- Plugin processing behavior
- Performance benchmarks

### Performance Tests
- CPU usage with/without subscribers
- Memory allocation patterns
- Latency measurements

## Metrics and Monitoring

### Plugin Metrics
- Processing events skipped due to no subscribers
- Subscriber count per topic
- CPU time saved

### System Metrics
- Overall resource utilization
- WebSocket connection patterns
- Plugin performance impact

## Documentation Updates

### Developer Guide
- How to implement SubscriberAware plugins
- Best practices for subscriber checking
- Performance considerations

### API Documentation
- SubscriberAware trait documentation
- TopicSubscriberTracker usage
- Migration guide for existing plugins

---

## Next Steps

1. **Review and approve** this implementation plan
2. **Create detailed code changes** for each phase
3. **Implement incrementally** starting with Phase 1
4. **Test thoroughly** at each phase
5. **Document** the new patterns and best practices

This approach provides maximum performance benefits while using Rust's type system to enforce good practices at compile time.
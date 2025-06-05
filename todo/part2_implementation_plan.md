# Part 2 Implementation Plan: Process Consolidation & DSP Integration

**Created:** 6/4/2025  
**Status:** In Progress  
**Priority:** High - Critical Performance Issue  

## Current State
- ✅ Part 1 Complete: DSP coordinator created, reduced from 4 to 3 processes
- 🔴 Current CPU: 8% (4%, 2%, 2% across 3 daemon processes)
- 🎯 Target CPU: 3% (single daemon process)

## Implementation Strategy

Starting with **Process Consolidation** as it has the highest immediate impact (62% CPU reduction).

### Phase 1: Process Consolidation (Immediate)

#### 1.1 PID File Management
**File**: `daemon/src/pid_manager.rs` (NEW)
**Purpose**: Ensure only one daemon instance runs

**Implementation**:
```rust
// PID file management module
pub struct PidManager {
    pid_file_path: PathBuf,
}

impl PidManager {
    pub fn new(pid_file_path: &str) -> Self
    pub fn acquire_lock(&self) -> Result<(), String>
    pub fn release_lock(&self) -> Result<(), String>
    pub fn is_running(&self) -> bool
    pub fn cleanup_stale_pid(&self) -> Result<(), String>
}
```

#### 1.2 Daemon Main.rs Integration
**File**: `daemon/src/main.rs`
**Changes**:
1. Add PID manager initialization at startup
2. Check for existing instances before starting
3. Cleanup PID file on shutdown
4. Add signal handlers for graceful shutdown

#### 1.3 Systemd Service Update
**File**: `daemon/adc_daemon.service`
**Changes**:
1. Add PIDFile directive
2. Update restart policy
3. Add proper cleanup on stop

### Phase 2: DSP Coordinator Integration

#### 2.1 Replace Current DSP Logic
**File**: `daemon/src/driver_handler.rs`
**Changes**:
1. Remove existing DSP processing
2. Integrate DSP coordinator
3. Add client requirement tracking

#### 2.2 Connection-Aware Processing
**File**: `daemon/src/server.rs`
**Changes**:
1. Track WebSocket connections
2. Map client types to DSP requirements
3. Register/unregister clients with coordinator

### Phase 3: State Management Integration

#### 3.1 Automatic State Transitions
**Implementation**:
1. Idle state when no clients connected (0% CPU)
2. Basic streaming for simple clients (2% CPU)
3. Full processing for FFT clients (3% CPU)

## Expected Results

| Metric | Before | After | Improvement |
|--------|--------|-------|-------------|
| Process Count | 3 | 1 | 67% reduction |
| Total CPU | 8% | 3% | 62% reduction |
| Idle CPU | 2% | 0% | 100% reduction |
| Architecture | Scattered | Centralized | Simplified |

## Implementation Order

1. **PID Management** (Immediate impact)
2. **Process Consolidation** (Major CPU reduction)
3. **DSP Integration** (Eliminate redundant processing)
4. **Connection Tracking** (Enable demand-based processing)
5. **Performance Validation** (Confirm improvements)

## Files to Modify

### New Files
- `daemon/src/pid_manager.rs` - PID file management
- `daemon/src/connection_manager.rs` - WebSocket connection tracking

### Modified Files
- `daemon/src/main.rs` - Add PID management, integrate DSP coordinator
- `daemon/src/lib.rs` - Add new modules
- `daemon/src/driver_handler.rs` - Replace DSP logic with coordinator
- `daemon/src/server.rs` - Add connection tracking
- `daemon/adc_daemon.service` - Update systemd configuration

## Success Criteria

- [ ] Only 1 `eeg_daemon` process running
- [ ] CPU usage reduced to ~3% when active
- [ ] 0% CPU usage when no clients connected
- [ ] No data loss during transitions
- [ ] Responsive client connections (< 100ms)
- [ ] Simplified, maintainable codebase

## Risk Mitigation

1. **Incremental Implementation**: Make changes step by step
2. **Backup Current State**: Ensure rollback capability
3. **Testing**: Validate each phase before proceeding
4. **Monitoring**: Add performance metrics to track improvements

---

**Next Action**: Switch to Code mode to implement PID management and process consolidation.
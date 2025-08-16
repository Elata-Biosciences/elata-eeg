// Simple test to verify WebSocket connection guard logic
let connectionGuard = false;
let ws = null;

function useEffectSimulation(isReady) {
  console.log('useEffect called with isReady:', isReady);
  
  // If the system isn't ready, ensure any existing connection is closed
  // and reset the connection guard to allow a new connection attempt later.
  if (!isReady) {
    if (ws) {
      console.log('System not ready, closing existing WebSocket.');
      ws = null;
    }
    connectionGuard = false;
    return;
  }

  // Check if we're in React Strict Mode development double-run scenario
  // In Strict Mode, the first run sets the guard, and the second run should be ignored
  if (connectionGuard) {
    console.log('Connection guard active, skipping duplicate connection attempt.');
    return;
  }

  // Ensure we don't create duplicate connections
  if (ws) {
    console.log('Duplicate connection.');
    return;
  }
  
  console.log('Connecting to WebSocket');
  // Set the connection guard to prevent duplicate connections
  connectionGuard = true;
  ws = { status: 'connected' }; // Simulate WebSocket connection
  console.log('WebSocket connection established');
}

// Test the double execution scenario
console.log('=== First execution ===');
useEffectSimulation(false); // Not ready initially
useEffectSimulation(true);  // Then becomes ready

console.log('\n=== Second execution (Strict Mode simulation) ===');
useEffectSimulation(true);  // Should be skipped due to connection guard

console.log('\n=== After disconnect ===');
useEffectSimulation(false); // Should reset the guard
useEffectSimulation(true);  // Should connect again
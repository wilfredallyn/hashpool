# Warm Start Time-Series Initialization Plan

## Overview
Implement "warm start" for hashrate time-series by backfilling synthetic historical data on startup/miner connection, allowing graphs to show correct values immediately without the 0-to-correct ramp-up.

---

## Phase 1: Data Structure Changes

### 1.1 Extend MinerInfo to track data provenance
```rust
pub struct MinerInfo {
    // ... existing fields ...
    pub recent_shares: Vec<(u64, f64, bool)>,  // (timestamp, difficulty, is_synthetic)
}
```
- Add boolean flag to mark synthetic vs real shares
- Synthetic shares are backfilled on startup
- Real shares overwrite as they arrive

### 1.2 Create helper function for synthetic backfill
```rust
pub async fn warm_start_miner(&self, id: u32, estimated_hashrate: f64)
```
- Called when miner connects or service restarts
- Takes current `estimated_hashrate` from difficulty system
- Generates 10 shares spread over past 10 seconds
- Each share's difficulty = `(estimated_hashrate * 10) / 10` to match total
- Marks all as `is_synthetic = true`

---

## Phase 2: Translator Changes

### 2.1 Update record_share() in miner_stats.rs
- Mark incoming shares as `is_synthetic = false`
- These will gradually replace the warm-start synthetic data

### 2.2 Call warm_start_miner() when:
- A new miner connects: `add_miner()` → call `warm_start_miner()` with `estimated_hashrate: 0.0` (let it build naturally)
- Service starts: `TranslatorSv2::new()` → for each existing miner, call `warm_start_miner()` with current `estimated_hashrate`

### 2.3 Update get_metrics_snapshot()
- Calculate `sum_difficulty_in_window` from all shares (synthetic + real)
- Include metadata: `has_synthetic_data: bool` and `synthetic_count: u32`

---

## Phase 3: Stats-Pool Changes

### 3.1 Extend HashratePoint struct
```rust
pub struct HashratePoint {
    pub timestamp: u64,
    pub hashrate_hs: f64,
    pub is_synthetic: bool,  // NEW
}
```

### 3.2 Update storage.rs queries
- Retrieve `is_synthetic` flag from database (or calculate on read)
- Pass through to API responses

### 3.3 Update API responses
- Include `is_synthetic` in `/api/hashrate` and `/api/downstream/{id}/hashrate` responses

---

## Phase 4: Dashboard Changes

### 4.1 Filter synthetic points before rendering
- JavaScript receives all points (synthetic + real)
- `Chart.js` dataset filters: only render where `is_synthetic: false`
- This prevents the backfilled data from appearing on the graph

### 4.2 Use estimated_hashrate for "current" display
- Show `estimated_hashrate` separately (already correct)
- Time-series graph shows only real data points (fills in gradually)

---

## Phase 5: Testing

### 5.1 Test warm start on miner connect
- Start translator, connect miner
- Check that `recent_shares` has 10 synthetic points immediately
- Verify `get_metrics_snapshot()` includes them

### 5.2 Test on service restart
- Restart translator
- Connect miner that's still submitting shares
- Verify synthetic backfill uses the current `estimated_hashrate`
- Check that new real shares are marked `is_synthetic = false`

### 5.3 Test graph display
- Verify dashboard filters out synthetic points
- Only real data points appear on graph
- Graph fills gradually as real shares arrive (no ramp-up from 0)

### 5.4 Test metrics accuracy
- Verify window calculations include synthetic data (for DB storage)
- Verify API filtering removes synthetic before display
- Verify hashrate values are correct

---

## Phase 6: Cleanup & Validation

### 6.1 Remove old window reset logic
- Delete `cleanup_old_shares()` if no longer needed
- Rely on natural share TTL (only store recent shares)

### 6.2 Documentation
- Document that synthetic data is internal only
- Explain why it exists (warm start)

### 6.3 Final verification
- Restart service multiple times
- Verify no 0-to-correct ramp-up behavior
- Verify graphs show correct historical data

---

## Key Design Decisions

1. **Synthetic data is internal**: Never shown on dashboard, only used for calculations
2. **Warm start on connect vs restart**: On new miner = start at 0 (natural), on restart = use last known rate
3. **Granularity**: 10 points over 10 seconds = 1 second apart (matches typical snapshot interval)
4. **Lifecycle**: Synthetic shares fade out naturally as real shares arrive

---

## Success Criteria

✅ Graph shows correct hashrate immediately (not ramping from 0)
✅ Graph uses real data points only (synthetic hidden)
✅ Restart doesn't reset to 0 (synthetic backfill bootstraps)
✅ No artificial delay in convergence
✅ All metrics remain accurate

---

## Implementation Order

1. Phase 1: Modify MinerInfo struct
2. Phase 2: Implement warm_start_miner() and update translator
3. Phase 3: Update stats-pool storage and API
4. Phase 4: Update dashboard filtering
5. Phase 5: Comprehensive testing
6. Phase 6: Cleanup and documentation

# Technical Debt & Future Improvements

Issues and improvements deferred during development to maintain focus on core functionality.

## Blocked / Cannot Proceed

### Mint Service Missing SRI Spec APIs

**Status:** Cannot compile - blocked by SRI specification gaps
**Priority:** Critical - Blocking Phase 3 completion

**Current Issue:**
Mint service fails to compile with 5 errors:
1. Missing `plain_connection_tokio` module in `network_helpers_sv2`
2. Missing `PoolMessages` enum in `roles_logic_sv2::parsers_sv2`
3. Missing `Minting` variant in `parsers_sv2`

**Error Details:**
```
error[E0432]: unresolved import `network_helpers_sv2::plain_connection_tokio::PlainConnection`
error[E0432]: unresolved import `roles_logic_sv2::parsers_sv2::PoolMessages`
error[E0433]: failed to resolve: could not find `Minting` in `parsers_sv2`
```

**Locations:**
- `roles/mint/src/lib/sv2_connection/connection.rs:5` - PlainConnection import
- `roles/mint/src/lib/sv2_connection/message_handler.rs:3` - PoolMessages import
- `roles/mint/src/lib/sv2_connection/quote_processing.rs:8,81,120` - PoolMessages and Minting

**Why This Blocks Development:**
The SRI 1.5.0 specification doesn't yet include:
- Plain TCP connection APIs for non-Noise connections (needed for quote protocol)
- Pool-level messages for quote handling (Mint-specific extensions)
- Message variants for Mint Quote request/response

**What Needs To Happen:**
1. SRI specification must be updated to include PoolMessages and plain_connection_tokio APIs
2. Stratum V2 spec must clarify Mint Quote protocol message flow
3. Once spec APIs are available, uncomment "mint" in roles/Cargo.toml
4. Fix mint/src/lib/sv2_connection imports to use proper APIs

**Current Workaround:**
- Mint is commented out in `roles/Cargo.toml` workspace members
- Mint process is disabled in `devenv.nix`
- Mint config files exist but cannot be used

**Owner:**
SRI (Stratum V2 Reference Implementation) maintainers - awaiting spec updates

---

## High Priority

### Mint SV2 Connection Not Using Proper Protocol

**Status:** Currently using PlainConnection workaround
**Priority:** High - Security & Protocol Compliance

**Current Implementation:**
The mint service connects to the pool using `PlainConnection` which:
- Skips Noise encryption handshake (no authentication)
- Skips `SetupConnection` protocol negotiation (no capability flags)
- Is basically raw framed TCP with no security

**Location:** `roles/mint/src/lib/sv2_connection/connection.rs:24`

**What Needs To Happen:**
1. Use proper `Connection::new()` with `HandshakeRole::Initiator` like translator/JD do
2. Send `SetupConnection` message with appropriate flags after handshake
3. Handle `SetupConnectionSuccess/Error` response
4. Store negotiated capabilities
5. Determine if mint should open a mining channel or if quote protocol is channel-less (currently shows port number in channel ID column)

**Reference Implementation:**
- Good example: `roles/translator/src/lib/upstream_sv2/upstream.rs:167`
- Good example: `roles/jd-server/src/lib/job_declarator/mod.rs:467`

**Security Implications:**
- Mint quote requests/responses currently sent in plaintext
- No authentication of pool server
- Vulnerable to MITM attacks
- Not production-ready

---

## Medium Priority

### Shared Config Sections Leaking Service Connection Details

**Status:** Confusing architectural design + inconsistent parsing
**Priority:** Medium - Clean up config organization

**Current Problem:**
1. **Conceptual confusion:** Shared configs contain sections that exist purely for cross-service connections (e.g., `[mint]`, `[pool]`, `[proxy]` in miner.toml that only translator reads). These aren't truly "shared".
2. **Inconsistent parsing:** Roles parse shared configs differently:
   - `pool` and `mint` use `PoolGlobalConfig` struct (type-safe)
   - `web-pool` reads `config/shared/pool.toml` directly via toml parsing (manual extraction)
   - `stats-proxy` and `web-proxy` read `config/shared/miner.toml` directly via toml parsing (manual extraction)

**What Needs To Happen:**
1. Rename `config/shared/miner.toml` → `config/shared/proxy.toml` (clearer intent)
2. Rename translator config files: `tproxy.config.toml` → `proxy.config.toml` (both dev and prod)
3. Create `ProxyGlobalConfig` struct in `roles/roles-utils/config/src/lib.rs` (parallel to `PoolGlobalConfig`)
4. Update all miner-side roles to use `ProxyGlobalConfig` instead of manual toml parsing:
   - `stats-proxy`: load via `ProxyGlobalConfig::from_path()`
   - `web-proxy`: load via `ProxyGlobalConfig::from_path()`
5. Update `pool`-side `web-pool` to use `PoolGlobalConfig` instead of manual toml parsing
6. Update systemd services and deployment docs to use new filenames

**Files to Rename:**
- `config/shared/miner.toml` → `config/shared/proxy.toml`
- `config/tproxy.config.toml` → `config/proxy.config.toml`
- `config/prod/tproxy.config.toml` → `config/prod/proxy.config.toml`
- Update `roles/translator/src/args.rs` default value from `proxy-config.toml`

**Current Code Locations (need refactoring):**
- `roles/roles-utils/config/src/lib.rs:100` - Add `ProxyGlobalConfig` struct here
- `roles/stats-proxy/src/config.rs` - Replace manual toml parsing with struct
- `roles/web-proxy/src/config.rs` - Replace manual toml parsing with struct
- `roles/web-pool/src/config.rs` - Replace manual toml parsing with `PoolGlobalConfig`
- `roles/translator/src/args.rs` - Update default config filename
- Systemd services that reference `tproxy.config.toml`

---

### Web-Proxy Tightly Coupled To Translator Proxy Config

**Status:** Working but architecturally problematic
**Priority:** Medium - Configuration Management

**Current Problem:**
`roles/web-proxy/src/config.rs` loads the entire **translator proxy configuration file** (`tproxy.config.toml`) just to extract 4 fields:
- `downstream_address`
- `downstream_port`
- `upstream_address`
- `upstream_port`

**Why This Sucks:**
1. **Tight coupling** - Web-proxy is now dependent on translator config schema
2. **Schema pollution** - tproxy.config.toml must include fields meant for web-proxy display
3. **Deployment fragility** - Changing translator config can break web-proxy even if unrelated
4. **Conceptual confusion** - Web-proxy shouldn't care about translator internals
5. **Shared config duplication** - These values should come from shared config, not translator-specific config

**Location:**
- `roles/web-proxy/src/config.rs:119-128` - Loads tproxy config
- `config/tproxy.config.toml:29-30` - Has flat `upstream_address/upstream_port` fields just for web-proxy

**What Needs To Happen:**
1. Move network topology config (downstream/upstream addresses and ports) to shared config
   - Add to `config/shared/miner.toml` or `config/shared/pool.toml`
   - Or create `config/shared/network.toml`
2. Create a shim/adapter in translator config to read from shared config if needed
   - This maintains SRI config compatibility without duplication
3. Update web-proxy to read from shared config instead of tproxy.config.toml
4. Consider: Should `stats_proxy_url` also come from shared config?

**Recommended Structure:**
```toml
# config/shared/network.toml
[proxy]
downstream_address = "0.0.0.0"
downstream_port = 34255
upstream_address = "127.0.0.1"
upstream_port = 34254
```

**Then in translator and web-proxy:**
```rust
// Both read from shared config
let network_config = load_shared_config("network.toml")?;
```

**Impact on SRI:**
- Keep SRI configs as-is for backward compatibility
- The shared config would be a new layer that doesn't break existing SRI structure
- Can be gradual migration (Phase 4 or later)

---

## Medium Priority

### Stats Protocol: Newline-Delimited JSON

**Status:** Custom ad-hoc protocol
**Priority:** Low-Medium - Works but fragile

**Current Implementation:**
```rust
// Sender (pool):
let json = serde_json::to_vec(&msg)?;
buffer.extend_from_slice(&json);
buffer.push(b'\n');
stream.write_all(&buffer).await?;

// Receiver (pool-stats):
while let Some(newline_pos) = leftover.iter().position(|&b| b == b'\n') {
    let line = &leftover[..newline_pos];
    handler.handle_message(line).await?;
    leftover.drain(..=newline_pos);
}
```

**Issues:**
- No proper framing (relies on newlines which could appear in JSON strings)
- Manual buffer management is error-prone
- No authentication or encryption
- Text encoding overhead vs binary
- Not self-describing (no version field)

**Options:**
1. **Keep it** - It works, easy to debug with `nc`
2. **Length-prefixed JSON** - Add 4-byte length header, proper framing
3. **MessagePack** - Binary JSON, faster/smaller, still serde compatible
4. **SV2 Custom Messages** - Migrate to SV2 protocol extensions (ties into architecture question above)

**Recommendation:** Keep for now, revisit when we settle on stats architecture

### Time Series Data Not Being Collected

**Status:** Hashrate samples table exists but nothing writes to it
**Priority:** Medium - Needed for dashboard graphs

**Problem:**
- `hashrate_samples` table created but **0 rows** in database
- No code inserts hashrate samples periodically
- `get_hashrate_history()` function exists but returns empty results
- Can't graph hashrate over time without data

**What Needs To Happen:**
1. Add periodic sampling task to pool-stats (every 5 minutes?)
2. Sample current hashrate from `current_stats` and insert into `hashrate_samples`
3. Implement proper hashrate calculation (not just share count)
4. Add data retention policy (delete samples older than 7 days?)

**Example implementation:**
```rust
// In pool-stats main loop or separate task
tokio::spawn(async move {
    let mut interval = tokio::time::interval(Duration::from_secs(300)); // 5 min
    loop {
        interval.tick().await;
        if let Err(e) = db.sample_current_hashrate().await {
            error!("Failed to sample hashrate: {}", e);
        }
    }
});
```

**Current State:**
- `quote_history`: ✅ Being populated (works correctly)
- `hashrate_samples`: ❌ Empty (not being written)

---

### Mint Quote Plumbing Cleanups (from rework plan)

**Status:** Functional but carrying technical debt
**Priority:** Medium – keeps future maintenance simple

1. **Unify quote tracking in the hub.** `QuotePoller` still maintains its own `pending_quotes` map (`roles/pool/src/lib/mining_pool/quote_poller.rs:28`). Move it to rely on `MintPoolMessageHub::pending_quote` / `PendingQuoteContext` so there is one source of truth for outstanding quotes.
2. **Use typed frame builders.** The mint forwarding path still hand-builds frames with `StandardSv2Frame::from_bytes_unchecked()` (`roles/pool/src/lib/mining_pool/mint_connection.rs:315`). Switch to the helpers in `roles/roles-utils/mint-pool-messaging/src/sv2_frames.rs` to remove manual header math.
3. **Decide who owns channel context.** `MintIntegrationManager` only proxies locking-key lookups (`roles/pool/src/lib/mining_pool/mint_integration.rs:14`). Fold that state into `MintPoolMessageHub` (or document why we can’t) so quote routing doesn’t depend on duplicate maps.

Follow-up: After the hub owns pending state, the poller can just iterate `hub.pending_quotes()` and we can delete the bespoke tracker entirely.

---

## Low Priority

### Production Logging Not Working For Several Services

**Status:** Empty log files in /var/log/hashpool/ (0 bytes)
**Priority:** High - Cannot debug production issues without logs

**Current Problem:**
Several services are running but not logging to files in production:
- `mint.log` - 0 bytes (empty)
- `stats-pool.log` - 0 bytes (empty)
- `stats-proxy.log` - 0 bytes (empty)
- `web-pool.log` - 0 bytes (empty)
- `web-proxy.log` - 0 bytes (empty)

While others work fine:
- `bitcoind.log` - 3.2 MB (working)
- `pool.log` - 19.8 MB (working)
- `proxy.log` - 156 MB (working)
- `jd-client.log` - 7.9 MB (working)
- `jd-server.log` - 644 KB (working)

**Related Issue: Stats-Pool Falls Back to .devenv Database**
- When `stats-pool` config file is missing or not found, it falls back to using `/opt/hashpool/.devenv/state/stats-pool/metrics.db` instead of the configured production path
- This causes silent failures where the service runs but uses the wrong database, resulting in empty graphs on the web dashboard
- **Fix:** Ensure config file path is correct in systemd service definition and that the config file is deployed properly

**Root Cause:**
Services like mint, stats-pool, stats-proxy, web-pool, and web-proxy use different logging mechanisms or don't support the `-f` flag that was added to systemd service definitions. They're probably logging to stdout/stderr which is being discarded.

**What Needs To Happen:**
1. Investigate why mint, stats-pool, stats-proxy, web-pool, and web-proxy aren't writing logs
2. Check if these services support the `-f` log file flag (they may use different logging libraries)
3. Either:
   a. Add proper logging flag support to these services, OR
   b. Configure systemd to capture stdout/stderr to files (StandardOutput=file:/var/log/hashpool/service.log), OR
   c. Implement proper logging in the code (e.g., via tracing crate like translator/pool use)
4. Verify all services log properly after fix
5. Consider implementing log rotation (logrotate) since proxy.log is already 156 MB

**Affected Services:**
- `roles/mint` - Logger implementation check needed
- `roles/stats-pool` - Logger implementation check needed
- `roles/stats-proxy` - Logger implementation check needed
- `roles/web-pool` - Logger implementation check needed
- `roles/web-proxy` - Logger implementation check needed

**Systemd Service Files to Update:**
- `scripts/systemd/hashpool-mint.service`
- `scripts/systemd/hashpool-stats-pool.service`
- `scripts/systemd/hashpool-stats-proxy.service`
- `scripts/systemd/hashpool-web-pool.service`
- `scripts/systemd/hashpool-web-proxy.service`

---

### Systemd Service Environment Configuration Scattered

**Status:** Works but repetitive
**Priority:** Low - Operational convenience

**Current Problem:**
Environment variables like `BITCOIND_NETWORK=testnet4` and `RUST_LOG=info` are hardcoded in each systemd service file, duplicated across multiple services. Makes it hard to change globally (e.g., switching from testnet4 to mainnet).

**What Needs To Happen:**
1. Create `/etc/hashpool/hashpool.env` with centralized environment variables:
```ini
BITCOIND_NETWORK=testnet4
RUST_LOG=info
```

2. Update all systemd services to source this file:
```ini
EnvironmentFile=/etc/hashpool/hashpool.env
```

3. Remove duplicate `Environment=` directives from individual service files

**Services affected:**
- hashpool-pool.service
- hashpool-proxy.service
- (any others that add common env vars)

**Benefit:** Single point to change network/log level across all services

---

### Stats Service Connection Identity

**Current:** Mint uses `address.port()` as downstream_id
**Better:** Use proper ID allocation like other downstreams

The mint connection doesn't go through the normal downstream ID allocation because it doesn't use the standard connection path. Once it uses proper SV2, it should get a proper downstream_id.

---

## Documentation Needed

- Document the mint quote protocol flow
- Document why we need both pool-stats and proxy-stats services
- Architecture diagram showing all service connections
- SV2 message flow diagrams

---

## Testing Needed

- Integration test for mint connection failure/reconnection
- Test mint behavior when pool restarts
- Test what happens when mint and JD both connect simultaneously
- Load testing with multiple mints

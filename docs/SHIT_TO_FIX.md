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

## Low Priority

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


# Plugin/Module Architecture for SRI + Hashpool

**Status:** Architectural Proposal for next major version refactor

This document proposes a modular plugin architecture for SRI + Hashpool features, enabling:
- Clear separation between SRI protocol implementation and Hashpool features
- Plugin-based extensions without modifying core pool logic
- Easier upstreaming of improvements to SRI
- Better testing isolation

## Current Architecture Problems

### 1. Monolithic Roles
- `roles/pool/` directly implements quote dispatch, stats, wallet integration, etc.
- All concerns bundled together: validation, protocol, accounting, web, stats
- Hard to test features independently
- Difficult to extract reusable components

### 2. Tight Coupling
- Pool directly imports and calls quote dispatcher, wallet, stats services
- Share validation logic deeply intertwined with quote generation
- Mint service is a bolted-on external dependency with no standard interface
- Each role (pool, translator, jd-server) reimplements stats polling

### 3. Cross-Cutting Concerns
- Stats reporting duplicated in Pool and Translator
- Wallet operations spread across translator without clear boundaries
- Configuration system tightly coupled to each role
- No standard way to add new features across the stack

### 4. Two Separate Workspaces
- `protocols/` contains SRI code and ehash protocol utilities
- `roles/` contains deployment services
- Creates dependency management complexity
- Makes it hard to create reusable library crates

## Proposed Architecture

### Layer 1: Protocol Crates (protocols workspace)
These are pure protocol definitions with NO dependencies on roles.

```
protocols/
├── ehash/                    # Existing: ehash protocol utilities
│   ├── quote.rs              # Quote request/response building
│   ├── locking_key.rs        # Key validation (NEW)
│   └── work.rs               # Difficulty calculations
├── v2/
│   └── ... (SRI protocol definitions)
└── v1/
    └── ... (SRI v1 definitions)
```

**This layer is framework-agnostic.** It defines:
- Message structures
- Validation rules
- Data transformations
- Error types

### Layer 2: Plugin/Hook Interfaces (shared-utils)
Thin crates that define **what** can be extended, NOT **how**.

```
shared-utils/
├── share-hooks/             # Share acceptance event callbacks (NEW)
│   ├── lib.rs
│   └── Cargo.toml
├── pool-extensions/         # Plugin points for pool (NEW)
│   ├── AcceptanceHook trait
│   ├── RewardCalculator trait
│   ├── StatsProvider trait
│   └── WalletBridge trait
├── translator-extensions/   # Plugin points for translator (NEW)
│   ├── SharePreprocessor trait
│   ├── WalletManager trait
│   └── QuoteReceiver trait
├── stats/                   # Existing: stats collection interface
├── config/                  # Existing: config loading
└── network-helpers/         # Existing: network utilities
```

**Key principle:** These crates define **interfaces**, not implementations.

### Layer 3: Plugin Implementations (plugins workspace - NEW)
Implementations of the hook interfaces that can be:
- Enabled/disabled at runtime
- Loaded from configuration
- Tested in isolation
- Shared between roles

```
plugins/
├── ehash-hooks/             # Quote dispatch as plugin (NEW)
│   ├── lib.rs (QuoteDispatchHook implementation)
│   └── Cargo.toml
├── stats-plugins/           # Stats collection plugins (NEW)
│   ├── prometheus-exporter/
│   ├── json-snapshots/
│   └── rolling-averages/
├── wallet-plugins/          # Wallet integrations (NEW)
│   ├── cdk-wallet/         (Cashu wallet)
│   └── balance-tracker/
└── reward-plugins/          # Reward calculation algorithms (NEW)
    ├── linear-share-weight/
    ├── proportional-payout/
    └── variance-reduction/
```

**Key principle:** Plugins depend on shared-utils interfaces, NOT on each role.

### Layer 4: Core Roles (roles workspace - REFACTORED)
These become thin orchestration layers that:
- Load plugins based on configuration
- Connect plugins via interfaces
- Handle deployment-specific concerns
- Remain framework-agnostic

```
roles/
├── pool/                    # REFACTORED: plugin-based
│   ├── src/
│   │   ├── lib.rs          # Core pool logic
│   │   ├── plugin_loader.rs # Load/register plugins
│   │   └── hooks.rs        # Hook invocation points
│   └── Cargo.toml
├── translator/              # REFACTORED: plugin-based
├── mint/                    # REFACTORED: plugin-based
└── ...
```

**Key principle:** Roles define WHERE hooks are called, not HOW they're implemented.

## Implementation Example: Quote Dispatch

### Current (Monolithic)
```
pool/src/mining_pool/
├── message_handler.rs       # Directly calls dispatcher
├── quote_dispatch_hook.rs   # Hook implementation in pool
└── mint_integration.rs      # Pool-specific mint logic
```

### Proposed (Modular)
```
protocols/ehash/
├── quote.rs                 # Quote request/response building
├── locking_key.rs          # Key validation
└── lib.rs                   # Exports

plugins/ehash-hooks/
├── lib.rs
├── quote_dispatch.rs        # QuoteDispatchHook implementation
└── Cargo.toml
   dependencies:
   - share-hooks (interface)
   - ehash (protocol)
   - quote-dispatcher (implementation)

roles/pool/
├── src/
│   ├── lib.rs              # Core pool logic
│   ├── plugin_loader.rs    # Loads ehash-hooks plugin
│   └── hooks.rs
│       ```
│       for hook in self.share_hooks {
│           hook.on_share_accepted(event).await
│       }
│       ```
└── Cargo.toml
   dependencies:
   - share-hooks (trait only)
   - plugins/ehash-hooks (optional, loaded at runtime)
```

**Benefits:**
- Pool doesn't know about quote dispatch specifics
- Quote dispatch can be tested independently
- Multiple implementations can coexist
- Disabling quotes is just removing the plugin

## Dependency Graph (Proposed)

```
┌─────────────────────────────────────────────────────────┐
│  Deployments (roles)                                    │
│  ├─ pool/        ────┐                                  │
│  ├─ translator/  ────┤                                  │
│  └─ mint/        ────┤                                  │
└────────────────────────┼──────────────────────────────┘
                         │
                         ↓ (depends on)
            ┌────────────────────────┐
            │ Plugin Loader Config   │
            └────────────────────────┘
                         │
                         ↓
    ┌────────────────────────────────────────┐
    │ Plugins (feature-specific)             │
    ├─ ehash-hooks/     ──────┐              │
    ├─ stats-plugins/   ──────┤              │
    └─ wallet-plugins/  ──────┤              │
    └────────────────────────────────────────┘
                         │
                         ↓ (depend on)
    ┌────────────────────────────────────────┐
    │ Shared Interfaces (shared-utils)       │
    ├─ share-hooks/     (ShareAcceptanceHook)│
    ├─ pool-extensions/ (AcceptanceHook, etc)│
    ├─ stats/           (StatsProvider)      │
    └─ config/          (ConfigLoader)       │
    └────────────────────────────────────────┘
                         │
                         ↓ (depend on)
    ┌────────────────────────────────────────┐
    │ Protocol & Utilities (protocols)       │
    ├─ ehash/           (Quote protocol)     │
    ├─ v2/              (SRI protocol)       │
    └─ v1/              (SRI v1 protocol)    │
    └────────────────────────────────────────┘
```

**Key Principle:** Dependencies ONLY go DOWN. Nothing in a lower layer knows about upper layers.

## Implementation Roadmap

### Phase 1: Define Interfaces (1 week)
1. Move `share-hooks` from `roles-utils` to `shared-utils`
2. Create `pool-extensions` with key hook points
3. Create `translator-extensions` with key hook points
4. Audit existing code for plugin points

### Phase 2: Extract Plugins (2-3 weeks)
1. Move quote dispatch to `plugins/ehash-hooks`
2. Extract stats collectors to `plugins/stats-plugins`
3. Extract wallet logic to `plugins/wallet-plugins`
4. Create `plugins/config-loader` for plugin discovery

### Phase 3: Refactor Roles (2-4 weeks)
1. Add plugin loader to Pool
2. Add plugin loader to Translator
3. Add plugin loader to Mint
4. Update all hook invocation points
5. Create plugin registry and discovery mechanism

### Phase 4: Testing & Documentation (1-2 weeks)
1. Write plugin development guide
2. Document plugin interfaces
3. Add plugin example (dummy quote tracker)
4. Create plugin versioning/compatibility docs

### Phase 5: Configuration & Runtime (1-2 weeks)
1. Add plugin configuration to config system
2. Add runtime plugin enable/disable
3. Add plugin dependency resolution
4. Add plugin hot-reload (optional)

## File Organization (Final State)

```
hashpool/
├── protocols/                    # SRI + ehash protocol crates
│   ├── ehash/
│   │   ├── quote.rs              # ✅ Quote building
│   │   ├── locking_key.rs        # ✅ Key validation
│   │   └── ... (existing)
│   └── v2/, v1/                  # ✅ SRI unchanged
│
├── shared-utils/                 # ✨ NEW: Interfaces only
│   ├── share-hooks/              # ✅ Already moved
│   ├── pool-extensions/          # ✨ Pool plugin points
│   ├── translator-extensions/    # ✨ Translator plugin points
│   ├── stats/                    # ✅ Stats interface (moved)
│   ├── config/                   # ✅ Config system (moved)
│   └── ... (existing utils)
│
├── plugins/                      # ✨ NEW: Feature implementations
│   ├── ehash-hooks/              # ✨ Quote dispatch plugin
│   │   ├── src/quote_dispatch.rs
│   │   └── Cargo.toml
│   ├── stats-plugins/            # ✨ Stats collection plugins
│   │   ├── json-snapshots/
│   │   ├── prometheus/
│   │   └── Cargo.toml
│   ├── wallet-plugins/           # ✨ Wallet plugins
│   │   ├── cdk-wallet/
│   │   └── Cargo.toml
│   └── reward-plugins/           # ✨ Reward calculation plugins
│
└── roles/                        # ✅ Deployment services (refactored)
    ├── pool/
    │   ├── src/
    │   │   ├── lib.rs            # Core pool logic
    │   │   ├── plugin_loader.rs  # ✨ Load plugins
    │   │   └── hooks.rs          # ✨ Invocation points
    │   └── Cargo.toml            # Minimal deps
    ├── translator/
    ├── mint/
    └── ... (existing roles)
```

## Advantages of This Architecture

### For Developers
✅ **Clear ownership**: Know exactly where to add new features
✅ **Easy testing**: Test plugins in isolation without spinning up full role
✅ **Quick iteration**: Add a new plugin without touching role code
✅ **Code reuse**: Share plugins across roles (e.g., stats collection)

### For Operators
✅ **Feature control**: Enable/disable features via configuration
✅ **Custom implementations**: Swap out plugins for different algorithms
✅ **Reduced dependencies**: Only load plugins you need
✅ **Safer upgrades**: Update plugins independently from roles

### For SRI Contributions
✅ **Less divergence**: Core role code remains close to SRI
✅ **Easier rebasing**: Plugin layer isolates hashpool-specific features
✅ **Better documentation**: Clear interface contracts
✅ **Community plugins**: Others can build on interfaces

### For Future Versions
✅ **Easy monorepo split**: Move `plugins/` and `shared-utils/` to separate repo
✅ **Plugin distribution**: Package plugins as separate crates
✅ **Version compatibility**: Plugin versioning for different SRI versions
✅ **Ecosystem**: Enable community plugins for any SRI + hashpool combo

## Migration Strategy

### Minimal Disruption
1. **Keep existing code working** - All changes are additive
2. **Parallel implementation** - Old code and new plugins coexist initially
3. **Gradual cutover** - Disable old code when plugin is proven
4. **Backward compatibility** - Old configs still work (with deprecation warnings)

### Example: Quote Dispatch Migration
```rust
// OLD (deprecated but still works)
if let Some(dispatcher) = &pool.quote_dispatcher {
    dispatch_quote(...).await
}

// NEW (using plugins)
for hook in &pool.share_hooks {
    hook.on_share_accepted(event).await
}

// Both run initially, old code disabled later via config
```

## Next Steps

1. **Review this proposal** with stakeholders
2. **Create shared-utils workspace** with initial interfaces
3. **Start Phase 1** (interface definition)
4. **Establish plugin development guidelines**
5. **Create plugin template** for community contributions

## Open Questions

1. **Plugin discovery**: Hardcoded list vs. dynamic loading vs. manifest file?
2. **Plugin versions**: How to handle plugin API evolution?
3. **Plugin composition**: Can plugins depend on each other?
4. **Configuration**: Plugin config in main config file or separate plugin configs?
5. **Hot reload**: Support runtime plugin enable/disable or only at startup?
6. **Plugin distribution**: Package in monorepo or separate crates?

## Reference: Share-Hooks as Template

The current `share-hooks` implementation is a good template for the plugin architecture:

```rust
// ✅ Protocol-agnostic trait
pub trait ShareAcceptanceHook: Send + Sync {
    async fn on_share_accepted(&self, event: ShareAcceptedEvent) -> Result<(), HookError>;
}

// ✅ Simple data types
pub struct ShareAcceptedEvent { /* ... */ }
pub enum HookError { /* ... */ }

// ✅ No role-specific logic
// ✅ No database dependencies
// ✅ Easy to test in isolation
// ✅ Easy to implement multiple times
```

This is the pattern that should be replicated for:
- Pool acceptance hooks
- Stats providers
- Wallet managers
- Reward calculators
- Config loaders

## Conclusion

A plugin architecture provides a clear separation of concerns while maintaining the simplicity of the current codebase. It enables:

- **Better testing** through isolated plugin testing
- **Easier contributions** with clear extension points
- **Simpler maintenance** by reducing coupling
- **Future scalability** when moving to multiple repos
- **Community engagement** by enabling third-party plugins

This proposal moves SRI + Hashpool toward a true modular system where features are plugins, not hard-coded options.

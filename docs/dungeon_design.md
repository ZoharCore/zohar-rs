# Instanced Dungeon Map Design

## Background and Context
The project aims to introduce instanced dungeon maps into the Rust server emulator, improving upon the legacy implementation.

### Legacy Implementation (`/legacy/`)
* **Map Instancing Concept:** The legacy server generated instances by taking the base numeric Map ID, multiplying it by 10,000, and adding an auto-incrementing instance number. This effectively created isolated "private maps" within the same logical map data structures.
* **Routing/Channels:** It heavily relied on a unique channel (often hardcoded as `ch99`) to host these non-channel specific instances or event maps.
* **Limitations:**
  * Magic numbers scattered across the codebase (e.g., `lMapIndex >= 10000`, `map_id / 10000`).
  * Reconnection strategies were limited or problematic if a specific core server crashed, meaning progress was tightly coupled to the single process.
  * Overloaded the `ch99` channel, creating a monolith bottleneck for all instances rather than distributing them dynamically.

### Core Goals
1. **Elegant & Robust Design:** Permit larger refactors while prioritizing clean, idiomatic Rust.
2. **Graceful Reconnection:** Allow players to reconnect to their dungeon if their connection momentarily drops.
3. **In-Memory Volatility:** Cache instance progress strictly in memory on the specific core server handling the instance. If the core server dies, the instance dies (no database persistence for in-progress dungeon states).
4. **Decouple from `ch99`:** Avoid the legacy hardcoded channel 99 approach.
5. **Deployment Consistency:** Retain the current deployment model where "one `zohar-core` instance can only handle one map template at a time".

---

## Technical Context & Constraints
* **Orchestration:** Managed by Kubernetes/Agones (`GameServer` pods).
* **Current Core Topology:** A `zohar-core` process is bootstrapped to load **one** map template (via `CoreRuntimeConfig::map`).
* **Map Identifiers:** `zohar_domain::MapId` encapsulates map names as `SmolStr` (e.g., `zohar_map_a1`), moving away from purely numeric IDs.
* **Instance Keys:** The codebase already introduces `zohar_sim::types::MapInstanceKey` and `InstanceId`.
  ```rust
  pub enum MapInstanceKind {
      Shared,
      Instanced(InstanceId),
  }
  ```
* **Routing & Resolution:** `MapEndpointResolver` (in `zohar-gamesrv`) handles dynamic lookups of K8s services or Agones GameServers based on K8s labels (`channel=X,map=Y`).

---

## Architectural Deep Dive & Investigation

Given the constraint that one `zohar-core` process runs only **one map template** at a time, we must decide how multiple instances of the *same* template run.

### Evaluated Alternatives

#### 1. Single Bevy ECS World, Partitioned by InstanceId
* **Concept:** Run a single Bevy `App` in the `zohar-core` process. Inject `InstanceId` as a component on every entity. Modify Spatial Hash Grids, Area of Interest (AOI), AI sensors, Combat, and Pathfinding to only interact with entities sharing the same `InstanceId`.
* **Pros:** Least amount of OS threads/processes. Centralized tick loop.
* **Cons:** Exceptionally invasive. Requires rewriting all gameplay logic to filter by `InstanceId`. Extremely high risk of "cross-contamination" bugs where a mob from Instance A attacks a player in Instance B.

#### 2. Process-per-Instance via Agones (Dynamic Pod Provisioning)
* **Concept:** For every dungeon instance created, the Gateway signals Agones via K8s API to allocate a brand new `zohar-core` `GameServer` pod exclusively for that instance.
* **Pros:** Perfect isolation. Matches "one process = one map".
* **Cons:** High latency for instance creation (waiting for container scheduling/startup). Massive resource overhead (OS limits, container overhead). Not viable for hundreds of concurrent micro-dungeons.

#### 3. Multi-App Instance Manager
* **Concept:** Leverage the headless nature of Bevy by spawning a separate Bevy `App` per instance on its own OS thread.
* **Pros:** Complete isolation without refactoring core ECS systems.
* **Cons:** Extremely high resource overhead. Spawning potentially hundreds of OS threads (one per dungeon instance) in a single process creates severe bottlenecking and context-switching contention. It fundamentally breaks down at scale.

#### 4. Single ECS World with Instance Entities (Proposed Solution)
* **Concept:** Move from a "Singleton Map" paradigm to an "Entity Map" paradigm. Instead of Bevy `Resource`s holding global map state (like `MapConfig`, `ActionBuffer`, `SpatialGrid`), these are migrated into `Component`s attached to a central `MapInstance` Entity.
* **Mechanism:**
  * A single Bevy `App` manages all instances of a map template simultaneously.
  * When a new instance is required, a new `MapInstance` Entity is spawned.
  * Every player, mob, and item entity receives an `InInstance(Entity)` component referencing their host instance.
  * Queries join against the `InInstance` component to ensure logic (AOI, Combat, Movement) only applies within the correct boundary.
* **Pros:**
  * **Optimal Performance & Scalability:** Fully leverages Bevy's ECS scheduling to process hundreds of instances in parallel without spawning any new OS threads.
  * **Elegant Architecture:** Strictly follows the Entity-Component-System philosophy, avoiding global singletons in favor of compositional state.
  * **Graceful Reconnects:** Re-connecting routes players back to the single Bevy App, which simply re-attaches them to the correct `Instance` entity in memory.

---

## Proposed Design: Single ECS World with Instance Entities

### 1. ECS Architecture Shift
Currently, `zohar-sim` uses `Resource`s for state like `RuntimeState`, `MapConfig`, and `PortalPollState`. This design will refactor those into `Component`s:
```rust
#[derive(Component)]
pub struct MapInstance {
    pub instance_id: InstanceId,
    pub config: Arc<MapConfig>,
}

#[derive(Component)]
pub struct InInstance(pub Entity);
```
Systems will be updated from accessing global resources to querying `MapInstance` entities:
```rust
// Legacy: fn process_combat(mut state: ResMut<RuntimeState>, ...)
// Proposed:
fn process_combat(
    instances: Query<(Entity, &MapInstance)>,
    mut actors: Query<(&InInstance, &mut Health, ...)>,
) { ... }
```

### 2. Connection & Routing Flow
* **Gateway/Auth:** The player requests to enter a dungeon. The Gateway generates a unique `InstanceId` and issues a session token.
* **K8s Routing:** The player reconnects and is routed to the `zohar-core` pod responsible for `channel=1,map=map41`.
* **GameServer Entry:** The TCP pipeline reaches the `InGame` phase. It sends an `EnterMsg` containing the `InstanceId` to the unified `MapEventSender`.
* **Instance Injection:** The Bevy App intercepts the `EnterMsg`. If the `InstanceId` doesn't have an associated `MapInstance` entity, it spawns one. The player's entity is created and given the `InInstance` component linking it to that instance.

### 3. Graceful Reconnects & In-Memory Volatility
* The dungeon state lives purely as an ECS `MapInstance` entity hierarchy.
* Disconnections simply remove the player from the instance, but the instance continues to tick natively.
* The DB retains the connection token and `InstanceId`. Upon reconnecting, the K8s routing layer sends the client back to the exact pod.
* The unified `MapEventSender` pushes the player back in, and Bevy drops them straight back into the existing `MapInstance` entity.
* **Fate Sharing:** If the `zohar-core` pod crashes, the entire Bevy `App` dies. No instance persistence occurs, inherently satisfying the volatility requirement.

### 4. Instance Lifecycle & Cleanup
* A `CleanupSystem` runs periodically within Bevy. It queries all `MapInstance` entities.
* If a `MapInstance` has zero `InInstance` player children for longer than a designated timeout, the system despawns the `MapInstance` entity and recursively despawns all associated mobs and entities.

### 5. Shared vs. Instanced Map Modes
There is a clear architectural distinction between K8s deployments meant to serve purely public maps and those meant to scale out as dungeon servers.
* **CLI Configuration:** Distinguish the mode at startup via a CLI flag (e.g., `--map-mode shared` vs. `--map-mode instanced`).
* **Instancing Semantics:**
  * **Shared Mode:** The server boots up exactly one `Shared` instance entity. Incoming connections without a specific instance ID are attached to it.
  * **Instanced Mode:** The server does *not* boot a shared instance. It exclusively waits for `EnterMsg` payloads containing an `InstanceId` to dynamically spawn instance entities.
* **Helm/K8s Support:** This configuration propagates to the Helm charts, adding K8s labels to separate the shared map topology from the dynamic dungeon server pool.

### 6. Retiring `ch99` & Magic Numbers
* `MapInstanceKey::Instanced(InstanceId)` replaces the legacy `MapId * 10000` math natively.
* Agones topology seamlessly handles pod allocation without requiring a hardcoded bottleneck channel.

## Summary of Changes Required
1. Introduce `MapInstance` and `InInstance` components to `zohar-sim`.
2. Refactor existing global systems (combat, spatial, AI) to query against instance entities rather than assuming a singleton `RuntimeState`.
3. Modify `zohar-gamesrv` to pass `InstanceId` intent via `EnterMsg`.
4. Update `zohar-sim` inbound handlers to dynamically spawn `MapInstance` entities upon receiving a novel `InstanceId`.
5. Implement a lifecycle ECS system to despawn empty instances.
6. Add CLI flags and Helm chart templates for `--map-mode shared/instanced`.
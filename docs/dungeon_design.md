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

#### 3. Multi-App Instance Manager (Proposed Solution)
* **Concept:** Leverage the headless nature of Bevy. The `zohar-core` process retains its identity as the host for a *single map template* (e.g., `map41`). However, it spawns multiple decoupled Bevy `App` instances across different threads within the same Tokio runtime.
* **Mechanism:**
  * When `zohar-core` boots, it loads the heavy, immutable SQLite content (terrain, navmesh, spawn rules) once into `Arc` pointers (`SharedConfig`, `MapConfig`).
  * The core maintains an `InstanceManager`, essentially a `HashMap<InstanceId, MapEventSender>`.
  * `zohar-gamesrv` handles incoming TCP connections. When a player requests to enter an instance, it routes the message to the corresponding `MapEventSender`.
  * If an instance does not exist, the `InstanceManager` spawns a new thread, initializes a fresh Bevy `App` (sharing the `Arc` configs), runs the map loop, and stores the sender.
* **Pros:**
  * **Perfect Isolation:** Bevy `App`s share no mutable state. No risk of instance crossover.
  * **Zero Code Intrusion:** The core gameplay logic (`combat.rs`, `spatial.rs`) remains completely oblivious to instancing. It acts exactly like a shared map.
  * **Memory Efficient:** Reuses the heavy `Arc` structures. Headless Bevy overhead per instance is minimal (just ECS memory and channels).
  * **Solves Reconnects:** The TCP/Tokio layer tracks `InstanceId` in session state. On drop, the `App` keeps ticking. On reconnect, K8s routes them to the same Pod, and the Gateway reconnects their channel to the existing `App`.
  * **Fate Sharing:** If the `zohar-core` pod crashes, all Bevy `App` threads die instantly. No stale state is left behind in the DB, fulfilling the volatility requirement.

---

## Proposed Design: Multi-App Instance Manager

### 1. Connection & Routing Flow
* **Gateway/Auth:** The player requests to enter a dungeon (e.g., via an NPC interaction). The Gateway generates a unique `InstanceId` (e.g., UUID or DB sequence) and updates the player's session or issues an encrypted token containing the `MapId` and `InstanceId`.
* **K8s Routing:** The player's client reconnects. The K8s routing layer (Agones/Service) routes the client to the `zohar-core` pod responsible for `channel=1,map=map41` (the template pod).
* **GameServer Entry:** In `zohar-gamesrv`, the TCP connection pipeline reaches the `InGame` phase. It parses the connection intent. Instead of unconditionally sending to the *Shared* map channel, it inspects the session context for an `InstanceId`.

### 2. Core Orchestration (`zohar-core` / `InstanceManager`)
* Replace the single `MapEventSender` in `GameContext` with an `Arc<InstanceManager>`.
* The `InstanceManager` provides:
  ```rust
  impl InstanceManager {
      pub async fn get_or_create_instance(&self, key: MapInstanceKey) -> Result<MapEventSender> {
          // 1. Check if map event sender exists in HashMap
          // 2. If yes, return it.
          // 3. If no, call `zohar_sim::spawn_map_runtime` with the provided `key`.
          // 4. Store the new MapEventSender and return it.
      }
  }
  ```
* When `spawn_map_runtime` is called, it receives the `SharedConfig` and `MapConfig`. These contain `Arc` pointers to read-only content, ensuring rapid, low-memory startup of new instances.

### 3. Graceful Reconnects
* The dungeon state lives purely within the Bevy `App` running on its dedicated thread.
* If a player disconnects, they are removed from the map via `LeaveMsg`. The Bevy `App` continues to tick (mobs wander, timers countdown).
* The session manager (`SessionTracker` / Postgres `sessions` table) marks the player as offline but retains their connection token for the TTL.
* Upon reconnecting within the TTL, the player re-authenticates. The `zohar-gamesrv` queries the DB, finds the player was in `MapInstanceKey::instanced(123)`.
* It asks `InstanceManager` for `123`. The instance still exists in memory, so it returns the existing `MapEventSender`.
* The player is re-injected via `EnterMsg` and resumes the dungeon.

### 4. Instance Lifecycle & Cleanup
* The `InstanceManager` needs a way to reap dead instances to prevent memory leaks.
* A watchdog or periodic Bevy system runs inside the instance. If the player count is 0 for longer than `N` minutes, the Bevy `App` signals it is done and terminates its loop.
* The `MapEventSender` channel drops, which the `InstanceManager` detects, subsequently removing the dead instance from its HashMap.
* If the entire server crashes, no persistence logic triggers for the instance itself, fulfilling the "no useless persistence" requirement.

### 5. Shared vs. Instanced Map Modes
While the Multi-App Instance Manager supports both shared and instanced contexts cleanly, there is a clear architectural distinction between maps that are purely public and those that are strictly instanced.
* **CLI Configuration:** We can distinguish the mode at startup via a CLI flag (e.g., `--map-mode shared` vs. `--map-mode instanced`).
* **Instancing Semantics:**
  * **Shared Mode:** The server boots up exactly one `Shared` instance. Incoming connections without a specific instance ID default to this shared instance.
  * **Instanced Mode:** The server does *not* boot a shared instance. Instead, it exclusively waits for `get_or_create_instance` requests containing an `InstanceId` and spins up apps dynamically.
* **Helm/K8s Support:** This configuration propagates to the Helm charts. When seeding K8s/Agones Deployments and GameServers, we can add labels indicating whether the spawned map template acts as a shared world map or an instanced dungeon server. This allows the K8s routing layer to cleanly direct gateway traffic for dungeon creation exclusively to pods configured in `instanced` mode.

### 6. Retiring `ch99` & Magic Numbers
* By using `MapInstanceKey::Instanced(InstanceId)`, we eliminate the need for `MapId * 10000` mathematics.
* The K8s topology handles physical routing based on map *templates* (`channel=1,map=map41`), so we do not need a specialized `ch99` pod. The normal `map41` pod scales to handle both the public `Shared` version and any dynamically spun up `Instanced` versions.

## Summary of Changes Required
1. Introduce an `InstanceManager` in `zohar-gamesrv` or `zohar-core` to manage multiple `MapEventSender`s.
2. Update `GameContext` to utilize the `InstanceManager` rather than a hardcoded single sender.
3. Modify the `InGame` handler (`ingame.rs`) to parse instance keys from the player's session and request the correct channel from the `InstanceManager`.
4. Implement instance termination logic in `zohar_sim` (shutting down the Bevy `App` when empty for a set duration).
5. Ensure `zohar_sim::spawn_map_runtime` properly inherits the `MapInstanceKey` and fully clones the necessary `Arc` configs.
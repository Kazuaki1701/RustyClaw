# Architectural Analysis: Cron Scheduling & Queueing Mechanics
## GeminiClaw (Inngest) vs. RustyClaw (Custom Tokio)

This document provides a comprehensive technical comparison of how scheduled tasks (Crons) are managed, queued, and dispatched under the event-driven serverless architecture of **GeminiClaw (TypeScript/Inngest)** and the self-contained in-process runtime of **RustyClaw (Rust/Tokio)**.

---

## 1. Architectural Paradigms

| Aspect | GeminiClaw (TypeScript) | RustyClaw (Rust) |
| :--- | :--- | :--- |
| **Orchestrator** | **Inngest Engine** (Out-of-Process Server/Cloud) | **Tokio Tasks & Channels** (In-Process Runtime) |
| **Control Flow** | Distributed Event-Driven | Lightweight Actors (LaneRegistry) & Semaphores |
| **Dependencies** | Requires local/cloud Inngest Daemon & HTTP | Pure Rust, zero external database/queue process |

---

## 2. Cron Firing & Queue Assignment Flow

### 2.1. RustyClaw (In-Process Tokio Actor Model)
In RustyClaw, cron scheduling and live task queueing are completely managed inside the single Rust process:

```
+--------------------------------------------------------------------------+
|                            [ RustyClaw Process ]                         |
|                                                                          |
|  +-----------------+                                                     |
|  |   CronService   | (Tokio interval ticks)                              |
|  +--------+--------+                                                     |
|           | (Publish Event)                                              |
|           ▼                                                              |
|  +--------+--------+                                                     |
|  |   MessageBus    |                                                     |
|  +--------+--------+                                                     |
|           | (Route)                                                      |
|           ▼                                                              |
|  +--------+--------+                                                     |
|  |  LaneRegistry   | (Resolves Session ID, e.g. "cron:heartbeat")         |
|  +--------+--------+                                                     |
|           | (Spawns Lane Worker Task dynamically if missing)             |
|           ▼                                                              |
|  +--------+--------+                                                     |
|  |  Lane Worker    | ---> Registers "Waiting" in live Queue State        |
|  +--------+--------+                                                     |
|           | (Acquires Slot)                                              |
|           ▼                                                              |
|  +--------+--------+                                                     |
|  |  gmn_sem (4/4)  | ---> Locks permit, marks "Executing", runs Pipeline  |
|  +-----------------+                                                     |
+--------------------------------------------------------------------------+
```

1. **Passive Schedule Tracking**:
   Cron schedules (defined in `cron.json`) are only tracked as passive future timestamps (queried via `/api/schedule`). No worker thread is allocated, and the task is **not** present in the active queue state (`/api/queue`) before its scheduled time.
2. **Cron Ticks (Instant of Fire)**:
   A lightweight background thread in `CronService` sleeps using `tokio::time::interval`. The exact millisecond the cron fires, it publishes an `IncomingMessage` event to the `MessageBus`.
3. **Dynamic Lane Worker Spawning**:
   The `LaneRegistry` receives the event and checks if a lane worker for this `session_id` (e.g. `"cron:heartbeat"`) is running. If not, it **dynamically spawns a new Tokio lane worker task** on the spot.
4. **Queue Enqueuing & Permit Acquisition**:
   The newly spawned lane worker immediately registers its state in the global `QUEUE_STATE` map with `status: "Waiting"`. It then waits to acquire an execution slot from `gmn_sem` (capacity increased to 4). Once acquired, its status changes to `"Executing"`, the agent pipeline runs, and the lane shuts down upon completion.

---

### 2.2. GeminiClaw (Out-of-Process Inngest Engine)
In GeminiClaw, cron scheduling and throttling are managed entirely outside the core application process by the Inngest Server:

```
+--------------------------+               +--------------------------+
|     [ Inngest Server ]   |               |   [ GeminiClaw App ]     |
|                          |               |     (Bun / Node.js)      |
|  +--------------------+  |               |                          |
|  | Cron Trigger Engine|  |               |                          |
|  +----------+---------+  |               |                          |
|             | (Fires)    |               |                          |
|             ▼            |               |                          |
|  +----------+---------+  |               |                          |
|  | Event Queue (DB)   |  |               |                          |
|  +----------+---------+  |               |                          |
|             | (Throttles) |               |                          |
|             ▼            | (HTTP POST)   |  +--------------------+  |
|  [ Concurrency Check ]---+-------------->|  |  Bun Client Route  |  |
|     (Global limit: 4)    |               |  +---------+----------+  |
|                          |               |            | (Invokes)   |
|                          |               |            ▼             |
|                          |               |  +--------------------+  |
|                          |               |  |  Agent Execution   |  |
|                          |               |  +--------------------+  |
+--------------------------+               +--------------------------+
```

1. **Server-Side Schedule Register**:
   The TypeScript codebase registers crons declaratively via code. The Inngest Engine (either local Dev Server or cloud engine) tracks schedules and maintains the Cron timers.
2. **Server-Side Queueing**:
   When a cron triggers, the Inngest server writes the trigger event to its own **persistent server-side event queue**.
3. **Out-of-Process Concurrency Verification**:
   The Inngest Server verifies the concurrency limits *before* calling the GeminiClaw client:
   * **Session-Level**: `scope: 'fn', key: 'event.data.sessionId', limit: 1`
   * **Account-Level**: `scope: 'account', key: '"global"', limit: 4`
   If the global limit (4) is exhausted, the event **remains waiting in Inngest's server-side queue**.
4. **On-Demand Client Invocation**:
   Once a parallel slot becomes available, the Inngest Server issues an **HTTP POST request** to the GeminiClaw client web server (Bun/Node.js), invoking the specific agent execution step.

---

## 3. Deep-Dive Architectural Comparison

| Comparison Dimension | GeminiClaw (Inngest) | RustyClaw (Custom Tokio) |
| :--- | :--- | :--- |
| **Trigger Agent** | External Inngest Server | Internal `CronService` thread loop |
| **Queue Storage** | Persistent DB inside Inngest | Thread-safe, in-memory `QUEUE_STATE` map |
| **Throttling Verification** | **Out-of-Process**: Throttled in Inngest *prior* to client invocation. | **In-Process**: Lane worker spawns immediately, then blocks internally on `gmn_sem`. |
| **Crash Safety** | **Lossless**: If the client crashes, Inngest preserves the queue and retries. | **Volatile (In-Memory)**: If the daemon crashes, active in-memory queue states are lost. |
| **Resource Overhead** | Requires additional Node/Go process for Inngest, plus SQLite/Redis DB. | **Zero Overhead**: Embedded directly inside the single Compiled Rust Binary. |
| **Network Latency** | High (incurs loopback HTTP requests and JSON serialization). | **Ultra Low** (instantaneous memory channel/pointer passing). |

---

## 4. Engineering Trade-offs & Parity Summary

### 4.1. Why RustyClaw Uses In-Process Tokio
For a resource-constrained hardware environment like a **Raspberry Pi 4 (8GB RAM)**, hosting external orchestration layers (like Inngest or PM2) introduces substantial CPU/memory bloat and network loopback overhead. 

By building a custom `LaneRegistry` and `MessageBus` on top of **Tokio's lock-free MPSC channels and Async Semaphores**, RustyClaw achieves:
* **Extreme Memory Efficiency**: The entire gateway daemon, including HTTP server, cron service, and parallel queues, fits under **30MB RAM** at idle.
* **Complete Self-Containment**: A single, dependency-free binary can be easily deployed via systemd.
* **Dynamic Transparency**: By letting lane workers spawn immediately and wait *inside* the process on `gmn_sem`, the dashboard can query `QUEUE_STATE` and present a high-fidelity visual grid of all `Waiting` and `Executing` tasks in real time.

### 4.2. Concurrency Safety Parity
With **Phase 2 (File Locking & Semaphore Scaling to 4)** successfully implemented:
* RustyClaw has bridged the gap with GeminiClaw's parallel execution scaling.
* It safely runs up to 4 concurrent user/cron/heartbeat turns across separate sessions.
* File-level race conditions are eliminated using the in-process async **`RwLock` Path Lock Manager**, maintaining robust integrity under parallel workloads.

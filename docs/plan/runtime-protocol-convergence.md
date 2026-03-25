# Runtime Protocol 收斂草稿

## 目前進度

這份文檔目前是純草稿。

目前已實作：

- `runtime_protocol` 已有一套可用的 read-side view model：
  - `RuntimeHealthView`
  - `ManagedWorkspaceView`
  - `ThreadStateView`
  - `ArchivedThreadView`
  - `WorkingSessionSummaryView`
  - `WorkingSessionRecordView`
- management API 已對外提供這些 query surface 與 typed SSE event：
  - `GET /api/setup`
  - `GET /api/runtime-health`
  - `GET /api/threads`
  - `GET /api/threads/:thread_key/transcript`
  - `GET /api/threads/:thread_key/sessions`
  - `GET /api/threads/:thread_key/sessions/:session_id/records`
  - `GET /api/workspaces`
  - `GET /api/archived-threads`
  - `GET /api/events`
- management API 已有一批實際 control route：
  - workspace pick/add
  - repair session binding
  - open workspace
  - repair runtime
  - `launch-hcodex-*`
  - archive / restore
  - managed Codex preference / refresh / build
- Telegram adapter 已有一批實際 command surface：
  - `/new_session`
  - `/repair_session`
  - `/launch ...`
  - `/execution_mode`
  - `/sessions`
  - `/session_log`
  - `/stop`
  - `/plan_mode`
  - `/default_mode`
- observer / interaction 已有一條 shared event lane：
  - `RuntimeInteractionEvent::RequestUserInput`
  - `RuntimeInteractionEvent::RequestResolved`
  - `RuntimeInteractionEvent::TurnCompleted`

目前尚未完成：

- control action 尚未有一套和 view / event 同級的 Rust protocol 型別
- 很多 capability 仍只有 route 名或 slash command 名，缺少 canonical action object
- interaction event 仍是平行語言，尚未正式併入同一份 runtime protocol 契約
- collaboration mode 尚未進入 management / protocol public view
- `/new_session`、`/stop` 這類能力尚未形成 management API / Telegram / protocol 三面一致的收斂
- 部分 capability 只存在 local app API 或 Telegram surface，尚未成為 transport-facing public contract

## 問題

現在 `threadBridge` 的主要缺口不是缺 view，也不是缺路由，而是：

- view / query 已開始 protocol 化
- 但 control / interaction 仍有相當一部分停留在 surface-driven

具體表現是：

- management API route 名、Telegram slash command 名、shared service method 名，仍常常代表同一件事
- `runtime_protocol.rs` 幾乎只承載 read-side view 與 top-level event，沒有對等的 control action vocabulary
- `RuntimeInteractionEvent` 已是 shared event，但仍平行存在於 `runtime_protocol` 之外
- collaboration mode、interrupt、fresh session 這類能力，已存在於代碼，卻還沒有完整 public protocol surface

結果就是：

- 文檔要同時描述 route、command、handler 三套語言
- 新增一個 surface 時，容易再複製一套 naming
- 很難明確回答「這是一個 Telegram 功能，還是一個 runtime capability」

## 定位

這份文檔不是新的主規格。

它的角色是：

- `runtime-protocol.md` 的實施 / 收斂草稿
- 描述如何把既有代碼中的 route、slash command、shared service、interaction event 收斂到同一份 protocol 語義

它處理的是：

- rollout phase
- workstream 切分
- 哪些 capability 先收斂
- 哪些 Rust 模組要改
- 怎麼判定這個 protocol 收斂任務完成

它明確不處理：

- `binding_status` / `run_status` 等 canonical state semantics 本身
- Telegram delivery 主規格
- 第二個 adapter 的產品化
- capability bridge 的長期設計細節

## 核心原則

- 先收斂 naming，再收斂 transport。
- 先收斂 control / interaction，再補更多 query。
- 先讓既有 capability 對齊到單一 protocol action，不先擴更多功能面。
- protocol 的 source of truth 應先落在 shared Rust 型別，而不是只留在 plan 文檔。
- management API 與 Telegram 都應視為 adapter surface，而不是 capability owner。

## 主體規格

### 1. 先承認目前其實有三層語言

目前同一個 capability 常同時有三種表示：

- protocol / plan 裡的語義名稱
- HTTP route 名稱
- Telegram slash command 名稱

例如：

- execution mode
  - protocol: `set_workspace_execution_mode`
  - HTTP: `PUT /api/workspaces/:thread_key/execution-mode`
  - Telegram: `/execution_mode`
- local launch
  - protocol: `launch_local_session`
  - HTTP: `launch-hcodex-new|continue-current|resume`
  - Telegram: `/launch new|current|resume`

這代表目前真正缺的不是更多 route，而是少一層被代碼承認的 canonical action model。

### 2. 收斂工作分四條 workstream

#### Workstream A: Control Action Model

先在 shared Rust 層新增正式的 control action vocabulary。

v1 應至少覆蓋：

- `add_workspace`
- `pick_workspace_and_add_binding`
- `start_fresh_session`
- `repair_session_binding`
- `set_workspace_execution_mode`
- `set_thread_collaboration_mode`
- `launch_local_session`
- `interrupt_running_turn`
- `adopt_tui_session`
- `reject_tui_session`
- `archive_thread`
- `restore_thread`
- `repair_workspace_runtime`
- `reconcile_runtime_owner`
- `set_managed_codex_preference`
- `refresh_managed_codex_cache`
- `build_managed_codex_source`
- `set_managed_codex_build_defaults`

建議新增的 shared 型別至少包括：

- `RuntimeControlAction`
- `RuntimeControlActionKind`
- per-action request payload
- per-action response payload

這一層的目標不是立刻換 transport，而是讓 management API 與 Telegram 都呼叫同一個 canonical action vocabulary。

#### Workstream B: Interaction Protocol

把目前平行存在的 `RuntimeInteractionEvent` 收回 protocol 主線。

v1 至少應固定：

- `request_user_input_requested`
- `request_user_input_resolved`
- `plan_follow_up_requested`

這裡不一定要硬併進 SSE，但至少要在 protocol 文檔與 shared Rust 型別上，和 `RuntimeEventKind` 屬於同一個 vocabulary family。

近期最重要的是先把：

- `RequestUserInput`
- `RequestResolved`
- `TurnCompleted(has_plan=true)`

這三種事件重新表達成 protocol-facing 名稱，而不是繼續只留在 Telegram interaction bridge 內部語言。

#### Workstream C: Surface Parity

把已存在能力分成三類：

- 已經有 management API + Telegram + shared protocol
- 已經有 shared protocol + 單一 adapter
- 只有 adapter surface，尚未 protocol 化

近期應優先補齊這幾個 capability：

- `start_fresh_session`
  - 目前有 Telegram command 與 shared service
  - 缺 management API public surface
- `set_thread_collaboration_mode`
  - 目前有 Telegram command 與 repository persistence
  - 缺 management/API/protocol public view
- `interrupt_running_turn`
  - 目前有 Telegram command 與 Codex client call
  - 缺 management/API/protocol public surface

這三個能力是最值得優先處理的，因為它們最能驗證「protocol 先於 adapter」是否真的成立。

#### Workstream D: Public Vocabulary Cleanup

把 user-facing 與 docs-facing 名稱固定下來。

至少要收斂：

- `new_session` vs `start_fresh_session`
- `launch current` vs `continue_current`
- `plan_mode/default_mode` vs `set_thread_collaboration_mode`
- `stop` vs `interrupt_running_turn`

原則是：

- protocol 名可以和 slash command 不同
- 但 mapping 必須明確、穩定、可文檔化
- 不應讓不同 surface 各自延伸出新的 capability 名稱

### 3. 分階段落地

#### Phase 1: 補齊 shared control 型別

目標：

- 在 shared Rust 層新增 canonical control action model
- 不先變更外部行為

建議落點：

- 新增或擴充 `rust/src/runtime_protocol.rs`
- 視情況新增 `rust/src/runtime_actions.rs`
- management API 與 Telegram 先做最薄 mapping

完成標誌：

- 至少三個 action 已不再直接以 route/command 為主語義
  - `set_workspace_execution_mode`
  - `launch_local_session`
  - `repair_session_binding`

#### Phase 2: 補 interaction protocol

目標：

- 把 `RuntimeInteractionEvent` 明確掛回 protocol vocabulary
- 固定 interaction event naming

建議落點：

- `rust/src/runtime_interaction.rs`
- `rust/src/runtime_protocol.rs`
- `rust/src/app_server_observer.rs`
- `rust/src/telegram_runtime/interaction_bridge.rs`

完成標誌：

- interaction event 不再只是 Telegram bridge 專用語言
- 文檔可直接回答哪個 interaction event 屬於 public runtime contract

#### Phase 3: 補 surface parity

目標：

- 補齊目前缺 management/API surface 的 shared capability

優先順序：

1. `start_fresh_session`
2. `set_thread_collaboration_mode`
3. `interrupt_running_turn`

完成標誌：

- 這三個 capability 都有：
  - canonical action 名
  - shared code path
  - 至少一個 transport-facing public surface
  - 清楚的 README / plan mapping

#### Phase 4: 補 event / observability coverage

目標：

- 決定 control action 結果是否也應進 SSE / observability payload
- 決定 interaction event 是否需要 public stream surface

這一階段不一定要做更細的增量 event，但要把責任邊界固定。

### 4. 建議的代碼切面

這個任務主要會碰到：

- [runtime_protocol.rs](/Volumes/Data/Github/threadBridge/rust/src/runtime_protocol.rs)
  - canonical view / event / action vocabulary
- [management_api.rs](/Volumes/Data/Github/threadBridge/rust/src/management_api.rs)
  - HTTP route -> canonical action mapping
- [runtime_control.rs](/Volumes/Data/Github/threadBridge/rust/src/runtime_control.rs)
  - shared service / action execution
- [telegram_runtime/thread_flow.rs](/Volumes/Data/Github/threadBridge/rust/src/telegram_runtime/thread_flow.rs)
  - slash command -> canonical action mapping
- [runtime_interaction.rs](/Volumes/Data/Github/threadBridge/rust/src/runtime_interaction.rs)
  - interaction vocabulary
- [app_server_observer.rs](/Volumes/Data/Github/threadBridge/rust/src/app_server_observer.rs)
  - observer -> interaction event mapping

### 5. 驗收標準

這個任務至少要達到下面幾件事，才算 protocol 收斂開始成立：

- 可以為每個重要 capability 先說出 canonical action 名，再說出各 adapter surface
- `runtime_protocol.rs` 不再只有 read model，也開始承載 control / interaction vocabulary
- `new_session`、`stop`、`plan_mode/default_mode` 不再只是 Telegram-first 功能
- management API、Telegram、plan 文檔不再各自重複發明同一能力的名字

## 與其他計劃的關係

- [runtime-protocol.md](/Volumes/Data/Github/threadBridge/docs/plan/runtime-protocol.md)
  - 這份文檔是它的 rollout / convergence 草稿，不取代它的主規格地位
- [telegram-adapter-migration.md](/Volumes/Data/Github/threadBridge/docs/plan/telegram-adapter-migration.md)
  - Telegram 何時才算退回 protocol consumer，會直接依賴這份收斂計畫
- [session-lifecycle.md](/Volumes/Data/Github/threadBridge/docs/plan/session-lifecycle.md)
  - `start_fresh_session` / `repair_session_binding` / launch surface 的 protocol naming 需要和它對齊
- [codex-busy-input-gate.md](/Volumes/Data/Github/threadBridge/docs/plan/codex-busy-input-gate.md)
  - `/stop` 是否正式收斂成 `interrupt_running_turn`，會影響這份 plan
- [owner-runtime-contract.md](/Volumes/Data/Github/threadBridge/docs/plan/owner-runtime-contract.md)
  - owner / adapter / shared control 的邊界，會決定哪些 action 應屬於 protocol 主線

## 開放問題

- `RuntimeControlAction` 應直接放進 `runtime_protocol.rs`，還是拆成獨立模組？
- interaction event 應和 `RuntimeEventKind` 共用同一個 enum family，還是保持另一條 typed stream？
- `start_fresh_session` 是否需要補 management API route，還是只先收斂 shared action vocabulary？
- collaboration mode 是否應進入 `ManagedWorkspaceView` / `ThreadStateView`，還是暫時只作 control surface？
- `/stop` 的對等 management/API surface 是否應該先補，還是先只補 protocol action 和 shared dispatcher？
- control action 結果是否需要進 typed SSE，還是目前仍以 view diff 為主？

## 建議的下一步

1. 先在 `runtime-protocol` 主規格確認這份 rollout 草稿採用的 canonical action 名稱。
2. 先做一個最小 shared Rust 型別切片，至少覆蓋 execution mode、launch、repair session 三個 action。
3. 再決定 interaction vocabulary 是要併入 `RuntimeEventKind`，還是保持獨立但同級的 protocol 型別。
4. 補一輪文檔同步，把 `telegram-adapter-migration`、`session-lifecycle`、`codex-busy-input-gate` 的 action naming 引到同一套語言。
5. 最後再開始補缺 management/API surface 的 capability，而不是先擴更多新功能。

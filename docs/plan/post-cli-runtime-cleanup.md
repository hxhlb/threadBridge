# Post-CLI Runtime Cleanup 草稿

## 目前進度

這份文檔目前仍是純草稿，尚未開始正式重構。

目前已確認：

- `threadBridge` 的 canonical runtime 已不是舊 CLI / hook 模型，而是 `desktop runtime owner + shared app-server + owner-canonical runtime health`
- 舊 CLI viewer、attach intent plumbing、`SessionAttachmentState` 正式欄位已被移除
- 但在 `workspace_status`、`runtime_protocol`、`hcodex` 啟動鏈、以及少量 repository compatibility 上，仍可看見 CLI / handoff 時代延續下來的語義與中間 shim

目前尚未完成：

- 尚未把這些遺留分成「應保留的 local TUI core」與「應被移除或重命名的過渡語義」
- 尚未把 `handoff_readiness`、`shared-runtime` 狀態面、`local-session.json`、`hcodex-ws-bridge` 等結構收斂成更符合 owner-managed app-server runtime 的模型
- 尚未為這些重構建立正式的 vocabulary migration 與 artifact migration 計畫

## 問題

目前 `threadBridge` 的 runtime 核心已經完成了一次大方向轉換：

- 正式 owner 已是 desktop runtime
- Telegram 已不再是 runtime owner
- workspace canonical backend 已是 shared `codex app-server`

但架構上仍有一批 read-side / control-side / launch-side 的殘留，仍在使用比較接近舊 CLI 時代的心智模型。

這些殘留不一定會立刻造成錯誤，但會持續帶來幾個問題：

- 文件和代碼會同時描述「owner-managed runtime」與「local/bot handoff」兩套語言
- 管理面與狀態面容易把 observation surface 誤認成 runtime authority
- `hcodex` 啟動鏈看起來比實際需要更複雜，讓 transport shape 像是 workaround 疊加
- 未來若要繼續做 adapter/core 邊界收斂，會先被舊命名與舊狀態模型拖住

所以這份文檔記錄的不是單一 bug，而是：

- 在 shared app-server / desktop owner 方案已成立後，哪些架構層仍保留 CLI 時代遺留語義

## 目前觀察到的遺留類型

### 1. 狀態模型仍帶有 CLI / local ownership 語義

最明顯的是 [`workspace_status.rs`](/Volumes/Data/Github/threadBridge/rust/src/workspace_status.rs)。

目前狀態面仍然使用：

- `.threadbridge/state/shared-runtime`
- `SessionStatusOwner::{Local, Bot}`
- `local-session.json`
- `LocalSessionClaim`

具體可見：

- [`workspace_status.rs`](/Volumes/Data/Github/threadBridge/rust/src/workspace_status.rs#L15)
- [`workspace_status.rs`](/Volumes/Data/Github/threadBridge/rust/src/workspace_status.rs#L23)
- [`workspace_status.rs`](/Volumes/Data/Github/threadBridge/rust/src/workspace_status.rs#L91)

這些命名與資料模型更像是在描述：

- 本地 TUI / CLI 與 bot 誰持有 session

而不是在描述：

- desktop owner 管理下的 workspace runtime activity / observation surface

這表示 CLI 時代的 ownership vocabulary 雖然已不再是正式架構，但在 read-side status surface 上仍未完全退場。

### 2. 管理面仍以 `handoff_readiness` 作為主語義

[`runtime_protocol.rs`](/Volumes/Data/Github/threadBridge/rust/src/runtime_protocol.rs) 到現在仍把 runtime health 的核心對外欄位命名成 `handoff_readiness`，並用它統計 ready/degraded/unavailable workspace，也用它生成 recovery hint。

具體可見：

- [`runtime_protocol.rs`](/Volumes/Data/Github/threadBridge/rust/src/runtime_protocol.rs#L29)
- [`runtime_protocol.rs`](/Volumes/Data/Github/threadBridge/rust/src/runtime_protocol.rs#L309)
- [`runtime_protocol.rs`](/Volumes/Data/Github/threadBridge/rust/src/runtime_protocol.rs#L720)
- [`runtime_protocol.rs`](/Volumes/Data/Github/threadBridge/rust/src/runtime_protocol.rs#L808)

在 today 的模型裡，這個詞已經有些語義偏移，因為：

- authority 是 desktop owner heartbeat
- canonical backend 是 shared app-server
- `TUI proxy` 是 `hcodex` 專用 bridge，不是 handoff owner

所以這裡比較像是從「CLI handoff」重寫成「mirror/readiness」時，只換了一部分邏輯，沒有完成 vocabulary migration。

### 3. `hcodex` 啟動鏈仍保留多層 transition shim

目前 `hcodex` 的啟動不是單純「找到 canonical ws endpoint 然後連上」，而是：

1. `ensure-hcodex-runtime`
2. `resolve_hcodex_launch.py`
3. 若 URL 帶 path，再啟 `hcodex-ws-bridge`
4. 最後 `run-hcodex-session`

具體可見：

- [`workspace.rs`](/Volumes/Data/Github/threadBridge/rust/src/workspace.rs#L69)
- [`workspace.rs`](/Volumes/Data/Github/threadBridge/rust/src/workspace.rs#L151)
- [`workspace.rs`](/Volumes/Data/Github/threadBridge/rust/src/workspace.rs#L183)
- [`workspace.rs`](/Volumes/Data/Github/threadBridge/rust/src/workspace.rs#L210)
- [`resolve_hcodex_launch.py`](/Volumes/Data/Github/threadBridge/tools/resolve_hcodex_launch.py#L107)
- [`hcodex_ws_bridge.rs`](/Volumes/Data/Github/threadBridge/rust/src/hcodex_ws_bridge.rs#L66)

這個形狀本身不一定錯，但它很像：

- shared app-server ws 遷移完成後，為了兼容既有 `hcodex` 接入與 proxy path sideband，而保留下來的一串過渡式 shim

它的問題不是功能不能用，而是：

- transport shape 不夠 canonical
- `hcodex` 對真正 runtime contract 的依賴被包在多層 launcher / resolver / bridge 裡

### 4. repository 仍保留少量 attachment / handoff compatibility 尾巴

repository 主模型大致已切到現在的 session binding，但仍保留了一個明確的 legacy compatibility 測試：

- [`repository.rs`](/Volumes/Data/Github/threadBridge/rust/src/repository.rs#L1533)

它仍接受舊欄位：

- `attachment_state = "local_handoff"`  
  [`repository.rs`](/Volumes/Data/Github/threadBridge/rust/src/repository.rs#L1548)

這說明 attachment / handoff 語義已不是正式模型，但其資料痕跡仍是 today deserialization compatibility 的一部分。

## Git 歷史驗證

這不是純粹從現況倒推的猜測。git 提交順序本身就支持「主模型已切換，但部分邊界尚未清理完畢」這個判斷。

### 1. 舊 CLI 同步與 handoff 模型先存在

較早的提交包括：

- `d12a85d` `feat(workspace): 實作本地 Codex CLI 與 Telegram 狀態同步機制`
- `1e0a7b0` `feat(threadbridge): add exclusive cli handoff attach`
- `ca9ea28` `feat(threadbridge): add managed hcodex mirror handoff`

這代表 CLI / hook / handoff 世界觀先形成，再慢慢遷移。

### 2. shared app-server runtime 之後才落地

`9f60e40` `feat(threadbridge): add shared app-server runtime foundation` 才是 shared app-server runtime foundation 成形的關鍵點。

接著 `36a7bfb` `refactor(threadbridge): remove codex sync bootstrap layer` 移除了 `tools/codex_sync.py` 這類早期 bootstrap layer。

這表示：

- canonical backend 已切到 app-server
- 但 CLI 時代的一些語義層並沒有在同一輪裡一起清乾淨

### 3. mirror/readiness 的文檔語義之後才補上

`122a504` `feat(threadbridge): replace cli model with local mirror readiness` 明確把文檔和部分狀態語言改寫成 mirror/readiness。

但今天仍可看到：

- `workspace_status` 仍用 `Local/Bot`
- `runtime_protocol` 仍用 `handoff_readiness`

這說明當時更像是：

- 先把主模型往新語義改
- 再逐步收尾舊 vocabulary 與舊狀態面

### 4. 一部分遺留已被正式移除

以下提交代表並不是所有舊模型都還在：

- `d786026` `feat(threadbridge): remove viewer runtime leftovers`
- `4fcdad4` `refactor(threadbridge): drop legacy attach intent plumbing`
- `1c0838d` `refactor(repository): 移除已棄用的 SessionAttachmentState 列舉與欄位`

因此更準確的說法是：

- CLI 時代的顯性元件已清掉一批
- 但 vocabulary、status surface、launch transport shape 仍留有 transition debt

## 定位

這份文檔只處理：

- shared app-server / desktop owner 模型成立之後，剩餘的 post-CLI vocabulary、status surface、launch surface 清理

這份文檔不處理：

- mirror intake observer 化細節  
  這由 [app-server-ws-mirror-observer.md](/Volumes/Data/Github/threadBridge/docs/plan/app-server-ws-mirror-observer.md) 處理
- Telegram adapter 的完整抽象化路線  
  這由 [runtime-transport-abstraction.md](/Volumes/Data/Github/threadBridge/docs/plan/runtime-transport-abstraction.md) 與 [telegram-adapter-migration.md](/Volumes/Data/Github/threadBridge/docs/plan/telegram-adapter-migration.md) 處理
- working session observability 的最終 UI / API 形狀
- `TUI proxy` 是否完全刪除

換句話說，這份文檔處理的是：

- app-server / desktop owner 模型已成立之後，剩下哪些詞彙、artifact 與 launch path 還在替舊模型背債

## 建議的收斂方向

### 1. 將狀態面從 `Local/Bot` 收斂到更中性的 runtime observation vocabulary

較合理的方向應是：

- 把 `.threadbridge/state/shared-runtime/*` 重新描述為 observation / activity surface，而不是讓名稱看起來像 canonical runtime 本體
- 讓 `SessionStatusOwner` 不再承載舊 local-vs-bot ownership 世界觀
- 重新評估 `local-session.json` 是否應保留、改名，或被更一般的 local TUI activity record 取代

### 2. 將 `handoff_readiness` 收斂到 owner-managed runtime readiness vocabulary

較合理的方向應是：

- 對外 view / recovery hint 用更符合 today 模型的詞
- 將「pending adoption」與「runtime degraded」拆成更清楚的不同類型狀態
- 讓 management surface 不再把 workspace readiness 建立在 handoff 概念上

### 3. 將 `hcodex` 啟動 contract 收斂到更直接的 workspace runtime contract

較合理的方向應是：

- 明確定義 `hcodex` 應依賴的 canonical launch contract
- 重新檢視 `/thread/<thread_key>` path sideband 是否仍必要
- 重新檢視 `hcodex-ws-bridge` 是否只是 transition shim，或是否應被更正式的 endpoint contract 取代

### 4. 將 compatibility 邊界固定在明確的 migration policy

例如：

- repository 對 legacy serialized fields 的兼容要保留多久
- 什麼時候可以停止接受 `attachment_state`
- 哪些 artifact 仍需 best-effort 讀舊格式，哪些應直接拒絕

## 與其他計劃的關係

- [session-level-mirror-and-readiness.md](/Volumes/Data/Github/threadBridge/docs/plan/session-level-mirror-and-readiness.md)
  - 描述現行 shared runtime + mirror + adoption 模型
- [app-server-ws-mirror-observer.md](/Volumes/Data/Github/threadBridge/docs/plan/app-server-ws-mirror-observer.md)
  - 只處理 mirror intake boundary，屬於本文件的一個子債務
- [runtime-protocol.md](/Volumes/Data/Github/threadBridge/docs/plan/runtime-protocol.md)
  - 後續若要改 `handoff_readiness` vocabulary，這份文件需要同步更新
- [workspace-runtime-surface.md](/Volumes/Data/Github/threadBridge/docs/plan/workspace-runtime-surface.md)
  - 後續若要改 `.threadbridge/state/shared-runtime/*` 的定位或命名，這份文件需要同步更新
- [runtime-transport-abstraction.md](/Volumes/Data/Github/threadBridge/docs/plan/runtime-transport-abstraction.md)
  - post-CLI 清理是 transport/core 邊界更乾淨之前的前置收尾工作之一

## 開放問題

1. `workspace_status` 應該被視為 owner-canonical health 的附屬 observation surface，還是未來某種 session activity registry？
2. `handoff_readiness` 應該直接改名，還是先在對外 protocol 上引入 alias，再逐步移除舊詞？
3. `hcodex-ws-bridge` 是短期兼容層，還是 `codex --remote` 現形狀下不可避免的長期組件？
4. `local-session.json` 是否真的還有獨立存在的必要，還是可被更一般的 session activity record 取代？

## 建議的下一步

1. 先把 `mirror observer` 與 broader post-CLI cleanup 分成兩條獨立重構線，不再混成同一件事。
2. 盤點 `workspace_status`、`runtime_protocol`、`workspace-runtime-surface` 三份規格與實作中仍使用的舊 vocabulary，列出可逐步替換的對照表。
3. 釐清 `hcodex` canonical launch contract，再決定是否保留 `/thread/<thread_key>` sideband 與 `hcodex-ws-bridge`。
4. 在 repository 層定一個明確的 legacy field compatibility policy，避免 attachment/handoff 類歷史欄位永久滯留。

# Hcodex Responsibility Matrix 草稿

## 目前進度

這份文檔目前已進入「部分落地」。

目前已成立的部分：

- `hcodex` 已不再是 runtime owner，而是 owner-managed local entrypoint
- `hcodex` 的 launch contract 已明確分離出 ingress launch URL 與 upstream Codex `--remote` endpoint
- `run-hcodex-session` 已正式接手本地 `codex --remote` child 的 spawn
- `workspace_status` 已有 local launcher started / ended write-path
- dirty 版本的 signal forwarding / cleanup 已先補上，避免 stale TUI busy state 長期鎖死

目前尚未完全收斂的部分：

- `hcodex` core 與 ingress / observer / adoption glue 的邊界仍未完全做成獨立模組
- `reconcile` 仍部分依賴 read-side stale recovery，而不是完全由 supervisor write-path 保證
- `hcodex` 曾歷史性背負的責任尚未正式分成「必留 / 過渡保留 / 應移出 core」

## 問題

`hcodex` 這個詞在歷史上背過太多責任。

從 git 歷史看，它至少經歷過：

- CLI shell wrapper / `codex_sync.py`
- managed handoff / attach
- shared app-server ws launch
- local websocket bridge
- ingress relay / live request injection
- local child lifecycle supervision

如果不把這些責任明確分類，後續 clean refactor 很容易犯兩種相反的錯：

- 把應該保留在 `hcodex` core 的強生命週期責任一起做薄
- 或把本來只是過渡結構的 ingress / adapter glue 永久黏在 `hcodex` core 裡

所以這份文檔的目的不是重複歷史，而是明確回答：

- today 的 `hcodex` core 應該保留什麼
- 哪些東西 today 可以暫時留在 `hcodex` 周邊，但不應成為長期核心
- 哪些責任應該離開 `hcodex` core

## 定位

這份文檔是 `hcodex` 的責任矩陣草稿。

它處理：

- `hcodex` core 的長期最小責任集合
- 歷史責任與 today 責任之間的對照
- 哪些責任屬於過渡保留，哪些責任應該移到其他層

它不處理：

- `launch_ticket` / websocket frame 細節
- stale busy state 的完整狀態機
- Telegram UI / callback / slash command 產品面
- observer 的完整 projection vocabulary

## 核心結論

today 的 `hcodex` core 只應明確保留 4 個核心責任：

- `launch`
- `bridge`
- `supervise`
- `reconcile`

這 4 個詞的意思是：

### 1. `launch`

- 選定 thread / session target
- resolve launch URL
- 決定本次啟動是否需要 local compatibility bridge

### 2. `bridge`

- 把 threadBridge ingress launch URL 轉成 upstream Codex 可接受的 bare websocket endpoint
- 在本地短暫 reconnect 視窗內保住同一條 upstream session

### 3. `supervise`

- spawn `codex --remote`
- 持有 child pid / command / launcher ownership
- signal forwarding
- wait / teardown

### 4. `reconcile`

- child 結束後收尾 local claim
- 收斂 session snapshot / busy state / aggregate status
- 補 adoption pending 等本地 runtime state mutation

一句話總結：

- `hcodex` today 應該是 `local Codex process owner + launch/bridge adapter`

## 責任矩陣

### A. 必須保留在 `hcodex` core

這些責任如果離開 `hcodex` core，系統就會失去它最重要的穩定性來源。

#### 1. Launch orchestration

- thread / session target selection
- ingress launch resolution
- local bridge decision

原因：

- upstream Codex 只接受 remote endpoint，不知道 threadBridge 的 launch contract
- desktop owner 也不是每次本地 child spawn 的直接執行者

#### 2. Local transport compatibility boundary

- path / query / `launch_ticket` 保留
- bare websocket endpoint adaptation
- 本地 reconnect replay

原因：

- 這是 threadBridge launch contract 與 upstream Codex remote contract 的交界
- 只要 contract 仍未統一，這層不能憑感覺刪掉

#### 3. Local child lifecycle supervision

- spawn
- child identity tracking
- signal forwarding
- wait / forced kill / teardown

原因：

- git 歷史顯示，真正讓舊模型穩的是強生命週期閉環
- 這一責任如果拆散，就會重新出現 stale local lifecycle state

#### 4. Final local state reconciliation

- launcher ended write-path
- local claim cleanup
- session idle 收尾
- adoption pending mutation
- stale fallback 的正式 write-side收斂責任

原因：

- `supervise` 只管進程本身
- `reconcile` 才負責把進程結束後留下的 workspace-local state 收回一致

### B. 可以暫時保留在 `hcodex` 周邊，但不應成為長期 core

這些責任 today 可以與 `hcodex` 同區域存在，但不應再被視為 `hcodex` 的本質定義。

#### 1. Ingress relay

- websocket ingress listener / relay
- launch ticket consume

原因：

- today 仍和 `hcodex` launch path 緊耦合
- 但它更接近 runtime ingress capability，而不是 local child lifecycle 核心

#### 2. Live request-response injection

- `request_user_input` response 注入
- live daemon request forwarding

原因：

- 這是 ingress/runtime interaction 邊界的一部分
- 不屬於 `launch / bridge / supervise / reconcile` 這 4 個核心詞

#### 3. Adoption glue

- session 結束後的 adoption pending 連接
- local/TUI takeover 的過渡語義

原因：

- today 仍需要
- 但長期應更明確歸到 runtime state / control semantics，而不是 `hcodex` core 自身定義

### C. 應離開 `hcodex` core

這些責任若還留在 `hcodex` core，代表責任邊界沒有切乾淨。

#### 1. Runtime owner authority

- machine-level health authority
- workspace runtime ensure / repair authority
- owner reconcile loop

這些屬於 `desktop runtime owner`，不是 `hcodex`。

#### 2. Canonical observer / mirror projection

- preview / final / process transcript projection
- adapter-neutral interaction events
- canonical observability feed

這些屬於 observer runtime，不是 `hcodex` core。

#### 3. Telegram adapter UI

- Telegram message send / edit
- callback UX
- slash command surface

這些屬於 Telegram adapter。

#### 4. 舊 CLI hooks / handoff vocabulary

- `codex_sync.py`
- shell hooks / notify
- viewer handoff
- attach intent 舊語義

這些只應作為歷史參考，不應回流進 today 的 `hcodex` core。

## 與 git 歷史的對齊

這份責任矩陣是直接從 git 歷史提煉出來的。

### 1. CLI 架構的優點

代表提交：

- `d93d9f0`
- `501763d`

它們證明：

- 舊模型真正值得保留的是本地 lifecycle ownership
- 不是 shell / Python 本身

### 2. app-server ws 架構的優點

代表提交：

- `9f60e40`
- `996fe0e`
- `fa06570`
- `a0936bf`
- `00d3814`

它們證明：

- 新模型真正值得保留的是 transport / session contract clarity
- 不是把 `hcodex` 做成薄 wrapper

### 3. 轉折點

代表提交：

- `88d4bb1`

它證明：

- 本地 child lifecycle 的責任從 shell 轉交給 Rust `run-hcodex-session`
- 所以 today 的 clean refactor 必須正式承認：`hcodex` 是 local process owner

## 對實作的約束

後續任何 clean refactor，只要碰 `hcodex` core，都應先回答下面 3 個問題。

### 1. 這個改動是否還保留 `launch / bridge / supervise / reconcile` 四責任？

如果不是，應視為高風險改動。

### 2. 這個改動是在抽離周邊責任，還是在做薄核心責任？

- 抽離 ingress / adoption glue：通常合理
- 抽薄 supervision / reconciliation：通常危險

### 3. 這個改動是否把某個外層責任錯誤拉回 `hcodex` core？

例如：

- owner authority
- observer projection
- Telegram adapter UX

如果有，代表邊界正在回退。

## 與其他計劃的關係

- [hcodex-lifecycle-supervision.md](/Volumes/Data/Github/threadBridge/docs/plan/hcodex-lifecycle-supervision.md)
  - 展開 `supervise` 與 `reconcile` 的正式約束
- [hcodex-launch-contract.md](/Volumes/Data/Github/threadBridge/docs/plan/hcodex-launch-contract.md)
  - 展開 `launch` 與 `bridge` 的 websocket contract
- [hcodex-pre-refactor-history.md](/Volumes/Data/Github/threadBridge/docs/plan/hcodex-pre-refactor-history.md)
  - 提供這份責任矩陣的歷史背景
- [owner-runtime-contract.md](/Volumes/Data/Github/threadBridge/docs/plan/owner-runtime-contract.md)
  - 固定哪些 ownership 屬於 owner，哪些 ownership 屬於 `hcodex`

## 開放問題

- ingress relay 應長期與 `hcodex` 保持同模組，還是應切成更獨立的 runtime ingress capability
- adoption pending 的 write-path 應留在 `reconcile` 末端，還是抽成更明確的 runtime state transition
- stale recovery 最終應保留多少 read-side fallback，多少完全轉回 supervisor write-side 保證

## 建議的下一步

1. 將這份文檔視為 `hcodex` clean refactor 的總責任矩陣。
2. 後續 patch series 以這 4 個核心責任拆分模組，而不是按現有文件名切。
3. 任何想移除或簡化 `hcodex` 的提案，都必須先指出它碰的是哪一類責任，不能只說「這段看起來像 shim」。

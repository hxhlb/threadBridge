# Codex Agent 安裝與首次 Telegram Setup 引導草稿

## 目前進度

這份文檔目前是純草稿，尚未開始實作成正式 agent-facing guide。

目前代碼裡已經有的相關前置條件：

- [README.md](../../../README.md) 已有 `Requirements`、`Setup`、`First Run Flow`，可作為最小安裝說明
- `threadbridge_desktop` 已支援 desktop-first 啟動，且在缺少 Telegram 憑據時也能先啟動本地 management UI
- desktop runtime 已有 first-run welcome 導流，會打開本地 management UI 的 `#/welcome`
- `GET /api/setup` 已能回傳 `first_run`、`telegram_token_configured`、`authorized_user_ids`、`telegram_polling_state`、`control_chat_ready`、`bot_url`
- `PUT /api/setup/telegram` 已能保存 bot token 與 authorized user ids，且 setup 儲存後會在背景重新嘗試拉起 Telegram polling
- 第一個 workspace 已可從本地 management UI / tray picker 或 Telegram `/add_workspace <absolute-path>` 建立

目前尚未完成：

- repo 內還沒有一份專門給外部 Codex agent 使用的安裝與首次設定主文檔
- 目前沒有固定「agent 應優先檢查哪些本地狀態、哪些步驟必須交給使用者完成」的正式 contract
- 目前沒有一條從安裝、啟動、Telegram setup、control chat ready，到 first workspace bind 的單一成功路徑文檔
- 現有 onboarding 想法仍偏產品/UI 視角，尚未整理成 agent 可直接照做的操作手冊

## 問題

今天關於 `threadBridge` 首次使用的資訊分散在幾個地方：

- repo `README`
- desktop/runtime 的實作細節
- management setup API
- `macos-menubar-thread-manager` 裡的 first-run onboarding 草稿

這對維護者自己還勉強可行，但對「另一個 Codex agent 代替使用者完成安裝與首次設定」並不夠穩定。

缺口主要有幾個：

- agent 不知道哪些前置條件可以本地檢查，哪些一定要請使用者去 Telegram 完成
- agent 容易把「token 已保存」誤當成「Telegram bot 已完成 setup」
- agent 容易忽略 `control_chat_ready` 與第一個 workspace bind，導致 setup 停在半成品
- agent 容易照舊模型指引用戶直接從 Telegram 開始，而忽略現在正式支援的 desktop-first / management-first 路徑

因此這個問題不是再補一段 README，而是需要一份獨立文檔，定義 agent 應如何帶使用者走完第一條成功路徑。

## 定位

這份文檔定義的是：

- `threadBridge` 的 agent-facing 安裝與首次 Telegram setup 引導草稿

它處理：

- agent 如何做本地 preflight
- agent 應採用哪條正式啟動路徑
- agent 如何引導使用者建立 Telegram bot 與收集 authorized user id
- agent 應用哪些現有 runtime/setup state 作為 checkpoint
- agent 何時應引導使用者送出 `/start`
- agent 何時應引導使用者建立第一個 workspace

它不處理：

- `runtime_protocol` 的完整 query / action / event 規格
- welcome 頁或 management UI 的完整產品視覺與互動細節
- 多 bot token 的最終設定模型
- public release packaging / notarization 流程
- 第一個 workspace 之後的長期使用與進階修復流程

這份草稿掛在 `management-desktop-surface/`，因為第一條正式成功路徑的主承載面，仍是 desktop runtime 與本地 management setup surface，而不是 Telegram adapter 本身。

## 最短成功路徑

這份文檔應固定一條 agent 可遵循的最短成功路徑：

1. 檢查本機前置條件。
2. 啟動 `threadbridge_desktop`。
3. 打開本地 management UI / welcome 頁。
4. 引導使用者建立或提供 Telegram bot token。
5. 引導使用者提供自己的 Telegram user id，並寫入 authorized users。
6. 保存 setup，確認 polling 已恢復。
7. 引導使用者打開 bot URL，對 bot 發送第一條 `/start`。
8. 確認 `control_chat_ready=true`。
9. 引導使用者建立第一個 workspace binding。

其中第 9 步雖然已進入 workspace lifecycle，但對首次 setup 而言仍屬合理範圍，因為使用者若停在 control chat 建立前後，通常還無法真正開始使用 `threadBridge`。

## Agent Contract

這份 guide 應明確約束 agent 的行為。

### 1. 先驗證本地狀態，再決定要不要問使用者

agent 應優先從本地可觀測狀態判斷：

- 是否在 macOS 上
- Rust / Python 3 / `codex` CLI 是否存在
- `threadbridge_desktop` 是否可啟動
- 本地 management UI 是否可連上
- `GET /api/setup` 回傳什麼狀態

只有下面這類資訊才應優先向使用者索取：

- 要用新的 bot 還是現有 bot
- BotFather 建立出來的 bot token
- 需要被授權的 Telegram user id
- 第一次綁定的 workspace 路徑

### 2. 不要把 README 記憶當成 source of truth

agent 應優先以目前 repo 中的：

- `README.md`
- `rust/src/management_api.rs`
- `rust/src/bin/threadbridge_desktop.rs`
- `docs/plan/management-desktop-surface/macos-menubar-thread-manager.md`

作為安裝與 setup 的 current source of truth。

若這些來源和舊印象衝突，應以現行代碼與 repo 文檔為準。

### 3. 不要把 setup 與 onboarding 完整度混成同一件事

agent 應分開判斷：

- `first_run`
  - 只表示本機是否還沒有 `config.env.local`
- `telegram_token_configured`
  - 表示 token 是否已持久化
- `control_chat_ready`
  - 表示使用者是否已對 bot 發出第一條 `/start`

不能因為 `first_run=false`，就假設 setup 已完成。
也不能因為 token 已保存，就假設 bot 已準備好接收工作 thread。

### 4. 不要回退到舊的 Telegram-first 心智

agent 不應把正式主流程描述成：

- 先去 Telegram 手打一堆 slash command，再回來補本地設定

目前較穩定的主路徑應是：

- desktop runtime / management UI 先完成 Telegram setup
- `/start` 只在 setup 保存後作為 control chat readiness checkpoint
- 第一個 workspace 再由 management surface 或 `/add_workspace` 建立

## 建議的引導階段

### 階段 A: 本機 preflight

agent 應先檢查：

- 作業系統是否符合支援形態
- Rust toolchain 是否可用
- Python 3 是否可用
- `codex` CLI 是否已安裝並完成登入
- repo 是否可正常執行 `cargo run --bin threadbridge_desktop`

若缺少這些前置條件，agent 應先停在安裝與修正環節，不要過早開始 Telegram setup。

### 階段 B: 啟動 desktop runtime

agent 應引導使用者或直接幫使用者啟動：

- `cargo run --bin threadbridge_desktop`

這一階段要固定一個語義：

- 缺少 Telegram 憑據不等於無法啟動
- 啟動成功後，應優先使用本地 management UI / welcome 頁承接後續 setup

### 階段 C: Telegram bot 與 authorized users setup

這一階段應固定拆成兩個使用者任務：

1. 透過 `@BotFather` 建立或取得 bot token
2. 透過 `@userinfobot` 或等價方式取得自己的 Telegram user id

agent 應把這些資料寫入既有 setup surface，而不是另發明一套暫存檔案格式。

setup 保存後，至少應驗證：

- `telegram_token_configured=true`
- `authorized_user_count > 0`
- `telegram_polling_state=active`

若 polling 沒有進入 `active`，agent 應把它視為 setup 尚未完成，而不是直接前往下一步。

### 階段 D: Control Chat Ready

在 token 與 authorized users 保存成功後，agent 應引導使用者：

- 打開 `bot_url`
- 在 Telegram 私聊中對 bot 發送 `/start`

完成後應驗證：

- `control_chat_ready=true`
- `control_chat_id` 已存在

這一步是首次 setup 的正式 checkpoint，因為沒有 control chat，就沒有穩定的 `/add_workspace` 與後續控制面。

### 階段 E: First Workspace Bind

最後一階段應引導使用者建立第一個 workspace。

偏好的主路徑應是：

- 本地 management UI 或 tray 的 folder picker

可接受的備援路徑才是：

- Telegram `/add_workspace <absolute-path>`

完成後至少應驗證：

- workspace 已出現在 managed workspace list
- 該 workspace 已有綁定 thread
- fresh Codex session 已建立

這樣 agent 才能合理宣告第一次 setup 已走到可用狀態。

## 建議的 Checkpoints

這份 guide 應固定一組 agent 可重用的檢查點。

### 安裝完成

- 可執行 `threadbridge_desktop`
- management UI 可存取

### Telegram setup 已保存

- `first_run=false`
- `telegram_token_configured=true`
- `authorized_user_count > 0`

### Telegram bot 已開始工作

- `telegram_polling_state=active`
- `bot_url` 可用

### Control chat 已建立

- `control_chat_ready=true`
- `control_chat_id` 存在

### 首次可用

- 至少一個 workspace 已成功 bind
- 使用者可以從 Telegram workspace thread 或 `./.threadbridge/bin/hcodex` 繼續工作

## 對文檔與實作的影響

若這條草稿之後被採納，應補出至少一個正式對外入口：

- 一份 agent-facing setup guide
  - 可放在 repo docs，供外部 Codex agent 直接遵循

並補一份對內對齊：

- 哪些 `SetupStateView` 欄位是首次 setup 的正式 checkpoint
- 哪些步驟屬於 user-only action，不能假裝由 runtime 自動完成
- management UI welcome 文案、README、與 agent guide 之間要共用同一條最短成功路徑

## 與其他計劃的關係

- [macos-menubar-thread-manager.md](./macos-menubar-thread-manager.md)
  - 定義產品面上的 first-run onboarding 與 welcome 頁方向；本文件則把它收斂成 agent-facing 操作手冊草稿
- [runtime-protocol.md](../runtime-control/runtime-protocol.md)
  - `SetupStateView` 與 setup query 是這份 guide 的狀態來源之一
- [session-lifecycle.md](../runtime-control/session-lifecycle.md)
  - 第一個 workspace bind 與 `add_workspace` 語義由它承接
- [multi-bot-token-support.md](../telegram-adapter/multi-bot-token-support.md)
  - 若未來 Telegram setup 改成多 bot registry，這份 guide 也要從單 bot flow 升級

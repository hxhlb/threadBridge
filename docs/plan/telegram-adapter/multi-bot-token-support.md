# 多 Telegram Bot Token 支持草稿

## 目前進度

這份文檔目前仍是純草稿，尚未開始實作。

目前代碼與 setup surface 仍明確採用單 bot token 模型：

- `config.env.local` 只寫入單一 `TELEGRAM_BOT_TOKEN`
- `AUTHORIZED_TELEGRAM_USER_IDS` 也是單一全域集合，而不是 per-bot 設定
- `load_telegram_config()` / `load_app_config()` 只會解析一組 Telegram 設定
- `spawn_bot_runtime*` 只會建立一個 `Bot::new(token)`
- management setup / cached bot identity / polling state 也都以單 bot 為前提

因此目前若要切換 bot，只能覆蓋整份本機 Telegram setup，而不是讓同一個 desktop runtime 同時管理多個 bot identity。

目前新增確認的一個產品想法是：

- `threadBridge` 應正式支持多個 Telegram bot token
- 這不是單純為了未來抽象化預留空位，而是 Telegram adapter 自己就需要的能力
- 記錄這個想法的重點不是「也許之後可以做多 bot」，而是要把它視為應收斂成正式設定與路由語義的能力面

## 問題

單 bot 模型目前有幾個明顯限制：

- 無法同時承接多個 Telegram bot identity
- 無法把不同 workspace、不同用途、或不同 rollout 階段拆到不同 bot
- token rotation、sandbox / production 分流、或個人 / 團隊 bot 分離，都會變成整機級替換
- management UI 的 setup 也被迫只回答「目前這台機器的唯一 bot 是誰」

這個限制是 Telegram adapter 自己的能力邊界問題，不是 runtime core 是否要支持多 IM 的問題。

換句話說，這份草稿要回答的是：

- `threadBridge` 是否應該允許同一個 desktop runtime 同時管理多個 Telegram bot token
- 如果要支持，多 bot 應該落在哪個設定模型、啟動模型、以及 thread / workspace 綁定語義上

目前答案傾向是：

- 應該支持
- 但應以正式 bot registry / bot identity model 方式落地，而不是繼續堆疊單 bot env var 變體

## 定位

這份文檔只處理 Telegram adapter 的多 bot 能力。

它不是：

- 多 IM / 多 transport 產品化草稿
- runtime owner 邊界重構文檔
- 一般權限系統或多租戶安全模型主規格

它應掛在 `telegram-adapter/`，因為主要責任仍是 Telegram bot identity、polling、delivery、以及 adapter-facing setup surface。

## 方向

較合理的近期方向不是把單一 `TELEGRAM_BOT_TOKEN` 擴成第二個平行環境變數，而是先把 machine-local Telegram setup 重新描述成「bot identity registry」。

至少應滿足下面幾個目標：

- 一台 desktop runtime 可持有多個 Telegram bot identity
- 每個 bot 有自己的 token 與 authorized user 設定
- thread / workspace 能知道自己屬於哪個 bot，而不是只假設「所有 thread 都來自當前全域 bot」
- management UI 可以列出、增刪、驗證、以及選擇預設 bot
- Telegram polling / bridge / callback routing 可以按 bot instance 分流，而不是只有單一全域 handle

## 為什麼不應只補一個欄位

如果只是把現有 setup 改成：

- `TELEGRAM_BOT_TOKEN_A`
- `TELEGRAM_BOT_TOKEN_B`

這仍然會留下幾個核心空洞：

- thread metadata 沒有正式記錄 bot identity
- callback query / media send / bot URL cache 仍是單 bot 假設
- management API 的 `telegram_token_configured` / `control_chat_ready` 之類 view 無法表達「哪個 bot」
- first-run onboarding 仍會誤把 Telegram setup 當成一次性單 bot 流程

所以這個能力本質上不是「多一個 token 欄位」，而是 Telegram adapter 的 identity model 要正式成形。

## 建議的能力邊界

### 1. Bot registry 是 machine-local setup

bot token 應視為 machine-local Telegram setup 的一部分。

這代表：

- registry 由 desktop runtime / management API 持有
- setup 寫入面不應再只暴露單一 `telegram_token`
- runtime 啟動時應先解析 bot registry，再決定要啟動哪些 polling runtime

### 2. Thread / workspace 綁定必須記錄 bot identity

只要支持多 bot，Telegram thread metadata 就不能只靠 `chat_id` / `message_thread_id` 隱含來源。

至少需要一個穩定的 bot identity 欄位，用來回答：

- 這個 Telegram thread 是由哪個 bot 接入
- 最終回覆、preview draft、callback answer 應透過哪個 bot 發送
- restore / reconnect / observability 顯示的是哪個 bot

### 3. Authorized users 應改成 per-bot，而不是全域單集合

目前 `AUTHORIZED_TELEGRAM_USER_IDS` 是單一全域集合。

若支持多 bot，較合理的預設應是：

- authorized users 跟著 bot 走

否則會出現一個奇怪結果：

- 同一台機器上的所有 bot 都被迫共享同一份授權名單

這通常不是最乾淨的產品或安全邊界。

### 4. Polling state / bot identity cache 應改成 per-bot view

現在的 setup 與 management state 比較接近：

- Telegram 是否已配置
- 目前 polling 是否 active
- 目前 bot username / URL 是什麼

多 bot 之後，這些 read-side surface 都應改成列表或 keyed view，而不是單一扁平欄位。

## 建議的收斂方向

### 1. 設定模型

應引入正式的 Telegram bot entry 概念，例如：

- `bot_key`
- `label`
- `token`
- `authorized_user_ids`
- `enabled`
- `is_default`

這裡的重點不是欄位名稱本身，而是：

- setup 不再只是一個 scalar token
- bot identity 要有穩定 key，不能只把 token 當 key 到處傳

目前 `config.env.local` 是單值 env 檔；若要支持多 bot，較可能需要：

- 新的結構化本機設定檔，或
- 由 management API 擁有的結構化 Telegram setup 載體

單純把複數 bot 塞回一堆 env key，長期可維護性很差。

### 2. 啟動模型

desktop runtime 啟動後應能：

- 為每個 enabled bot 啟動各自的 polling runtime，或
- 在明確限制下，只啟動被選中的 active bot

這裡的關鍵是先把模型說清楚：

- 支持多 token
- 是否等於同時多 bot polling

這兩件事不一定完全相同。

第一版可以先只支持：

- registry 中存在多個 bot
- 但只有一個 active bot 會被拉起

只要 thread / workspace metadata 已開始記錄 bot identity，之後再擴到真正 concurrent multi-bot polling 會比較乾淨。

### 3. Thread 建立與回覆路由

至少以下幾個流程都要按 bot identity 分流：

- `/start` 與 add workspace 初始綁定
- ordinary message turn
- preview / final reply
- callback query
- `send_telegram_media`
- control chat deep link / bot URL 顯示

否則多 bot 只會停留在 setup 畫面，實際 delivery 仍會混回單 bot 假設。

### 4. Management surface

welcome / settings / setup UI 應從：

- 「填一個 bot token」

改成：

- 「管理 bot 列表」

至少要能：

- 新增 bot
- 驗證 bot identity
- 編輯 authorized users
- 啟用 / 停用 bot
- 指定預設 bot

若第一版不做 concurrent polling，也應明確顯示：

- 哪個 bot 目前 active
- 哪些 thread / workspace 綁在該 bot

## 非目標

這份草稿目前不打算先定義：

- 多 bot 之間的完整權限隔離模型
- 一個 workspace 同時掛多個 Telegram bot 的協作語義
- 非 Telegram transport 的共用 identity registry
- Telegram Web App 與多 bot 的完整產品整合

## 開放問題

- 第一版是否只支持「多 token registry + 單 active bot」，還是直接支持 concurrent multi-bot polling
- bot identity 要持久化在既有 `config.env.local` 的兼容格式上，還是升級成新的結構化設定檔
- 舊的 `TELEGRAM_BOT_TOKEN` / `AUTHORIZED_TELEGRAM_USER_IDS` 如何平滑遷移
- `SetupStateView`、`RuntimeHealthView`、以及 `telegram_polling_state` 應如何表達多 bot 狀態
- thread / workspace 是否需要顯示 bot label，避免 observability 與 management UI 混淆

## 建議的第一步

若之後要正式啟動這條能力，較合理的第一步是先做一份更具體的 v1 scope 文檔，固定：

- 第一版是否允許 concurrent multi-bot polling
- bot registry 的持久化載體
- thread metadata 需要新增的 bot identity 欄位
- management API setup / read-side view 的最小變更面

在這些核心語義還沒定之前，不建議直接從 `TELEGRAM_BOT_TOKEN` 開始做 ad hoc 擴充。

# Desktop Runtime Tool Bridge 草稿

## 目前進度

這份文檔目前是純草稿，尚未開始實作。

目前代碼裡已經有的前置條件：

- `threadbridge_desktop` 已開始作為 machine-local 長壽命 runtime owner
- 本地 management API 已有 query / control / SSE 骨架
- workspace 內已存在 `.threadbridge/bin/*` tool surface
- `threadBridge` 已能把某些能力明確留在 desktop runtime / machine-local 層，而不是全部塞進 Telegram 或 workspace wrapper

目前尚未完成：

- 從 workspace tool surface 呼叫 desktop runtime 的正式橋接模型
- 跨沙盒 / 跨 workspace 的 desktop capability 語義
- 這類 capability 的權限、審計、回傳 artifact 模型
- `desktop screenshot` 之類能力的正式 API / tool contract
- 由 desktop runtime 提供自定義 webview service 的正式 lifecycle / API / 安全邊界

目前新增確認的一個核心要求是：

- 只要是跨沙盒 capability，就需要 desktop runtime 的授權確認

目前新增記錄的一個方向是：

- desktop runtime 可以作為 tools 層，提供自定義 webview service，而不只是一次性 artifact capability

## 問題

現在 `threadBridge` 的工具面大多還是：

- workspace 內 wrapper
- 呼叫 repo 的 Python tool
- 產生 workspace artifact

這對純 workspace-local 的工具很好，但有一類能力天然不屬於 workspace 沙盒：

- 桌面截圖
- 讀取本機 UI / 視窗狀態
- 透過 desktop runtime 代執行 machine-local action

如果之後真的要讓 Codex 或 workspace tool 能安全地使用這些能力，就不能直接把它們塞進一般 workspace wrapper 裡，否則很容易模糊：

- workspace tool
- desktop owner capability
- 跨沙盒 privileged action

之間的邊界。

所以這個問題本質上是在問：

- threadBridge 是否要提供一個由 desktop runtime 代理執行的 tool bridge

## 定位

這份文檔定義的是：

- `desktop runtime as capability host`

它處理：

- workspace / Codex 如何請求 machine-local capability
- desktop runtime 如何作為特權能力執行者
- 這些能力如何回傳 artifact / result
- 哪些工具型能力應由 desktop runtime 提供受管 custom webview service
- 這些能力如何被記錄、授權、觀測

它明確不處理：

- Telegram renderer / delivery 細節
- 一般 workspace-local tool wrapper
- 完整 UI automation 產品策略
- 把 desktop runtime 直接變成第二個主對話代理

## 核心想法

### 1. desktop runtime 不只是管理面，也可以是 capability host

目前 desktop runtime 主要被理解成：

- owner
- tray / web 管理面
- local management API

但更合理的長期方向可能是：

- desktop runtime 也是 machine-local privileged capability host

也就是說，某些不適合在 workspace 沙盒內直接做的事，應由它代為執行。

### 2. 這應是 tool bridge，不是任意 shell escape

這條線不能寫成：

- 讓 workspace tool 任意要求 desktop runtime 執行任意命令

比較合理的方向應是：

- desktop runtime 只暴露少量明確 capability
- 每個 capability 有清楚 request / result schema
- runtime / user 能看懂它做了什麼

初版適合的能力例如：

- `desktop_screenshot`

而不是一開始就暴露通用 `run_anything_outside_sandbox`。

而且這些 capability 不應默默執行。

較合理的 v1 語義是：

- request 先送到 desktop runtime
- desktop runtime 顯示或持有授權確認
- 確認後才真正執行 capability

### 3. capability 應先經過 threadBridge protocol，而不是直接 shell 掉

這表示較合理的呼叫鏈路是：

- workspace / Codex
- 呼叫 threadBridge tool bridge
- desktop runtime 執行 capability
- result / artifact 回到 threadBridge runtime
- 再由 adapter / workspace tool surface 消費

而不是：

- workspace script 直接繞過 runtime 去碰 desktop 層

### 4. 跨沙盒能力預設需要授權確認

這條線最重要的新限制是：

- 只要 capability 跨出 workspace 沙盒，就預設需要 desktop runtime 授權確認

也就是說，v1 不應採用：

- desktop runtime 啟著就自動允許所有跨沙盒 capability

比較合理的方向是：

- runtime 先收到 capability request
- desktop runtime 以 machine-local UX 顯示 pending request
- 使用者顯式允許或拒絕
- threadBridge 再把結果回傳給 workspace / Codex / adapter

### 5. desktop runtime 也可以提供 tools 層的自定義 webview service

不是所有 capability 都適合收斂成：

- request 進來
- 執行一次 action
- 回傳單次 artifact / result

有一類工具更像是：

- machine-local 的受管 UI surface
- 有自己的短生命週期互動
- 可能需要 desktop runtime 持有視窗 / webview / 本地 session

例如未來如果有：

- 圖像挑選 / 標註面板
- 桌面級 approval / picker / inspector
- session-specific observability shell

比較合理的模型，不一定是讓 workspace 直接開瀏覽器或自己持有 UI，而是：

- desktop runtime 作為 tools 層 capability host
- 它代為啟動一個受管 custom webview service
- workspace / Codex 只拿到可引用的 handle、service state、或輸出結果

這樣做的價值是：

- UI shell 仍屬於 machine-local owner 邊界
- workspace 不需要自己越過 sandbox 管視窗生命週期
- tool bridge 可以同時承接一次性 capability 和短生命週期 service capability

這裡的 `custom webview service` 不應被理解成：

- 任意載入遠端網站
- 另一個獨立主對話代理
- 繞過 management API 的 ad-hoc UI 容器

比較合理的定義是：

- 由 desktop runtime 啟動或持有
- 有明確 tool schema / lifecycle
- 服務特定工作流
- 預設是 machine-local
- 仍受 threadBridge audit / approval / state view 約束

## 初版能力：`desktop_screenshot`

最自然的 v1 範例就是桌面截圖。

這個能力之所以適合作為第一個 capability，是因為：

- 它清楚是 machine-local
- 它很容易超出一般 workspace 沙盒
- 它回傳的結果是明確 artifact
- 它也很容易驗證 UI / Codex / tool bridge 的整體鏈路

初版至少要能回答：

- 截的是整個螢幕、指定螢幕，還是目前前景視窗
- 產物存去哪裡
- 是否要回傳 metadata
- Telegram / desktop / workspace 端怎麼引用該 artifact

## 後續能力方向：`desktop_webview_service`

若 `desktop_screenshot` 是一次性 capability 的代表，那另一條值得先記錄的方向是：

- `desktop_webview_service`

它代表的不是單次 action，而是：

- 由 desktop runtime 提供一個受管 custom webview surface
- 讓某個 tool / workflow 在 machine-local shell 中短暫存在
- 再把使用者操作結果、產出 artifact、或 service state 回傳給 threadBridge

這個方向適合承接的情境包括：

- 需要 richer local UI，但又不值得變成完整 management page
- 需要 workspace tool 觸發，但不適合讓 workspace 自己持有瀏覽器 / webview
- 需要 owner 可見、可關閉、可審計的本地互動面

v1 不需要先把它做成完整框架，但至少應先回答：

- service 由誰啟動與銷毀
- service 是否綁定 `thread_key` / `workspace_cwd` / `session_id`
- webview 載入的是本地靜態資產、management API route，還是受限的本地 app shell
- service result 如何寫回 threadBridge protocol
- 這類 service 是否與普通 capability 共用 approval / audit lane

## 建議的資料模型

### `DesktopCapabilityRequest`

至少包含：

- `request_id`
- `workspace_cwd`
- `thread_key`
- `capability`
  - 例如 `desktop_screenshot`
- `arguments`
- `requested_by`
  - `workspace_tool`
  - `management_ui`
  - `runtime`
- `requested_at`
- `requires_desktop_approval`
- `approval_reason`

### `DesktopCapabilityResult`

至少包含：

- `request_id`
- `capability`
- `status`
  - `completed`
  - `failed`
  - `denied`
- `artifacts`
- `summary`
- `error`
- `completed_at`
- `approved_at`
- `approved_by`

### `DesktopServiceHandle`

若 capability 不是單次 action，而是受管 service，至少需要一個可追蹤 handle：

- `service_id`
- `service_kind`
  - 例如 `desktop_webview_service`
- `thread_key`
- `workspace_cwd`
- `session_id`
- `status`
  - `pending_approval`
  - `launching`
  - `running`
  - `completed`
  - `failed`
  - `closed`
- `entrypoint`
  - 例如 local route、webview target、或受管 page key
- `artifacts`
- `summary`
- `created_at`
- `updated_at`
- `closed_at`

### `DesktopScreenshotArtifact`

至少包含：

- `path`
- `mime_type`
- `width`
- `height`
- `captured_at`
- `capture_target`
  - `screen`
  - `window`
  - `selection`

## Artifact 邊界

這條線最重要的是先決定 artifact 歸誰。

比較合理的方向是：

- capability 由 desktop runtime 執行
- artifact 仍應落到 thread / workspace 可引用的位置
- 但不應讓 desktop runtime 的私有暫存和 workspace artifact 混成同一層

若是 `desktop_webview_service` 這種受管 UI capability，還要再多區分：

- desktop runtime 持有的 service state
- webview/session 關聯的 ephemeral local state
- service 結束後可導出的 workspace-visible artifact

至少要區分：

- desktop runtime private temp
- workspace-visible exported artifact
- Telegram / adapter-facing delivery artifact

## 權限與安全邊界

這條線如果做錯，會很危險。

至少要回答：

- 哪些 capability 預設允許
- 哪些 capability 需要顯式使用者同意
- 是否要限制只有 desktop runtime owner 存在時才能執行
- 是否要把 capability 呼叫記進 runtime event / audit log

目前新增確認的 v1 策略應是：

- allowlist capability
- 跨沙盒 capability 預設 `requires_desktop_approval = true`
- 明確 request / result schema
- 先不支持任意 shell / arbitrary command

也就是說，像 `desktop_screenshot` 這種能力，v1 預設就應被視為：

- 需要 desktop runtime 顯式確認

而不是：

- 只要 request 進來就直接執行

## 授權確認模型

比較合理的 v1 授權流程應是：

1. workspace tool / runtime 發出 capability request。
2. desktop runtime 把它記成 pending approval。
3. 使用者在 desktop runtime surface 明確允許或拒絕。
4. threadBridge 寫回 `completed` / `denied` result。

這個確認面不一定一開始就要是複雜 UI，但至少要滿足：

- 在 machine-local desktop runtime 發生
- 不依賴 Telegram 端確認
- 有可審計的 allow / deny 結果

這條限制很重要，因為它直接把：

- cross-sandbox capability

和：

- ordinary workspace tool

清楚分開。

## 與 owner 收斂的關係

這份 plan 和 owner convergence 直接相關。

原因是：

- 只有 owner 收斂後，desktop runtime 才適合成為可信的 machine-local capability host
- 如果 bot、`hcodex`、desktop runtime 都能各自決定如何跨沙盒，這條線很快會失控

所以較合理的順序是：

1. 先收斂 owner authority。
2. 再讓 desktop runtime 暴露少量 capability bridge。
3. 最後才讓 workspace tool / Codex 正式依賴這條橋。

## 與其他計劃的關係

- [macos-menubar-thread-manager.md](/Volumes/Data/Github/threadBridge/docs/plan/management-desktop-surface/macos-menubar-thread-manager.md)
  - desktop runtime 已是 machine-local control plane；這份是往 capability host 再推一層
- [runtime-protocol.md](/Volumes/Data/Github/threadBridge/docs/plan/runtime-control/runtime-protocol.md)
  - capability request / result 之後應掛進正式 view / action / event 模型
- [runtime-transport-abstraction.md](/Volumes/Data/Github/threadBridge/docs/plan/runtime-control/runtime-transport-abstraction.md)
  - 這條線再次強化 Telegram 不是 core；desktop capability 也應屬於 core/runtime-side service
- [optional-agents-injection.md](/Volumes/Data/Github/threadBridge/docs/plan/runtime-control/optional-agents-injection.md)
  - 若未來有 tools-only / external instruction 模式，也要考慮這種 capability tool 如何被宣告
- [telegram-webapp-observability.md](/Volumes/Data/Github/threadBridge/docs/plan/telegram-adapter/telegram-webapp-observability.md)
  - capability request / result / artifact 之後應成為可觀測事件

## 風險

- 若把 desktop runtime 寫成通用越獄出口，會直接破壞 sandbox / owner 邊界
- 若 artifact 邊界不清，結果很容易散落在 desktop temp、workspace、Telegram delivery 之間
- 若 capability 沒有 audit / consent 模型，使用者很難信任這條能力面
- 若跨沙盒 capability 沒有 desktop runtime 的本地授權確認，owner 與 sandbox 邊界會再次變得模糊
- 若這條線直接寫死成 macOS only UI helper，之後很難抽成 runtime capability

## 開放問題

- v1 是否只做 `desktop_screenshot`？
- capability request 應該走 local HTTP control action、workspace tool request file，還是另一條專用通道？
- `desktop_webview_service` 應該被視為 capability 的一種，還是 protocol 裡另一種 service primitive？
- artifact 應先落在 workspace，還是由 desktop runtime 保管再導出？
- desktop runtime 的授權確認 v1 應該放在 tray、管理頁，還是原生通知 / dialog？
- 這條能力面未來是否應擴展到更多 desktop capability，例如視窗選取、檔案 picker、通知、UI automation？
- custom webview 應優先做成 management API route + 受管殼層，還是獨立 asset bundle / page registry？

## 建議的下一步

1. 先把這條能力面收斂成「desktop runtime capability host」而不是泛化的 sandbox escape。
2. 明確規定跨沙盒 capability 的 v1 默認需要 desktop runtime 授權確認。
3. 先以 `desktop_screenshot` 定義最小 request / result / artifact 模型。
4. 補一份 `desktop_webview_service` 的最小 lifecycle 草圖，確認它和 ordinary capability 的共用部分與分歧。
5. 把 capability request / result / approval state，及必要的 service handle/state，掛回 `runtime-protocol` 的 action / event 命名。
6. 再決定它是走 management API、workspace tool bridge，還是兩者共用的統一通道。

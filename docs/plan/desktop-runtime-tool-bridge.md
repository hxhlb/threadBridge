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

### 3. capability 應先經過 threadBridge protocol，而不是直接 shell 掉

這表示較合理的呼叫鏈路是：

- workspace / Codex
- 呼叫 threadBridge tool bridge
- desktop runtime 執行 capability
- result / artifact 回到 threadBridge runtime
- 再由 adapter / workspace tool surface 消費

而不是：

- workspace script 直接繞過 runtime 去碰 desktop 層

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

初版比較合理的策略是：

- allowlist capability
- 明確 request / result schema
- 先不支持任意 shell / arbitrary command

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

- [macos-menubar-thread-manager.md](/Volumes/Data/Github/threadBridge/docs/plan/macos-menubar-thread-manager.md)
  - desktop runtime 已是 machine-local control plane；這份是往 capability host 再推一層
- [runtime-protocol.md](/Volumes/Data/Github/threadBridge/docs/plan/runtime-protocol.md)
  - capability request / result 之後應掛進正式 view / action / event 模型
- [runtime-transport-abstraction.md](/Volumes/Data/Github/threadBridge/docs/plan/runtime-transport-abstraction.md)
  - 這條線再次強化 Telegram 不是 core；desktop capability 也應屬於 core/runtime-side service
- [optional-agents-injection.md](/Volumes/Data/Github/threadBridge/docs/plan/optional-agents-injection.md)
  - 若未來有 tools-only / external instruction 模式，也要考慮這種 capability tool 如何被宣告
- [telegram-webapp-observability.md](/Volumes/Data/Github/threadBridge/docs/plan/telegram-webapp-observability.md)
  - capability request / result / artifact 之後應成為可觀測事件

## 風險

- 若把 desktop runtime 寫成通用越獄出口，會直接破壞 sandbox / owner 邊界
- 若 artifact 邊界不清，結果很容易散落在 desktop temp、workspace、Telegram delivery 之間
- 若 capability 沒有 audit / consent 模型，使用者很難信任這條能力面
- 若這條線直接寫死成 macOS only UI helper，之後很難抽成 runtime capability

## 開放問題

- v1 是否只做 `desktop_screenshot`？
- capability request 應該走 local HTTP control action、workspace tool request file，還是另一條專用通道？
- artifact 應先落在 workspace，還是由 desktop runtime 保管再導出？
- 桌面截圖是否需要使用者顯式確認，還是只要 desktop runtime 啟用即可？
- 這條能力面未來是否應擴展到更多 desktop capability，例如視窗選取、檔案 picker、通知、UI automation？

## 建議的下一步

1. 先把這條能力面收斂成「desktop runtime capability host」而不是泛化的 sandbox escape。
2. 先以 `desktop_screenshot` 定義最小 request / result / artifact 模型。
3. 把 capability request / result 掛回 `runtime-protocol` 的 action / event 命名。
4. 再決定它是走 management API、workspace tool bridge，還是兩者共用的統一通道。

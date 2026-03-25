# Owner Runtime Contract 草稿

## 目前進度

這份文檔目前已進入「部分落地」。

目前已確認：

- `desktop runtime owner` 已是正式 runtime authority
- workspace app-server 已是 canonical runtime backend
- `hcodex` 已是 owner-managed local entrypoint，而不是自補 runtime 的獨立 owner
- local/TUI mirror intake 已開始從歷史上的 proxy 路徑拆到獨立 app-server observer
- management API 與 Telegram adapter 已開始消費同一批 runtime / transcript / control 語義

目前尚未完成：

- `runtime protocol` 仍未完全收斂成 transport-neutral 的正式契約
- observer 雖已存在，但 broader session observability 與 public vocabulary 收尾仍未完成
- `hcodex` ingress、launch contract、與 compatibility shim 的長期保留邊界仍未完全寫死
- adoption 的最終命名與對外呈現仍未拍板

## 問題

`threadBridge` 近期架構演化的核心，不是先抽新的 API，也不是先產品化多 adapter，而是把四個角色徹底拆開：

- `desktop runtime owner`
- `app-server ws observer`
- `hcodex`
- Telegram / management surface

目前最大的架構債，不在於功能缺失，而在於這四個角色仍有過渡性責任重疊，尤其是 `hcodex` / ingress 路徑仍曾混合入口、mirror、以及部分 adapter glue。

如果不先把 owner/runtime contract 收斂清楚，後續不論是 observer 化、`hcodex` launch cleanup、還是 transport abstraction，都很容易重新把 authority、projection、與 adapter UX 黏回一起。

## 定位

這份文檔是 owner/runtime boundary 的總草稿。

它處理：

- runtime authority 應固定在哪一層
- mirror read-side source 應固定在哪一層
- `hcodex` 應保留哪些核心責任
- Telegram / management surface 應如何退回 protocol consumer

它不處理：

- Telegram renderer / callback UX 的完整產品規格
- `codex plan`、preview、delivery 等單一子問題的細節規格
- 完整 transport-neutral protocol 的最終 wire format
- 每一個 compatibility shim 的立即移除時程

## 核心決策

### 1. `desktop runtime owner` 是唯一 runtime authority

近期唯一 authority 是 `desktop runtime owner`。

它負責：

- ensure / repair workspace runtime
- owner-canonical runtime health
- workspace-scoped control action orchestration
- observer 與本地入口的存在性管理

它不負責：

- Telegram message rendering
- preview / final reply 樣式
- adapter-specific callback UX

### 2. observer 是 canonical read-side projection source

`app-server ws observer` 的角色，是 mirror / observability 的 canonical read-side projection source，而不是新的 runtime owner。

observer 負責：

- thread-scoped event 訂閱
- preview / final / process projection
- session observability feed
- mirror intake contract

observer 不負責：

- 啟動 Codex binary
- child pid 管理
- workspace launch orchestration
- adapter-specific prompt / markup rendering

### 3. `hcodex` 是受管本地入口，不是 observer 或 authority

`hcodex` 的合理定位是：

- convenience entrypoint
- binary selector
- local lifecycle shim
- minimal transport compatibility shim

`hcodex` / ingress 路徑應保留的責任：

- launch 本地 `codex --remote`
- runtime-ready 檢查
- local session claim / pid tracking
- launch lifecycle 記錄
- 必要的 compatibility bridge / resolver
- live interactive response injection

它不應承擔：

- mirror canonical projection
- session observability 主聚合
- Telegram preview / final delivery
- runtime health authority

### 4. adapter 只消費 protocol 語義

Telegram adapter 與 management surface 的角色，應收斂成 protocol consumer / renderer。

其中：

- `desktop runtime owner`
  - producer / authority
- `runtime protocol`
  - canonical event / action semantics
- `management_api`
  - 本地 HTTP / SSE transport
- Telegram adapter
  - protocol consumer / renderer

這代表近期不應把 adoption、mirror、launch contract 的收斂理解成「先抽一個 API」，而應理解成：

- 先固定 runtime contract 與語義邊界
- 再讓不同 surface 以各自 transport 消費它

### 5. adoption 是 runtime signal，不是 `hcodex` UI

`hcodex` 可以產生「本地 session 結束後，有 continuity switch 候選」這種 lifecycle signal，但提示使用者、送出按鈕、接受 adopt / reject，應由 adapter surface 承接。

因此：

- signal 的 canonical 語義應提升到 runtime / state / control 模型
- Telegram 只負責把 signal 變成互動面
- management API 只是 transport，不是 adoption semantics owner

## 責任分工

### Desktop Runtime Owner

負責：

- ensure / repair workspace daemon
- ensure observer 與 `hcodex` 入口
- runtime health authority
- reconcile 與 control orchestration

不負責：

- adapter UX
- message rendering
- mirror projection 細節

### App-Server WS Observer

負責：

- thread-scoped event 訂閱
- preview / final / process projection
- session observability 事件流

不負責：

- local process lifecycle
- binary launch
- adapter-specific rendering

### `hcodex`

負責：

- 便利入口
- binary selection
- launch lifecycle
- local session claim / pid tracking
- 必要 compatibility shim

不負責：

- mirror canonical source
- runtime authority
- adapter UI

### Telegram / Management Surface

負責：

- commands / callback / media ingress
- prompt / button / callback rendering
- preview / final delivery
- control action 呈現與觸發

不負責：

- 直接確保 shared runtime
- 定義 canonical lifecycle semantics
- 直接成為 mirror source

## 與其他計劃的關係

- [app-server-ws-mirror-observer.md](/Volumes/Data/Github/threadBridge/docs/plan/app-server-ws-mirror-observer.md)
  - 處理 mirror intake 從歷史 ingress / proxy 路徑拆到 observer 的子問題
- [post-cli-runtime-cleanup.md](/Volumes/Data/Github/threadBridge/docs/plan/post-cli-runtime-cleanup.md)
  - 處理 `hcodex` launch contract、legacy artifact、與 compatibility 命名收尾
- [runtime-transport-abstraction.md](/Volumes/Data/Github/threadBridge/docs/plan/runtime-transport-abstraction.md)
  - 這份文檔是 transport abstraction 之前的前置邊界收斂
- [runtime-protocol.md](/Volumes/Data/Github/threadBridge/docs/plan/runtime-protocol.md)
  - 未來應承接這份文檔所要求的 canonical event / action / state semantics

這份文檔不重複定義上述子問題的實作細節，而是固定它們共同依附的 owner/runtime boundary。

## 開放問題

- adoption 最終是否保留這個對外命名，或改成更中性的 continuity switch 語言
- observer 是否需要更正式的 upstream subscribe contract，而不只依附現有 attach / resume 語義
- `hcodex` ingress 中哪些 compatibility shim 屬於長期入口能力，哪些應視為過渡結構
- `runtime protocol` 何時才算從本地 HTTP / SSE transport 收斂成真正的 transport-neutral 契約

## 建議的下一步

- 繼續把 observer 的 vocabulary、observability feed、與 public naming 收尾，避免文檔仍混用 `TUI proxy` 與 `hcodex ingress`
- 把 `hcodex` launch / ingress / compatibility shim 的長期保留邊界記錄到 cleanup 文檔，而不是散落在實作與 commit message
- 逐步讓 Telegram / management surface 對 mirror 與 control action 的依賴固定在 shared runtime semantics，而不是 ingress 內部細節

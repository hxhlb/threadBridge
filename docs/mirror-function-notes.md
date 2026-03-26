# Mirror 功能注意事項

日期：`2026-03-26`

這份文檔描述的是 `threadBridge` 目前 mirror 功能的維護契約，不是新的架構草案。

它處理：

- local `hcodex` / app-server observer 的 mirror 來源邊界
- fresh session 與 resume session 的 attach 規則
- 常見回歸模式
- 除錯時應先看哪些訊號

相關實作：

- [rust/src/hcodex_ingress.rs](../rust/src/hcodex_ingress.rs)
- [rust/src/app_server_observer.rs](../rust/src/app_server_observer.rs)
- [rust/src/workspace_status.rs](../rust/src/workspace_status.rs)
- [rust/src/telegram_runtime/status_sync.rs](../rust/src/telegram_runtime/status_sync.rs)
- [docs/codex-app-server-ws-protocol.md](codex-app-server-ws-protocol.md)

## 一句話模型

mirror 的 canonical projection 在 observer 層，但 fresh local `hcodex` session 的事件來源不是獨立 observer websocket，而是 ingress 已經握住的 live daemon stream。

## 目前契約

### 1. fresh local session

當 `hcodex` 送出 `thread/start` 並成功拿到新的 `thread.id` 時：

- session attach mode 必須是 `live_forwarded`
- `hcodex_ingress` 會把同一條 daemon websocket 上收到的 notifications forward 給 observer projection
- 不得立刻再開第二條 websocket 對同一個 fresh thread 做 `thread/resume`

原因：

- upstream Codex 對 fresh thread 的 rollout 是 lazy materialization
- 在第一個 user message 之前，thread 可能已有 `thread.id`，但 rollout 仍不存在
- 因此 `thread/resume` 並不是 fresh attach API

### 2. explicit resume session

當 session 是從既有 thread 進入，且語義上就是 resume：

- attach mode 才能是 `resume_ws`
- observer 可獨立開 websocket，透過 `thread/resume` attach 並接手 notifications

### 3. mode 一旦決定就不能切換

同一個 session / thread key 目前只允許一個 active source。

短期契約是：

- `live_forwarded` 不可中途切到 `resume_ws`
- `resume_ws` 也不可被另一個 source 靜默覆蓋
- 同 thread 的 mode conflict 必須視為錯誤，而不是自動 handoff

這是刻意的保守設計，用來避免 preview / process / final 重複或漏寫。

## 明確禁止事項

下面這些改動都屬於高風險，沒有一起處理完整契約前不要做：

- 在 fresh `thread/start` 成功後立刻呼叫 `thread/resume` attach observer
- 假設 `thread.id` 已存在就代表 thread 已可 resume
- 對同一 session 同時保留 `live_forwarded` 和 `resume_ws`
- 在沒有 dedupe 規則的情況下讓兩條來源同時寫 `runtime-observer/events.jsonl`
- 把 observer key 建在不穩定的 path normalization 上

最後一點很重要：

- observer key 必須對「路徑是否已存在」不敏感
- 否則 fresh session 註冊 source 時和後續 forward frame 時可能算出不同 key，導致 observer 看起來已註冊，但實際吃不到事件

## 為什麼這麼做

上游 `codex app-server` 的公開協議目前是：

- `thread/start` 會讓發起該 request 的連線自動收到後續 thread / turn / item notifications
- `thread/resume` 用於 resume 可恢復的 thread
- fresh thread 在第一個 user message 前，`thread/resume` 可能直接失敗，錯誤通常是 `no rollout found for thread id ...`

所以 `threadBridge` 現在採用的正確模型是：

- fresh local session：用既有 live stream 做 mirror
- resume session：才使用獨立 observer websocket

## 實際 mirror 寫入點

observer projection 最終會寫到 workspace-local observability surface：

- `.threadbridge/state/runtime-observer/events.jsonl`
- `.threadbridge/state/runtime-observer/sessions/<session>.json`

目前主要事件有：

- `user_prompt_submitted`
- `preview_text`
- `process_transcript`
- `turn_completed`

Telegram / transcript mirror 消費的是這條 workspace-local event lane，而不是直接讀 ingress websocket。

## 常見回歸症狀

### 症狀 1

fresh `hcodex` session 可以開起來，但 Telegram / local mirror 完全沒 preview / final。

優先檢查：

- 是否又回到 fresh `thread/start` 後立刻 `thread/resume`
- `runtime-observer/events.jsonl` 是否完全沒有 `preview_text`
- observer source 是否註冊成了錯的 mode

### 症狀 2

log 裡出現：

```text
thread/resume failed: no rollout found for thread id ...
```

優先判斷：

- 這通常不是 thread id 傳錯
- 更常見是對 fresh thread 誤用了 `thread/resume`

### 症狀 3

session 看似已註冊 source，但 observer 收不到 forwarded frame。

優先檢查：

- observer key 的 path normalization 是否穩定
- source registration 用的 `workspace_path` 與 frame forwarding 用的 `workspace_path` 是否會因為 `canonicalize()` 成功/失敗而產生不同 key

### 症狀 4

preview / final 重複，或同一 turn 似乎被 mirror 兩次。

優先檢查：

- 是否同時存在兩個 active source
- 是否偷偷做了 handoff 或雙 attach

## 除錯順序

出問題時，建議先按這個順序看：

1. `hcodex_ingress.session_tracked` log
2. `app_server_observer.source_registered` / `source_rejected_mode_conflict` / `source_closed`
3. workspace 的 `runtime-observer/events.jsonl`
4. session snapshot 的 `observer_attach_mode`
5. 若是 resume 路徑，再看是否有 `app_server_observer.failed`

若要判斷是不是 upstream 協議限制，對照：

- [docs/codex-app-server-ws-protocol.md](codex-app-server-ws-protocol.md)

## 未來可以做，但目前不要做的事

短期內不要做 source handoff。

這裡的 handoff 指的是：

- 一開始用 `live_forwarded`
- 等 rollout materialize 後，中途切到 `resume_ws`

這件事不是不可能，但它需要一起定義：

- 切換時機
- in-flight 事件邊界
- dedupe 規則
- 舊 source 關閉時序

在沒有完整規格之前，維持固定 source 比較安全。

## 維護結論

如果只記住一條規則，請記這條：

- fresh local `hcodex` session 的 mirror 必須跟著現有 live daemon stream 走，不要再用第二條 websocket 對 fresh thread 做 `thread/resume`

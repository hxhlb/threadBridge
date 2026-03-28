# App-Server WS Backend Worker 進度報告（2026-03-28）

## 目前進度

這份報告是 `2026-03-27` 進度快照的後續增量，聚焦 observer contract 收斂。

相較 [app-server-ws-backend-progress-2026-03-27.md](app-server-ws-backend-progress-2026-03-27.md) 中「未完成」的 observer attach 項，本次已完成：

- threadBridge observer attach contract 已從 `thread/resume` 語義過渡到正式 subscribe lifecycle：
  - `threadbridge/subscribeThread`
  - `threadbridge/unsubscribeThread`
- 舊 `threadbridge/observeThread` 已移除（不再作為 compatibility alias）
- observer stop/replace 路徑改為顯式 detach（unsubscribe）優先，abort 作為 timeout fallback

## 合約狀態（2026-03-28）

- threadBridge 對 observer 的正式 contract：已收斂
- worker 對 upstream 的過渡映射：仍使用 `thread/resume`（subscribe）+ `thread/unsubscribe`（detach）
- upstream `thread/subscribe` 原生 API：仍未提供

## 當前邊界

- `app_server_ws_worker` 擁有 worker-local observer method contract 與 read-only gate
- `app_server_observer` 擁有 observer lifecycle（subscribe, consume, graceful detach, timeout fallback）
- `codex.rs` 只消費 worker-local subscribe/unsubscribe contract，不再直接依賴 `observeThread`

## 後續焦點

1. 補齊 observer 與 ingress 並存時的 dedupe 規則（event/record 邊界）。
2. 在 upstream 提供 `thread/subscribe` 後，只調整 worker 內部映射，不破壞 threadBridge observer contract。
3. 持續把 observer contract 對齊 transport-neutral runtime protocol 的 vocabulary。

# Topic Title 狀態欄

## 目前進度

這份 Plan 已部分落地。

目前已實作：

- title 基底優先使用 thread title
- 若 thread title 缺失，回退到 workspace basename
- 已有獨立的「從目前對話自動生成 title」能力，可把生成結果寫回 thread title
- suffix 目前只支持：
  - `· broken`
- background watcher 會在共享 workspace status 變化時更新 title
- threadBridge 管理的 topic 內，新的 rename service message 會 best-effort 清理

目前尚未實作：

- context ratio / ctx%
- adoption 相關額外 title 語義
- 更細緻的更新節流規格
- title source 的顯式 Telegram UX
  - 目前還沒有把「自動生成 title」與「使用 workspace 資料夾名稱作為 title」做成 inline-button 選項

## 現行語義

title 現在承載的是非常少量的 durable runtime state：

- `broken`
  - 目前 binding 已失效，需要 `/repair_session` 或 `/new_session`

`busy` 已從 title 語義移除：

- busy 是短期執行態
- 不再透過 Telegram topic rename 呈現
- 改由 busy gate、`/workspace_info`、以及後續觀測面承接

已退場的舊 suffix：

- `.cli`
- `.cli!`
- `.attach`

這些屬於舊 handoff / viewer 模型，不再是正式 title 語義。

## 渲染規則

目前格式是：

- `<thread-title> · broken`
- `<thread-title>`

若 thread title 不存在，則改用 workspace basename。

也就是說，今天其實已經有兩種 title 基底來源：

- 顯式 thread title
  - 可能來自手動 rename
  - 也可能來自「從對話自動生成 title」
- workspace basename
  - 當 thread title 缺失時，作為穩定 fallback

但目前這兩種來源還沒有被整理成正式的 Telegram control surface。

## 資料來源

目前 title 的正式語義是只看 canonical binding 是否 broken：

- `binding_status=broken`

底層目前仍可能經過這些欄位推導：

- `metadata.session_broken`
- `session-binding.json.session_broken`

也就是說，title 現在不再對齊：

- `current_codex_thread_id` 對應的 active turn 是否正在執行
- 某個本地 live session
- 某個 attach viewer 狀態

## 後續方向

之後若 `hcodex` ingress / adoption contract 進一步收斂，title 還需要再決定是否承載：

- adoption pending
- alternate TUI session 正在 mirror
- context ratio

另外還有一條應獨立收斂的 title base/source control：

- Telegram 應可提供最小 title source picker，而不是只剩 slash command 或隱式 fallback
- v1 可先收斂成兩個 inline-button：
  - `自動生成`
    - 以目前對話內容呼叫既有 title generation flow，並把結果寫回 thread title
  - `使用資料夾名稱`
    - 直接以 workspace 資料夾名稱作為 thread title
- 這兩個按鈕處理的是 title 基底來源，不是 title 狀態 suffix
- `使用資料夾名稱` 的語義是「用目前綁定 workspace 的 basename 當 title」，不是讓使用者再去選一個資料夾
- 若之後還要支持自由輸入自訂名稱，應視為同一條 title control surface 的後續擴充，而不是再開另一份 status plan

但目前不應再把短期 runtime flag 塞進 title，尤其是 `busy` 這類高頻變動狀態。

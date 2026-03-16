# Telegram Markdown 適配草稿

## 問題

`threadBridge` 的最終輸出，目前大多直接把文字送回 Telegram。

但 Telegram 並不是一般的 Markdown 顯示器，它有自己的格式限制與解析規則。這會帶來幾種常見問題：

- 本來在普通 Markdown 裡可讀的內容，到 Telegram 裡格式錯亂
- 程式碼區塊、列表、引用、連結顯示不穩定
- 特殊符號沒有正確 escape，導致訊息發送失敗
- 同一份回覆，在不同 Telegram client 上呈現不一致

所以這個問題本質上不是「要不要支援 Markdown」，而是：

- threadBridge 要不要有一層專門面向 Telegram 的表示適配

## 方向

新增一個 Telegram 表示層，把 assistant 內容從「原始文字」轉成「適合 Telegram 的訊息格式」。

這一層應該負責：

- 控制哪些 Markdown 能保留
- 哪些結構需要降級
- 哪些字元要 escape
- 什麼時候改用純文字而不是格式化訊息

## 目標

### 主要目標

- 降低 Telegram 發送失敗率
- 提高程式碼、列表、引用、連結的穩定顯示品質
- 讓同一份 assistant 回覆在 Telegram 裡更可預測

### 次要目標

- 讓 Codex 不需要知道 Telegram 太多細節
- 把平台差異集中在 bot 端處理
- 為未來 Web App / 多平台輸出保留不同 renderer 的空間

## 建議的心智模型

建議把輸出拆成三層：

- `assistant 原始內容`
  - Codex 最終產生的內容
- `中間表示`
  - 結構化的段落、列表、程式碼區塊、引用、連結
- `Telegram renderer`
  - 轉成 Telegram 可接受的文字與 parse mode

這樣可以避免：

- 讓 Codex 直接為 Telegram Markdown 細節負責
- 在整個程式裡到處手工 escape 字串

## 要處理的顯示元素

### 基本文字

- 普通段落
- 粗體
- 斜體
- 行內 code
- pre/code block

### 結構化內容

- 有序列表
- 無序列表
- block quote
- 小節標題
- 鍵值資訊

### 連結與路徑

- URL
- 本地路徑
- 檔名
- 指令

## 適配策略

### 策略 1：保守 Markdown

只保留 Telegram 最穩定的格式：

- 粗體
- 行內 code
- code block
- 簡單列表

其餘內容降級為純文字。

優點：

- 最穩
- 最容易避免 parse error

缺點：

- 表現力有限

### 策略 2：Telegram MarkdownV2 Renderer

以 Telegram MarkdownV2 為標準做完整 escape 與渲染。

優點：

- 格式能力比較強
- 可以保留較多結構

缺點：

- escape 規則很麻煩
- 一旦有漏掉字元，整段訊息就可能送不出去

### 策略 3：HTML Renderer

使用 Telegram 支援的 HTML parse mode，而不是 Markdown。

優點：

- 某些結構比 MarkdownV2 更直觀
- escape 規則在某些情況下更容易控制

缺點：

- 不是所有結構都好表達
- 一樣需要做平台特化處理

## 建議的初版

初版建議採用：

- 內部先建立簡單的中間表示
- Telegram 端先實作一個保守 renderer
- 預設偏向：
  - 純文字段落
  - 行內 code
  - code block
  - 簡單列表
- 遇到不穩定結構時，寧可降級成純文字

換句話說：

- 初期追求穩定
- 不追求 Telegram 裡的完整 Markdown 表現力

## 與現有功能的關係

這個適配層不只影響普通 assistant 回覆，也會影響：

- preview draft
- tool 產生的文字說明
- 錯誤訊息
- restore / reconnect / reset 等系統提示
- 未來 Web App 中可能重放的消息內容

所以它不應該只是某一個 helper，而應該是一個比較明確的 renderer 邏輯。

## 可能的實作位置

比較合理的位置是在 Telegram runtime 層，而不是 Codex runtime 層。

理由：

- Codex 應該產生平台無關內容
- Telegram renderer 是 UI surface 的責任
- 未來如果要支援 Web App、CLI viewer、其他表現形式，也可以共用中間表示

## 風險

- 如果一開始直接追求完整 MarkdownV2，複雜度會很高
- 如果 renderer 太激進，可能改壞原本文字語意
- 如果沒有中間表示，後續會變成大量字串 escape 與 patch
- preview 與 final message 如果使用不同邏輯，會造成觀感不一致

## 開放問題

- 初版應該用 MarkdownV2 還是 HTML？
- preview draft 要不要也使用同一套 renderer？
- 本地路徑、命令、檔名是否應一律用 monospace？
- 是否要保留 assistant 原始輸出，以便 debug renderer 問題？
- 失敗時是否自動 fallback 到純文字模式？

## 建議的下一步

1. 先列出目前 threadBridge 最常見的輸出結構。
2. 決定 Telegram 初版 renderer 的最小支援集合。
3. 加一層安全 fallback：格式化失敗時自動退回純文字。
4. 把 renderer 收斂成 Telegram runtime 的單一入口，不要分散在各個 message call site。

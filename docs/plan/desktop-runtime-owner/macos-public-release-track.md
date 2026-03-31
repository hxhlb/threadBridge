# macOS Public Release Track

## 目前進度

這份文檔目前是「部分落地」。

目前代碼裡已經有的前置能力：

- `threadbridge_desktop` 已是正式 macOS desktop runtime 入口
- workspace-first runtime、management API、tray menu 與本地 helper 已可支撐日常開發運行
- repo 已有 Rust build/test/lint 基礎命令與既有維運腳本
- bot-local runtime data root 已落地成雙模式契約：
  - debug build 預設 repo-local `./data`
  - release build 預設 `~/Library/Application Support/threadBridge`
- 已新增 `scripts/release_threadbridge.sh`，作為本地 operator 用的 public release pipeline 入口

目前尚未完成：

- 尚未有 CI 自動化承接 public release pipeline
- 尚未完成第一輪真實 RC 演練與 smoke 測試回寫
- 尚未驗證完整 GitHub Release + Homebrew cask 發佈憑證/權限流程
- 尚未有 release branch / RC 退出條件 / 回滾流程的統一規範文檔

## 問題

目前 `threadBridge` 的 desktop runtime 已可用，但「可公開發佈」與「可本地開發」仍是兩條不同成熟度的能力。

若沒有一份固定 release track：

- 發佈節奏會依賴手動記憶，而不是可重複流程
- signing / notarization 會停留在 ad hoc 操作
- DMG 與 Homebrew 可能形成兩套不一致 artifact 語義
- RC 與主線開發難以穩定並行

若繼續把 `local_threadbridge.sh` 與 public release 混為同一路徑：

- 開發中的快速重啟腳本會承擔不必要的分發責任
- release 所需的 codesign / notary / tap publish 前置檢查無法清楚 fail fast
- README 與 operator runbook 會持續混淆「本地 dev helper」與「正式發佈流程」

## 定位

這份文檔定義的是 `threadBridge` 公開 macOS desktop 發佈（RC）路徑。

它處理：

- RC release discipline（branch、freeze、blocker policy、exit criteria）
- `local_threadbridge.sh` 與 `release_threadbridge.sh` 的責任邊界
- public artifact 契約（universal DMG + Homebrew cask）
- signed + notarized 的最低要求
- public app 的資料落點契約（`Application Support/threadBridge`）
- GitHub Release 與 dedicated tap 的分發關係
- 回滾與撤回策略

它不處理：

- App Store 發佈策略
- runtime architecture 主語義重定義
- Telegram adapter 與 runtime control 的功能規格細節

## 發佈目標（固定值）

本計劃固定採用下列目標值：

- release channel: `RC / preview`
- trust level: `signed + notarized`
- macOS architecture: `universal`（`arm64 + x86_64`）
- artifacts: `DMG + Homebrew cask`
- Homebrew 模式: dedicated tap（`qoli/homebrew-threadbridge`）
- desktop app identity:
  - display name: `threadBridge`
  - bundle identifier: `com.qoli.threadbridge`

第一個候選版本的建議版本語義：

- 版本：`0.1.0-rc.1`
- tag：`v0.1.0-rc.1`

## 主體規格

### 1. Release Governance

- 採用 release branch 模式：
  - 從主線切 `release/0.1.0-rc.1`
  - 主線可持續開發新功能
  - release branch 僅接受 blocker fix、release 流程修正、與 release 文檔同步
- release branch 入口條件：
  - 目標功能範圍已凍結
  - 有明確 RC 驗收清單與責任人
- release branch 退出條件：
  - 既定 gate 全數通過
  - 手動 smoke 測試通過
  - 已完成 release notes 與已知限制聲明

### 2. Signing Readiness Audit

- 在任何 release automation 前，先做 signing readiness audit：
  - 確認 Developer ID Application 可供非 Xcode 手動簽名使用
  - 確認 notarization 驗證憑據可供 CLI / CI 使用
  - 確認 Team ID、bundle id、版本注入策略一致
- 即使已有 Xcode 自動管理憑證，也不得假設 CLI/CI 已可直接重用。

### 3. Packaging + Notarization Pipeline

- `scripts/local_threadbridge.sh` 只作為本地開發 helper：
  - 目標是快速 build / bundle / start 最新代碼
  - 文件上不再把它視為 public release 入口
- `scripts/release_threadbridge.sh` 是公開發佈的正式腳本入口：
  - `build`
  - `sign`
  - `dmg`
  - `notarize`
  - `publish`
  - `release`
- `release` 是完整 orchestration path；public release 時不再鼓勵手動拼接零散命令

- pipeline 產物流程固定為：
  1. 產生 universal app bundle
     - app bundle 內需包含 `threadbridge_desktop` 與 `app_server_ws_worker`
  2. 對 app bundle 進行 codesign（hardened runtime）
  3. 建立 DMG
  4. 對 DMG 執行 notarization
  5. staple notarization ticket
  6. verify（codesign / spctl / staple 結果）
- DMG 是 GitHub Release 與 Homebrew 共同引用的單一 canonical binary artifact，不維護第二套獨立二進位。

### 4. Runtime Data Location

- 公開發佈前，bot-local runtime state 不再預設寫入 repo/worktree 下的 `data/`。
- 低層 path contract 與 override precedence 以 [runtime-data-root.md](../runtime-control/runtime-data-root.md) 為準；本節只保留 release gate。
- release build 的正式資料根目錄應遷移到：
  - `~/Library/Application Support/threadBridge/`
- 最低要求：
  - thread metadata、session binding、transcript mirror、debug/event logs、image-state artifacts 都應落在 `Application Support` 內的受管子目錄
  - 不得要求終端使用者從 app bundle 同層或 git worktree 啟動，才能保有正確的持久化狀態
  - 本地開發模式仍可保留 repo-local `data/`，但必須與 public release path 明確區分
- 這是 release gate，不是可選 polish：
  - 若仍依賴 repo-local `data/`，則不得視為 public-ready macOS app bundle

### 5. Distribution Contract

- GitHub Release：
  - 上傳 notarized DMG
  - 上傳 checksum
  - 發布 RC release notes（包含已知限制）
- Homebrew（dedicated tap）：
  - cask 指向 GitHub Release 的 DMG URL
  - cask checksum 與 release artifact 一致
  - 不發佈與 GitHub artifact 不一致的替代包
- release script operator inputs 固定至少包含：
  - `--version`
  - `--notes-file`
  - `--codesign-identity`
  - `--notary-profile`
- notarization 憑證由 macOS Keychain 中已配置好的 notarytool profile 提供；repo 不管理 secrets 檔案

### 6. Rollback / Yank

- 若 RC 出現 blocker：
  - 先更新 release notes 標記問題
  - 必要時撤回對應 cask 版本
  - 發佈修正 RC（例如 `0.1.0-rc.2`）取代問題版本
- 回滾目標是「保留可追溯歷史 + 防止新用戶安裝問題版」，不是刪除所有歷史記錄。

## 驗收標準

RC 發佈前，至少滿足：

- `cargo fmt --check`
- `cargo check`
- `cargo test`
- `cargo clippy --all-targets --all-features -- -D warnings`
- universal artifact 驗證（`arm64 + x86_64`）
- release path 的 runtime state 驗證：
  - 首次啟動會在 `~/Library/Application Support/threadBridge/` 建立資料根目錄
  - repo/worktree 外啟動仍能正確讀寫持久化資料
- codesign 驗證通過
- notarization 成功且 stapled 驗證通過
- 手動 smoke 測試：
  - Apple Silicon 安裝與啟動
  - Intel 安裝與啟動
  - Homebrew install / upgrade / uninstall 基本路徑

## 與其他計劃的關係

- [owner-runtime-contract.md](owner-runtime-contract.md)
  - 本文不重定義 owner/runtime 邊界，只承接 desktop public release 這條執行路徑
- [macos-menubar-thread-manager.md](../management-desktop-surface/macos-menubar-thread-manager.md)
  - 本文以該文檔描述的 desktop product surface 為發布對象，不重複描述 UI/控制面規格
- [runtime-protocol.md](../runtime-control/runtime-protocol.md)
  - 本文不定義 runtime protocol wire semantics；release gate 中涉及的 API 行為以既有 protocol/實作為準

## 開放問題

目前無阻塞文檔落地的關鍵決策空白。後續若策略改動（例如改 stable 首發、改單一發佈管道），應直接更新本文件並同步 registry。

## 建議的下一步

1. 先補齊 release discipline 文檔（本文件）與 `docs/plan/README.md` registry 對齊。
2. 以 `scripts/release_threadbridge.sh` 做第一次真實 RC 演練，確認 codesign / notarize / GitHub / tap 權限與 artifact 內容。
3. 再補 CI/workflow 或 release runbook 自動化，減少 operator 手動步驟。
4. 以 `release/0.1.0-rc.1` 進行首次 RC 演練，將實際差異回寫本文件。

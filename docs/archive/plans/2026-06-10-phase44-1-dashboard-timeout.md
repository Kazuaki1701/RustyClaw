# Phase 44-1: Dashboard タイムアウト調整 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Dashboard Chat の LLM 応答待機タイムアウトを 300 s → 120 s に短縮し、JS 側に自動タイムアウト検知・1回リトライ・応答インメモリキャッシュを追加してユーザー体感速度を向上させる。

**Architecture:** サーバー側は `tokio::select!` のタイムアウト定数を 300 s → 120 s に変更するだけ（ロジック変更なし）。JS 側は `AbortSignal.any()` で手動キャンセル信号と `AbortSignal.timeout(120_000)` を合成し、タイムアウト時は1回だけ自動リトライする。成功したレスポンスを `Map` にキャッシュ（5分 TTL）し、同一メッセージの再送を即時返却する。

**Tech Stack:** Rust (tokio), Vanilla JS (AbortController, AbortSignal.timeout, AbortSignal.any, Map)

---

## ファイルマップ

| 操作 | ファイル | 変更内容 |
|------|---------|---------|
| Modify | `crates/rustyclaw-gateway/src/health.rs` | ① Rust: `CHAT_TIMEOUT_SECS` 定数追加 + タイムアウト値変更 ② JS: `wasManualCancel` フラグ・キャッシュ・`doSendWithRetry()` 追加 |

---

## Task 1: Rust側 — タイムアウト定数の抽出と変更

**Files:**
- Modify: `crates/rustyclaw-gateway/src/health.rs`

現状: `tokio::time::sleep(std::time::Duration::from_secs(300))` がハードコードされている。
目的: 定数として抽出し 120 s に変更することでテスタビリティと可読性を上げる。

- [ ] **Step 1: 定数を追加する**

`health.rs` ファイル冒頭（`type CancelMap = ...` の手前）に以下を追加する:

```rust
/// Dashboard Chat のサーバー側タイムアウト秒数。
/// クライアント側 JS の CHAT_TIMEOUT_MS/1000 と合わせること。
const CHAT_TIMEOUT_SECS: u64 = 120;
```

- [ ] **Step 2: ハードコード値を定数に置き換える**

`POST /chat` ハンドラ内の `tokio::select!` のタイムアウトアームを変更する。

変更前:
```rust
_ = tokio::time::sleep(std::time::Duration::from_secs(300)) => {
    chat_resp = "Error: Request timeout".to_string();
}
```

変更後:
```rust
_ = tokio::time::sleep(std::time::Duration::from_secs(CHAT_TIMEOUT_SECS)) => {
    chat_resp =
        "⚠️ タイムアウト: LLM の応答を受信できませんでした。再度お試しください。"
            .to_string();
}
```

- [ ] **Step 3: Clippy でエラーがないことを確認する**

```bash
cargo clippy -p rustyclaw-gateway 2>&1
```

期待出力: `Finished` が出て warning/error が 0 件。

- [ ] **Step 4: 単体テストを追加する（定数値の確認）**

`health.rs` の `#[cfg(test)]` ブロック（ファイル末尾付近）に以下を追加する:

```rust
#[test]
fn test_chat_timeout_secs_is_reasonable() {
    // 極端に短すぎず（LLM推論に最低30s必要）、長すぎない（5分以内）ことを確認
    assert!(CHAT_TIMEOUT_SECS >= 30, "タイムアウトが短すぎる: {}", CHAT_TIMEOUT_SECS);
    assert!(CHAT_TIMEOUT_SECS <= 300, "タイムアウトが長すぎる: {}", CHAT_TIMEOUT_SECS);
}
```

- [ ] **Step 5: テストを実行して通ることを確認する**

```bash
cargo test -p rustyclaw-gateway test_chat_timeout_secs_is_reasonable 2>&1
```

期待出力: `test tests::test_chat_timeout_secs_is_reasonable ... ok`

- [ ] **Step 6: コミットする**

```bash
git add crates/rustyclaw-gateway/src/health.rs
git commit -m "fix(gateway): Phase 44-1 CHAT_TIMEOUT_SECS 定数を抽出し 300s→120s に変更"
```

---

## Task 2: JS側 — グローバル変数とキャッシュヘルパーの追加

**Files:**
- Modify: `crates/rustyclaw-gateway/src/health.rs` (埋め込み JS 部分)

現状: `let currentAbortController=null;` と `let currentSessionId=null;` のみ。
目的: タイムアウトと手動キャンセルを区別するフラグとレスポンスキャッシュを追加する。

- [ ] **Step 1: グローバル変数ブロックを置き換える**

現在の2行:

```javascript
let currentAbortController=null;
let currentSessionId=null;
```

これを以下の6行に置き換える:

```javascript
let currentAbortController=null;
let currentSessionId=null;
let wasManualCancel=false;
const responseCache=new Map();
const CACHE_TTL_MS=5*60*1000;
const CHAT_TIMEOUT_MS=120_000;
```

- [ ] **Step 2: キャッシュ操作ヘルパーを追加する**

`function setSendButtonState(state){` の直前に以下の2行を追加する:

```javascript
function getCachedResponse(msg){const e=responseCache.get(msg);if(!e)return null;if(Date.now()-e.ts>CACHE_TTL_MS){responseCache.delete(msg);return null;}return e.text;}
function setCachedResponse(msg,text){responseCache.set(msg,{text,ts:Date.now()});}
```

- [ ] **Step 3: Clippy でエラーがないことを確認する**

```bash
cargo clippy -p rustyclaw-gateway 2>&1
```

期待出力: `Finished` が出て warning/error が 0 件。

- [ ] **Step 4: コミットする**

```bash
git add crates/rustyclaw-gateway/src/health.rs
git commit -m "feat(gateway): Phase 44-1 JS キャッシュ変数・wasManualCancel フラグを追加"
```

---

## Task 3: JS側 — `cancelMessage` に `wasManualCancel` フラグをセット

**Files:**
- Modify: `crates/rustyclaw-gateway/src/health.rs` (埋め込み JS 部分)

- [ ] **Step 1: `cancelMessage` の先頭に1行追加する**

変更前:
```javascript
async function cancelMessage(){
  if(currentAbortController){currentAbortController.abort();}
```

変更後:
```javascript
async function cancelMessage(){
  wasManualCancel=true;
  if(currentAbortController){currentAbortController.abort();}
```

- [ ] **Step 2: Clippy でエラーがないことを確認する**

```bash
cargo clippy -p rustyclaw-gateway 2>&1
```

期待出力: `Finished` が出て warning/error が 0 件。

- [ ] **Step 3: コミットする**

```bash
git add crates/rustyclaw-gateway/src/health.rs
git commit -m "feat(gateway): Phase 44-1 cancelMessage に wasManualCancel フラグをセット"
```

---

## Task 4: JS側 — `sendMessage` をリトライ対応版に刷新する

**Files:**
- Modify: `crates/rustyclaw-gateway/src/health.rs` (埋め込み JS 部分)

`sendMessage` をキャッシュチェック・自動タイムアウト・1回リトライ対応の2関数構成に刷新する。

- [ ] **Step 1: `sendMessage` 関数全体を2関数で置き換える**

現在の `async function sendMessage(){...}` 全体を以下の2関数で完全に置き換える:

```javascript
async function sendMessage(){
  const inp=document.getElementById('chatInput');const msg=inp.value.trim();if(!msg)return;
  addBubble(msg,'user');inp.value='';
  const cached=getCachedResponse(msg);
  if(cached){addBubble('⚡ (キャッシュ) '+cached,'ai');return;}
  await doSendWithRetry(msg,inp,0);
}
async function doSendWithRetry(msg,inp,attempt){
  currentSessionId='http-dashboard-'+Date.now()+'-'+Math.floor(Math.random()*0xFFFFFF);
  currentAbortController=new AbortController();
  wasManualCancel=false;
  const signal=AbortSignal.any([currentAbortController.signal,AbortSignal.timeout(CHAT_TIMEOUT_MS)]);
  const lid=addLoading();inp.disabled=true;setSendButtonState('cancel');
  try{
    const r=await fetch('/chat',{method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify({message:msg,session_id:currentSessionId}),signal});
    removeLoading(lid);
    const text=r.ok?await r.text():'エラー: 返答の取得に失敗しました。';
    addBubble(text,'ai');
    if(r.ok)setCachedResponse(msg,text);
  }catch(e){
    removeLoading(lid);
    if(e.name==='AbortError'){
      if(wasManualCancel){
        addBubble('⚠️ 応答を中断しました。','ai');
      }else if(attempt<1){
        addBubble('⏱ タイムアウト。自動再試行中…','ai');
        currentAbortController=null;currentSessionId=null;
        await doSendWithRetry(msg,inp,attempt+1);
        return;
      }else{
        addBubble('⚠️ 再試行もタイムアウトしました。しばらくしてから再度お試しください。','ai');
      }
    }else{addBubble('通信エラー','ai');}
  }finally{
    inp.disabled=false;setSendButtonState('send');inp.focus();
    currentAbortController=null;currentSessionId=null;wasManualCancel=false;
  }
}
```

- [ ] **Step 2: Clippy でエラーがないことを確認する**

```bash
cargo clippy -p rustyclaw-gateway 2>&1
```

期待出力: `Finished` が出て warning/error が 0 件。

- [ ] **Step 3: 全テストが通ることを確認する**

```bash
cargo test -p rustyclaw-gateway 2>&1
```

期待出力: `test result: ok. N passed; 0 failed`

- [ ] **Step 4: コミットする**

```bash
git add crates/rustyclaw-gateway/src/health.rs
git commit -m "feat(gateway): Phase 44-1 sendMessage をキャッシュ・タイムアウト・1回リトライ対応版に刷新"
```

---

## Task 5: 仕様書・タスクリスト更新

**Files:**
- Modify: `docs/specs/06_dashboard_spec.md`
- Modify: `docs/task.md`

- [ ] **Step 1: `docs/specs/06_dashboard_spec.md` のタイムアウト記述を更新する**

ファイル内の「300」または「タイムアウト」に関する記述を探し:
- サーバー側: `300s` → `120s` (`CHAT_TIMEOUT_SECS`)
- クライアント側: JS タイムアウト `CHAT_TIMEOUT_MS = 120_000`、自動リトライ1回、5分キャッシュを追記
- ファイル冒頭の `最終更新日` を `2026-06-10` に更新

- [ ] **Step 2: `docs/task.md` の Phase 44-1 を完了マークにする**

```
- [ ] **Phase 44-1.
```
を
```
- [x] **Phase 44-1.
```
に変更する。

- [ ] **Step 3: ドキュメントをコミットする**

```bash
git add docs/specs/06_dashboard_spec.md docs/task.md
git commit -m "docs(specs): Phase 44-1 Dashboard タイムアウト仕様を 120s・リトライ・キャッシュに更新"
```

---

## Self-Review

- **Spec coverage**: タイムアウト変更 (Task 1)、wasManualCancel (Task 2-3)、リトライ (Task 4)、キャッシュ (Task 4)、ドキュメント (Task 5) すべてカバー済み
- **Placeholder scan**: TBD/TODO なし。全ステップにコードブロックあり
- **Type consistency**: `getCachedResponse`/`setCachedResponse` は Task 2 で定義し Task 4 で使用。`wasManualCancel` は Task 2 で定義し Task 3・4 で使用。一貫している
- **`AbortSignal.any()` 互換性**: Chrome 116+、Firefox 124+。内部 Dashboard 用途なので許容範囲

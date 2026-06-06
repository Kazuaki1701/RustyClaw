# Phase 28: Dashboard 再構築 — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** ダッシュボードをサイバー CSS + 2タブ構成（Monitor / Stats）に全面刷新し、LLM 送受信 Inspector・並行処理状態・トークン使用量時系列統計を追加する。

**Architecture:** `health.rs` の `get_dashboard_html()` を置き換えて Monitor/Stats 2タブ HTML を提供する。バックエンドは `/api/concurrency` 追加・`LlmResponse` にトークンフィールド追加・`usage` テーブル拡張の順で整備し、Stats ページを段階的にライブデータに切り替える。

**Tech Stack:** Rust / tokio / inline HTML+CSS+JS（ビルドステップなし）/ SVG チャート / Google Fonts CDN

---

## ファイルマップ

| ファイル | 変更種別 | Task |
|---|---|---|
| `crates/rustyclaw-gateway/src/health.rs` | 大規模修正（HTML全置換 + 新エンドポイント） | 1・2・3・6 |
| `crates/rustyclaw-gateway/src/lib.rs` | 修正（gmn_sem を HealthServer に渡す・usage 記録） | 3・5 |
| `crates/rustyclaw-providers/src/lib.rs` | 修正（LlmResponse + OpenAiResponse にトークンフィールド追加） | 4 |
| `crates/rustyclaw-storage/src/lib.rs` | 修正（usage テーブル拡張 + 集計クエリ追加） | 5・6 |

---

## Task 1: サイバー CSS + Monitor タブ HTML（メインダッシュボード刷新）

**Files:**
- Modify: `crates/rustyclaw-gateway/src/health.rs`（`get_dashboard_html()` 関数を全置換）

> **参照ファイル**: `docs/superpowers/dashboard-mockup.html` — モックアップの完全な HTML/CSS/JS がある。これを `get_dashboard_html()` に移植する。

**サイバー CSS 追加仕様**（モックアップに上乗せ）:

- スキャンライン: `body::after` で半透明の水平線オーバーレイ
- ネオングロー: 全パネルに `box-shadow` でカラー発光
- グリッチヘッダー: `h1` にアニメーション
- より深い黒背景: `#000810`
- ボーダー半径を小さく: `6px`（現行 `12px`）

- [ ] **Step 1: 現在の health.rs のバックアップライン確認**

```bash
wc -l /mnt/Projects/RustyClaw/crates/rustyclaw-gateway/src/health.rs
grep -n "fn get_dashboard_html\|fn get_stats_html" /mnt/Projects/RustyClaw/crates/rustyclaw-gateway/src/health.rs
```

Expected: `get_dashboard_html` は 248 行目あたりから存在する。`get_stats_html` は存在しない。

- [ ] **Step 2: CSS + Monitor タブ HTML を実装**

`health.rs` の `fn get_dashboard_html() -> String {` から `}` までを以下で**完全置換**する。

```rust
fn get_dashboard_html() -> String {
    r##"<!DOCTYPE html>
<html lang="ja">
<head>
<meta charset="UTF-8">
<meta name="viewport" content="width=device-width, initial-scale=1.0">
<title>RustyClaw — Runtime Controller</title>
<link href="https://fonts.googleapis.com/css2?family=Outfit:wght@300;400;600;800&family=Fira+Code:wght@400;500&display=swap" rel="stylesheet">
<style>
/* ── CYBER VARIABLES ──────────────────────────────────────── */
:root {
  --bg:       #000810;
  --panel-bg: rgba(0, 12, 28, 0.88);
  --border:   rgba(0, 200, 255, 0.12);
  --text:     #e2f4ff;
  --muted:    #4a7a9b;
  --purple:   #bf00ff;
  --blue:     #00d4ff;
  --green:    #00ff9f;
  --cyan:     #00e5ff;
  --amber:    #ffaa00;
  --pink:     #ff006e;
  --red:      #ff2244;
  --term-bg:  #000510;
  --radius:   6px;
}
* { box-sizing: border-box; margin: 0; padding: 0; }

/* ── SCANLINE OVERLAY ─────────────────────────────────────── */
body {
  font-family: 'Outfit', sans-serif;
  background: var(--bg);
  background-image:
    radial-gradient(ellipse at 0% 0%,    rgba(0,80,180,.18) 0, transparent 55%),
    radial-gradient(ellipse at 100% 100%, rgba(120,0,255,.14) 0, transparent 55%);
  color: var(--text);
  height: 100vh;
  display: flex;
  flex-direction: column;
  overflow: hidden;
  position: relative;
}
body::after {
  content: '';
  position: fixed; inset: 0;
  background: repeating-linear-gradient(
    0deg,
    transparent 0px, transparent 3px,
    rgba(0,0,0,.04) 3px, rgba(0,0,0,.04) 4px
  );
  pointer-events: none;
  z-index: 9999;
}

/* ── HEADER ───────────────────────────────────────────────── */
header {
  background: rgba(0,8,20,.92);
  backdrop-filter: blur(16px);
  border-bottom: 1px solid rgba(0,212,255,.15);
  box-shadow: 0 1px 20px rgba(0,212,255,.06);
  padding: 9px 18px;
  display: flex;
  align-items: center;
  gap: 18px;
  flex-shrink: 0;
}
.logo {
  font-size: 17px; font-weight: 800; letter-spacing: .02em;
  color: var(--blue);
  text-shadow: 0 0 12px rgba(0,212,255,.6), 0 0 30px rgba(0,212,255,.3);
  white-space: nowrap;
  position: relative;
}
.logo::before {
  content: attr(data-text);
  position: absolute; inset: 0;
  color: var(--pink);
  text-shadow: 0 0 8px var(--pink);
  clip-path: inset(40% 0 45% 0);
  animation: glitch 6s infinite;
}
@keyframes glitch {
  0%,90%,100% { clip-path: inset(100% 0 0 0); transform: translate(0); }
  91% { clip-path: inset(30% 0 60% 0); transform: translate(-2px, 0); opacity:.7; }
  93% { clip-path: inset(60% 0 20% 0); transform: translate(2px, 0); opacity:.9; }
  95% { clip-path: inset(10% 0 80% 0); transform: translate(-1px, 0); opacity:.6; }
}

/* ── TABS ─────────────────────────────────────────────────── */
.tabs {
  display: flex; gap: 2px;
  background: rgba(0,212,255,.04);
  padding: 3px; border-radius: var(--radius);
  border: 1px solid rgba(0,212,255,.1);
}
.tab {
  padding: 5px 18px; border-radius: 4px;
  font-size: 12px; font-weight: 700; letter-spacing: .05em;
  cursor: pointer; border: none; background: transparent;
  color: var(--muted); font-family: 'Fira Code', monospace;
  transition: all .15s;
}
.tab.active {
  background: rgba(0,212,255,.12);
  color: var(--cyan);
  border: 1px solid rgba(0,212,255,.25);
  box-shadow: 0 0 10px rgba(0,212,255,.15);
}
.header-right { margin-left: auto; display: flex; align-items: center; gap: 12px; }
.status-badge {
  display: flex; align-items: center; gap: 6px;
  font-size: 11px; font-weight: 700; letter-spacing: .06em;
  color: var(--green);
  background: rgba(0,255,159,.07);
  padding: 4px 12px; border-radius: 20px;
  border: 1px solid rgba(0,255,159,.2);
  box-shadow: 0 0 10px rgba(0,255,159,.1);
  font-family: 'Fira Code', monospace;
}
.status-dot {
  width: 6px; height: 6px;
  background: var(--green);
  border-radius: 50%;
  box-shadow: 0 0 6px var(--green);
  animation: pulse 1.5s infinite;
}
@keyframes pulse {
  0%  { transform:scale(.9); box-shadow: 0 0 0 0 rgba(0,255,159,.6); }
  70% { transform:scale(1);  box-shadow: 0 0 0 6px rgba(0,255,159,0); }
  100%{ transform:scale(.9); box-shadow: 0 0 0 0 rgba(0,255,159,0); }
}

/* ── PAGE WRAPPER ─────────────────────────────────────────── */
.page { display: none; flex: 1; overflow: hidden; min-height: 0; }
.page.active { display: flex; flex-direction: column; }

/* ═══════════════════════════════════════════════════════════
   MONITOR LAYOUT  (3 rows)
═══════════════════════════════════════════════════════════ */
.monitor-grid {
  flex: 1;
  display: grid;
  grid-template-rows: 130px 1fr 1fr;
  gap: 8px;
  padding: 8px 10px;
  overflow: hidden;
  min-height: 0;
}
.row1 { display: grid; grid-template-columns: 2.2fr 1fr 1fr; gap: 8px; }
.row2 { display: grid; grid-template-columns: 1fr 1fr;      gap: 8px; min-height: 0; }
.row3 { display: grid; grid-template-columns: 3fr 2fr;      gap: 8px; min-height: 0; }

/* ── PANEL BASE ───────────────────────────────────────────── */
.panel {
  background: var(--term-bg);
  border: 1px solid var(--border);
  border-radius: var(--radius);
  display: flex; flex-direction: column;
  overflow: hidden; min-height: 0;
  transition: box-shadow .3s;
}
.panel.queue      { border-color: rgba(255,0,110,.25); box-shadow: 0 0 12px rgba(255,0,110,.06), inset 0 0 20px rgba(255,0,110,.02); }
.panel.concur     { border-color: rgba(0,212,255,.25); box-shadow: 0 0 12px rgba(0,212,255,.06), inset 0 0 20px rgba(0,212,255,.02); }
.panel.neurons    { border-color: rgba(0,229,255,.25); box-shadow: 0 0 12px rgba(0,229,255,.06), inset 0 0 20px rgba(0,229,255,.02); }
.panel.request    { border-color: rgba(191,0,255,.25); box-shadow: 0 0 12px rgba(191,0,255,.06), inset 0 0 20px rgba(191,0,255,.02); }
.panel.response   { border-color: rgba(0,255,159,.25); box-shadow: 0 0 12px rgba(0,255,159,.06), inset 0 0 20px rgba(0,255,159,.02); }
.panel.applog     { border-color: rgba(0,212,255,.2);  box-shadow: 0 0 12px rgba(0,212,255,.04); }
.panel.chat-panel { border-color: rgba(191,0,255,.2);  box-shadow: 0 0 12px rgba(191,0,255,.06); background: rgba(0,12,28,.9); }

.panel-head {
  padding: 5px 12px;
  border-bottom: 1px solid rgba(255,255,255,.05);
  display: flex; align-items: center; justify-content: space-between;
  flex-shrink: 0;
  background: rgba(0,0,0,.3);
}
.panel-label {
  font-size: 10px; font-weight: 700; letter-spacing: .1em;
  font-family: 'Fira Code', monospace;
}
.panel.queue    .panel-label { color: var(--pink);   text-shadow: 0 0 8px rgba(255,0,110,.5); }
.panel.concur   .panel-label { color: var(--blue);   text-shadow: 0 0 8px rgba(0,212,255,.5); }
.panel.neurons  .panel-label { color: var(--cyan);   text-shadow: 0 0 8px rgba(0,229,255,.5); }
.panel.request  .panel-label { color: var(--purple); text-shadow: 0 0 8px rgba(191,0,255,.5); }
.panel.response .panel-label { color: var(--green);  text-shadow: 0 0 8px rgba(0,255,159,.5); }
.panel.applog   .panel-label { color: var(--cyan);   text-shadow: 0 0 8px rgba(0,212,255,.4); }
.panel.chat-panel .panel-label { color: var(--purple); text-shadow: 0 0 8px rgba(191,0,255,.5); }

.panel-body {
  flex: 1; padding: 8px 10px;
  overflow-y: auto; min-height: 0;
  font-family: 'Fira Code', monospace;
  font-size: 11px; line-height: 1.7;
}
.refresh-ts { font-size: 9px; color: var(--muted); font-family: 'Fira Code', monospace; }

/* ── QUEUE items ─────────────────────────────────────────── */
.q-item {
  display: flex; align-items: center; gap: 7px;
  padding: 4px 0; font-size: 11.5px;
  border-bottom: 1px solid rgba(255,255,255,.03);
}
.q-item:last-child { border-bottom: none; }
.q-pill {
  font-size: 9px; font-weight: 700; padding: 2px 6px;
  border-radius: 3px; letter-spacing: .06em; flex-shrink: 0;
}
.pill-exec { background:rgba(0,255,159,.12); color:var(--green); border:1px solid rgba(0,255,159,.3); box-shadow:0 0 6px rgba(0,255,159,.2); }
.pill-wait { background:rgba(0,212,255,.10); color:var(--blue);  border:1px solid rgba(0,212,255,.3); box-shadow:0 0 6px rgba(0,212,255,.15); }
.pill-cool { background:rgba(255,170,0,.10); color:var(--amber); border:1px solid rgba(255,170,0,.3); box-shadow:0 0 6px rgba(255,170,0,.15); }
.q-sid  { color: var(--text); font-size: 11px; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; max-width: 200px; }
.q-desc { color: var(--muted); font-size: 10px; margin-left: auto; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; max-width: 160px; }
.q-time { color: var(--muted); font-size: 10px; flex-shrink: 0; }
.cool-bar { height: 3px; background: rgba(255,255,255,.05); border-radius: 2px; margin-top: 2px; overflow: hidden; }
.cool-fill { height: 100%; border-radius: 2px; background: linear-gradient(90deg, var(--amber), #ff6600); box-shadow: 0 0 4px var(--amber); transition: width .5s; }

/* ── CONCURRENCY ─────────────────────────────────────────── */
.slot-row { display: flex; gap: 4px; margin: 6px 0 8px; }
.slot { flex: 1; height: 16px; border-radius: 3px; border: 1px solid rgba(0,212,255,.15); background: rgba(255,255,255,.03); }
.slot.active { background: rgba(0,212,255,.25); border-color: var(--blue); box-shadow: 0 0 6px rgba(0,212,255,.4); }
.stat-line { display:flex; justify-content:space-between; align-items:center; padding:3px 0; border-bottom:1px solid rgba(255,255,255,.03); font-size:11.5px; }
.stat-line:last-child { border-bottom: none; }
.sk { color: var(--muted); }
.sv { color: var(--text); font-weight: 600; }
.sv.ok   { color: var(--green); text-shadow: 0 0 8px rgba(0,255,159,.4); }
.sv.busy { color: var(--amber); text-shadow: 0 0 8px rgba(255,170,0,.4); }
.sv.warn { color: var(--pink);  text-shadow: 0 0 8px rgba(255,0,110,.4); }

/* ── NEURONS ─────────────────────────────────────────────── */
.n-gauge { margin: 6px 0 8px; height: 6px; background: rgba(255,255,255,.06); border-radius: 3px; overflow: hidden; }
.n-fill  { height: 100%; border-radius: 3px; background: linear-gradient(90deg, var(--cyan), var(--blue)); box-shadow: 0 0 6px var(--cyan); transition: width .5s; }

/* ── LLM INSPECTOR ───────────────────────────────────────── */
.msg-block { margin-bottom: 8px; }
.msg-role {
  font-size: 9px; font-weight: 700; letter-spacing: .08em;
  padding: 2px 8px; border-radius: 3px; display: inline-block; margin-bottom: 4px;
}
.role-sys  { background:rgba(191,0,255,.12); color:var(--purple); border:1px solid rgba(191,0,255,.2); }
.role-user { background:rgba(0,212,255,.10); color:var(--blue);   border:1px solid rgba(0,212,255,.2); }
.role-asst { background:rgba(0,255,159,.10); color:var(--green);  border:1px solid rgba(0,255,159,.2); }
.msg-content {
  color: rgba(180,210,230,.7); font-size: 11px; line-height: 1.6;
  border-left: 2px solid rgba(255,255,255,.06); padding-left: 8px;
  white-space: pre-wrap; word-break: break-all;
}

/* ── APP LOG ─────────────────────────────────────────────── */
.log-line { display:flex; gap:8px; align-items:baseline; padding:1px 0; }
.log-ts  { color:var(--muted); font-size:10px; flex-shrink:0; }
.log-lv  { font-size:9px; font-weight:700; flex-shrink:0; letter-spacing:.04em; }
.log-lv.info  { color:var(--cyan);  }
.log-lv.warn  { color:var(--amber); text-shadow:0 0 6px rgba(255,170,0,.5); }
.log-lv.error { color:var(--red);   text-shadow:0 0 6px rgba(255,34,68,.5); }
.log-msg { color: var(--text); font-size:10.5px; word-break:break-all; }

/* ── CHAT ────────────────────────────────────────────────── */
.chat-msgs { flex:1; overflow-y:auto; padding:10px; display:flex; flex-direction:column; gap:8px; min-height:0; }
.bubble { max-width:86%; padding:8px 12px; border-radius:var(--radius); font-size:13px; line-height:1.55; font-family:'Outfit',sans-serif; }
.bubble.user {
  align-self:flex-end;
  background: rgba(191,0,255,.12);
  border:1px solid rgba(191,0,255,.2);
  border-bottom-right-radius: 1px;
  box-shadow: 0 0 10px rgba(191,0,255,.08);
}
.bubble.ai {
  align-self:flex-start;
  background: rgba(0,212,255,.06);
  border:1px solid rgba(0,212,255,.12);
  border-bottom-left-radius: 1px;
}
.chat-in-area { padding:8px 10px; border-top:1px solid rgba(255,255,255,.05); display:flex; gap:6px; flex-shrink:0; }
.chat-in {
  flex:1; background:rgba(0,0,0,.4);
  border:1px solid rgba(0,212,255,.15);
  border-radius:var(--radius); padding:7px 12px;
  color:var(--text); font-size:13px; font-family:'Outfit',sans-serif; outline:none;
  transition: border-color .2s, box-shadow .2s;
}
.chat-in:focus { border-color: rgba(0,212,255,.45); box-shadow: 0 0 8px rgba(0,212,255,.15); }
.send-btn {
  background: linear-gradient(135deg, rgba(191,0,255,.7), rgba(0,212,255,.7));
  border: none; border-radius:var(--radius); color:#fff;
  padding: 0 16px; font-size:13px; font-weight:700; cursor:pointer;
  box-shadow: 0 0 12px rgba(0,212,255,.2);
  transition: opacity .2s, box-shadow .2s;
  font-family: 'Fira Code', monospace; letter-spacing: .04em;
}
.send-btn:hover { opacity:.88; box-shadow: 0 0 20px rgba(0,212,255,.35); }
.loading-dots { display:flex; gap:4px; padding:10px 14px; align-self:flex-start; }
.dot { width:5px;height:5px;background:var(--muted);border-radius:50%;animation:blink 1.4s infinite both; }
.dot:nth-child(2){animation-delay:.2s}.dot:nth-child(3){animation-delay:.4s}
@keyframes blink{0%,80%,100%{opacity:.2}40%{opacity:1}}

/* ── SCROLLBAR ───────────────────────────────────────────── */
::-webkit-scrollbar{width:4px;height:4px}
::-webkit-scrollbar-track{background:transparent}
::-webkit-scrollbar-thumb{background:rgba(0,212,255,.15);border-radius:2px}
::-webkit-scrollbar-thumb:hover{background:rgba(0,212,255,.3)}

/* ═══════════════════════════════════════════════════════════
   STATS PAGE
═══════════════════════════════════════════════════════════ */
.stats-layout { flex:1; display:flex; flex-direction:column; gap:8px; padding:10px; overflow-y:auto; min-height:0; }
.stats-top-bar { display:flex; align-items:center; justify-content:space-between; }
.stats-title { font-size:11px; color:var(--muted); font-family:'Fira Code',monospace; letter-spacing:.08em; }
.period-bar { display:flex; gap:3px; }
.period-btn {
  padding:4px 14px; border-radius:4px; font-size:11px; font-weight:700;
  cursor:pointer; border:1px solid rgba(255,255,255,.08);
  background:transparent; color:var(--muted); font-family:'Fira Code',monospace;
  transition: all .15s;
}
.period-btn.active { background:rgba(0,212,255,.12); color:var(--blue); border-color:rgba(0,212,255,.3); box-shadow:0 0 8px rgba(0,212,255,.12); }

.kpi-row { display:grid; grid-template-columns:repeat(4,1fr); gap:8px; }
.kpi {
  background: var(--panel-bg);
  border: 1px solid var(--border);
  border-radius: var(--radius);
  padding: 12px 14px;
}
.kpi.runs   { border-color:rgba(0,212,255,.25); box-shadow:0 0 10px rgba(0,212,255,.06); }
.kpi.tokens { border-color:rgba(191,0,255,.25); box-shadow:0 0 10px rgba(191,0,255,.06); }
.kpi.avg    { border-color:rgba(0,255,159,.25); box-shadow:0 0 10px rgba(0,255,159,.06); }
.kpi.cf     { border-color:rgba(0,229,255,.25); box-shadow:0 0 10px rgba(0,229,255,.06); }
.kpi-lbl { font-size:10px; color:var(--muted); font-weight:700; letter-spacing:.06em; font-family:'Fira Code',monospace; }
.kpi-val { font-size:24px; font-weight:800; margin:4px 0 2px; }
.kpi.runs   .kpi-val { color:var(--blue);   text-shadow:0 0 12px rgba(0,212,255,.4); }
.kpi.tokens .kpi-val { color:var(--purple); text-shadow:0 0 12px rgba(191,0,255,.4); }
.kpi.avg    .kpi-val { color:var(--green);  text-shadow:0 0 12px rgba(0,255,159,.4); }
.kpi.cf     .kpi-val { color:var(--cyan);   text-shadow:0 0 12px rgba(0,229,255,.4); }
.kpi-sub { font-size:10.5px; color:var(--muted); }

.chart-wrap {
  background: var(--term-bg);
  border: 1px solid rgba(191,0,255,.2);
  border-radius: var(--radius);
  padding: 12px 14px;
  box-shadow: 0 0 12px rgba(191,0,255,.05);
}
.chart-lbl { font-size:10px; font-weight:700; color:var(--purple); letter-spacing:.08em; font-family:'Fira Code',monospace; margin-bottom:8px; }
.x-axis { display:flex; justify-content:space-between; margin-top:4px; }
.x-axis span { font-size:9px; color:var(--muted); font-family:'Fira Code',monospace; }
.chart-legend { display:flex; gap:14px; margin-top:6px; }
.leg-item { display:flex; align-items:center; gap:5px; font-size:10px; color:var(--muted); font-family:'Fira Code',monospace; }
.leg-dot { width:8px; height:3px; border-radius:1px; }

.stats-bottom { display:grid; grid-template-columns:1fr 1fr; gap:8px; }
.breakdown {
  background: var(--term-bg);
  border: 1px solid var(--border);
  border-radius: var(--radius);
  padding: 12px 14px;
}
.bd-title { font-size:10px; font-weight:700; letter-spacing:.08em; font-family:'Fira Code',monospace; margin-bottom:8px; }
.bd-row { display:flex; align-items:center; gap:8px; padding:4px 0; border-bottom:1px solid rgba(255,255,255,.03); font-size:11px; }
.bd-row:last-child { border-bottom:none; }
.bd-name { width:150px; font-family:'Fira Code',monospace; font-size:10.5px; overflow:hidden; text-overflow:ellipsis; white-space:nowrap; }
.bd-bar-bg { flex:1; height:5px; background:rgba(255,255,255,.06); border-radius:2px; overflow:hidden; }
.bd-bar    { height:100%; border-radius:2px; }
.bd-cnt { font-size:10px; color:var(--muted); text-align:right; min-width:55px; }
</style>
</head>
<body>

<!-- HEADER -->
<header>
  <span class="logo" data-text="🐈‍⬛ RustyClaw">🐈‍⬛ RustyClaw</span>
  <nav class="tabs">
    <button class="tab active" onclick="switchTab('monitor',this)">MONITOR</button>
    <button class="tab"        onclick="switchTab('stats',  this)">STATS</button>
  </nav>
  <div class="header-right">
    <span style="font-size:10px;color:var(--muted);font-family:'Fira Code',monospace">:8080</span>
    <div class="status-badge"><span class="status-dot"></span>ACTIVE</div>
  </div>
</header>

<!-- ── MONITOR ── -->
<div id="page-monitor" class="page active">
<div class="monitor-grid">

  <!-- Row 1 -->
  <div class="row1">
    <!-- Queue -->
    <div class="panel queue">
      <div class="panel-head">
        <span class="panel-label">◈ LANE QUEUE</span>
        <span class="refresh-ts" id="queue-ts">—</span>
      </div>
      <div class="panel-body" id="queuePanel" style="padding:6px 8px;">
        <div style="color:var(--muted);text-align:center;padding:10px;font-family:'Fira Code',monospace;font-size:11px;">キューは空（稼働タスクなし）</div>
      </div>
    </div>
    <!-- Concurrency -->
    <div class="panel concur">
      <div class="panel-head">
        <span class="panel-label">◈ CONCURRENCY</span>
        <span class="refresh-ts" id="concur-ts">—</span>
      </div>
      <div class="panel-body" id="concurPanel">
        <div class="slot-row" id="slotRow"></div>
        <div class="stat-line"><span class="sk">Active</span><span class="sv" id="cActive">—</span></div>
        <div class="stat-line"><span class="sk">Queue depth</span><span class="sv" id="cDepth">—</span></div>
        <div class="stat-line"><span class="sk">Cooldown</span><span class="sv" id="cCool">—</span></div>
        <div class="stat-line"><span class="sk">Global limit</span><span class="sv" id="cGlobal">—</span></div>
      </div>
    </div>
    <!-- Neurons -->
    <div class="panel neurons">
      <div class="panel-head">
        <span class="panel-label">◈ CF NEURONS</span>
        <span class="refresh-ts" id="neuron-ts">—</span>
      </div>
      <div class="panel-body" id="neuronPanel">
        <div class="n-gauge"><div class="n-fill" id="nFill" style="width:0%"></div></div>
        <div class="stat-line"><span class="sk">Today</span><span class="sv" id="nToday" style="color:var(--cyan)">—</span></div>
        <div class="stat-line"><span class="sk">Limit</span><span class="sv">10,000</span></div>
        <div class="stat-line"><span class="sk">Remaining</span><span class="sv ok" id="nRem">—</span></div>
        <div class="stat-line"><span class="sk">Usage</span><span class="sv" id="nPct">—</span></div>
      </div>
    </div>
  </div>

  <!-- Row 2: LLM Inspector -->
  <div class="row2">
    <div class="panel request">
      <div class="panel-head">
        <span class="panel-label">◈ LLM REQUEST</span>
        <span class="refresh-ts" id="req-ts">—</span>
      </div>
      <div class="panel-body" id="reqPanel" style="white-space:pre-wrap;word-break:break-all;font-size:10.5px;color:rgba(180,210,230,.65);">読み込み中...</div>
    </div>
    <div class="panel response">
      <div class="panel-head">
        <span class="panel-label">◈ LLM RESPONSE</span>
        <span class="refresh-ts" id="res-ts">—</span>
      </div>
      <div class="panel-body" id="resPanel" style="white-space:pre-wrap;word-break:break-all;font-size:10.5px;color:rgba(0,255,159,.7);">読み込み中...</div>
    </div>
  </div>

  <!-- Row 3: AppLog + Chat -->
  <div class="row3">
    <div class="panel applog">
      <div class="panel-head">
        <span class="panel-label">◈ APP LOG</span>
        <span class="refresh-ts">↻ 2s</span>
      </div>
      <div class="panel-body" id="appLog">読み込み中...</div>
    </div>
    <div class="panel chat-panel">
      <div class="panel-head">
        <span class="panel-label">◈ CHAT</span>
        <span style="font-size:9px;color:var(--muted);font-family:'Fira Code',monospace;">http-dashboard</span>
      </div>
      <div class="chat-msgs" id="chatMessages">
        <div class="bubble ai">こんにちは。RustyClaw Dashboard です。何かお手伝いできますか？</div>
      </div>
      <div class="chat-in-area">
        <input class="chat-in" id="chatInput" type="text" placeholder="メッセージを入力..." onkeydown="handleKey(event)">
        <button class="send-btn" id="sendBtn" onclick="sendMessage()">SEND</button>
      </div>
    </div>
  </div>

</div><!-- /monitor-grid -->
</div><!-- /page-monitor -->

<!-- ── STATS ── -->
<div id="page-stats" class="page">
<div class="stats-layout">
  <div class="stats-top-bar">
    <span class="stats-title">TOKEN USAGE STATISTICS</span>
    <div class="period-bar">
      <button class="period-btn" onclick="setPeriod(7, this)">7D</button>
      <button class="period-btn active" onclick="setPeriod(30, this)">30D</button>
      <button class="period-btn" onclick="setPeriod(0, this)">ALL</button>
    </div>
  </div>
  <div class="kpi-row">
    <div class="kpi runs">
      <div class="kpi-lbl">TOTAL RUNS</div>
      <div class="kpi-val" id="kpiRuns">—</div>
      <div class="kpi-sub" id="kpiRunsSub">loading...</div>
    </div>
    <div class="kpi tokens">
      <div class="kpi-lbl">TOTAL TOKENS</div>
      <div class="kpi-val" id="kpiTokens">—</div>
      <div class="kpi-sub" id="kpiTokensSub">loading...</div>
    </div>
    <div class="kpi avg">
      <div class="kpi-lbl">AVG / RUN</div>
      <div class="kpi-val" id="kpiAvg">—</div>
      <div class="kpi-sub">tokens per execution</div>
    </div>
    <div class="kpi cf">
      <div class="kpi-lbl">CF NEURONS TODAY</div>
      <div class="kpi-val" id="kpiCf">—</div>
      <div class="kpi-sub" id="kpiCfSub">loading...</div>
    </div>
  </div>
  <div class="chart-wrap">
    <div class="chart-lbl">DAILY TOKEN USAGE</div>
    <svg id="timelineChart" viewBox="0 0 900 130" width="100%" height="130" xmlns="http://www.w3.org/2000/svg" preserveAspectRatio="none">
      <text x="4" y="20" fill="#1e3a5f" font-size="8" font-family="Fira Code">—</text>
    </svg>
    <div class="x-axis" id="chartXAxis"></div>
    <div class="chart-legend">
      <div class="leg-item"><div class="leg-dot" style="background:#bf00ff"></div>Input</div>
      <div class="leg-item"><div class="leg-dot" style="background:#00d4ff"></div>Output</div>
    </div>
  </div>
  <div class="stats-bottom">
    <div class="breakdown" style="border-color:rgba(191,0,255,.2);">
      <div class="bd-title" style="color:var(--purple);">BY MODEL</div>
      <div id="modelBreakdown"></div>
    </div>
    <div class="breakdown" style="border-color:rgba(0,229,255,.2);">
      <div class="bd-title" style="color:var(--cyan);">BY TRIGGER</div>
      <div id="triggerBreakdown"></div>
    </div>
  </div>
</div><!-- /stats-layout -->
</div><!-- /page-stats -->

<script>
// ── タブ切り替え ──────────────────────────────────────────────
function switchTab(id, btn) {
  document.querySelectorAll('.page').forEach(p => p.classList.remove('active'));
  document.querySelectorAll('.tab').forEach(t => t.classList.remove('active'));
  document.getElementById('page-' + id).classList.add('active');
  btn.classList.add('active');
  if (id === 'stats') loadStats();
}

// ── ユーティリティ ────────────────────────────────────────────
function fmtK(n) {
  if (n >= 1e6) return (n/1e6).toFixed(2) + 'M';
  if (n >= 1e3) return (n/1e3).toFixed(1) + 'k';
  return n.toString();
}
function now() { return new Date().toLocaleTimeString('ja-JP', {hour:'2-digit',minute:'2-digit',second:'2-digit'}); }

// ── Queue ─────────────────────────────────────────────────────
async function updateQueue() {
  try {
    const r = await fetch('/api/queue');
    if (!r.ok) return;
    const items = await r.json();
    document.getElementById('queue-ts').textContent = '↻ ' + now();
    const panel = document.getElementById('queuePanel');
    if (items.length === 0) {
      panel.innerHTML = '<div style="color:var(--muted);text-align:center;padding:10px;font-family:\'Fira Code\',monospace;font-size:11px;">キューは空（稼働タスクなし）</div>';
      return;
    }
    let html = '';
    items.forEach((item, i) => {
      const cls = item.status === 'Executing' ? 'pill-exec' : item.status === 'Waiting' ? 'pill-wait' : 'pill-cool';
      const lbl = item.status === 'Executing' ? 'EXEC' : item.status === 'Waiting' ? 'WAIT' : 'COOL';
      const elapsed = Math.floor((Date.now() - item.enqueued_at_ms) / 1000);
      html += `<div class="q-item">
        <span class="q-pill ${cls}">${lbl}</span>
        <span class="q-sid">${item.session_id}</span>
        <span class="q-desc">${item.description||''}</span>
        <span class="q-time">${elapsed}s</span>
      </div>`;
      if (item.status === 'Cooldown' && item.cooldown_left_secs > 0) {
        const pct = Math.min(100, (item.cooldown_left_secs / 60) * 100);
        html += `<div class="cool-bar"><div class="cool-fill" style="width:${pct}%"></div></div>`;
      }
    });
    panel.innerHTML = html;
  } catch { /* ignore */ }
}

// ── Concurrency ───────────────────────────────────────────────
async function updateConcurrency() {
  try {
    const r = await fetch('/api/concurrency');
    if (!r.ok) return;
    const d = await r.json();
    document.getElementById('concur-ts').textContent = '↻ ' + now();
    const slots = document.getElementById('slotRow');
    slots.innerHTML = '';
    for (let i = 0; i < d.capacity; i++) {
      const s = document.createElement('div');
      s.className = 'slot' + (i < d.active ? ' active' : '');
      slots.appendChild(s);
    }
    document.getElementById('cActive').textContent = d.active + ' / ' + d.capacity;
    document.getElementById('cActive').className = 'sv ' + (d.active >= d.capacity ? 'busy' : 'ok');
    document.getElementById('cDepth').textContent = d.queue_depth;
    document.getElementById('cCool').textContent  = d.cooldown_secs > 0 ? d.cooldown_secs.toFixed(1) + 's' : 'none';
    document.getElementById('cCool').className    = 'sv ' + (d.cooldown_secs > 0 ? 'warn' : 'ok');
    document.getElementById('cGlobal').textContent = d.global_cooldown ? d.global_cooldown.toFixed(1) + 's' : 'none';
    document.getElementById('cGlobal').className  = 'sv ' + (d.global_cooldown ? 'warn' : 'ok');
  } catch { /* ignore */ }
}

// ── Neurons ───────────────────────────────────────────────────
async function updateNeurons() {
  try {
    const r = await fetch('/api/neurons');
    if (!r.ok) return;
    const d = await r.json();
    document.getElementById('neuron-ts').textContent = '↻ ' + now();
    const today = d.today_used ?? 0;
    const limit = 10000;
    const pct = Math.min(100, (today / limit) * 100).toFixed(1);
    document.getElementById('nFill').style.width = pct + '%';
    document.getElementById('nToday').textContent = today.toLocaleString();
    document.getElementById('nRem').textContent   = (limit - today).toLocaleString();
    document.getElementById('nPct').textContent   = pct + '%';
  } catch { /* ignore */ }
}

// ── LLM Inspector ────────────────────────────────────────────
async function updateInspector() {
  try {
    const [rq, rs] = await Promise.all([fetch('/debug/request'), fetch('/debug/response')]);
    const ts = now();
    if (rq.ok) {
      const txt = await rq.text();
      document.getElementById('req-ts').textContent = ts;
      document.getElementById('reqPanel').textContent = txt.length > 4000 ? txt.substring(0, 4000) + '\n...(truncated)' : txt;
    }
    if (rs.ok) {
      const txt = await rs.text();
      document.getElementById('res-ts').textContent = ts;
      document.getElementById('resPanel').textContent = txt.length > 3000 ? txt.substring(0, 3000) + '\n...(truncated)' : txt;
    }
  } catch { /* ignore */ }
}

// ── App Log ───────────────────────────────────────────────────
async function updateLog() {
  try {
    const r = await fetch('/logs/app');
    if (!r.ok) return;
    const txt = await r.text();
    const el = document.getElementById('appLog');
    const atBottom = el.scrollHeight - el.clientHeight <= el.scrollTop + 60;
    const lines = txt.trim().split('\n').slice(-100);
    el.innerHTML = lines.map(line => {
      const lvl = line.includes(' INFO ') ? 'info' : line.includes(' WARN ') ? 'warn' : line.includes(' ERROR ') ? 'error' : 'info';
      const tsMatch = line.match(/\d{4}-\d{2}-\d{2}T(\d{2}:\d{2}:\d{2})/);
      const ts = tsMatch ? tsMatch[1] : '';
      const msg = line.replace(/^\S+\s+(INFO|WARN|ERROR)\s+/, '').trim();
      return `<div class="log-line"><span class="log-ts">${ts}</span><span class="log-lv ${lvl}">${lvl.toUpperCase()}</span><span class="log-msg">${msg}</span></div>`;
    }).join('');
    if (atBottom) el.scrollTop = el.scrollHeight;
  } catch { /* ignore */ }
}

// ── Chat ──────────────────────────────────────────────────────
function handleKey(e) { if (e.key === 'Enter' && !e.shiftKey) { e.preventDefault(); sendMessage(); } }
async function sendMessage() {
  const inp = document.getElementById('chatInput');
  const msg = inp.value.trim();
  if (!msg) return;
  addBubble(msg, 'user');
  inp.value = '';
  const loadId = addLoading();
  inp.disabled = true; document.getElementById('sendBtn').disabled = true;
  try {
    const r = await fetch('/chat', { method:'POST', headers:{'Content-Type':'application/json'}, body: JSON.stringify({ message: msg }) });
    removeLoading(loadId);
    addBubble(r.ok ? await r.text() : 'エラー: 返答の取得に失敗しました。', 'ai');
  } catch { removeLoading(loadId); addBubble('通信エラー', 'ai'); }
  finally { inp.disabled = false; document.getElementById('sendBtn').disabled = false; inp.focus(); }
}
function addBubble(text, role) {
  const d = document.createElement('div');
  d.className = 'bubble ' + role;
  d.innerHTML = text.replace(/\n/g, '<br>');
  const msgs = document.getElementById('chatMessages');
  msgs.appendChild(d);
  msgs.scrollTop = msgs.scrollHeight;
}
function addLoading() {
  const id = 'ld-' + Date.now();
  const el = document.createElement('div');
  el.className = 'loading-dots'; el.id = id;
  el.innerHTML = '<span class="dot"></span><span class="dot"></span><span class="dot"></span>';
  const msgs = document.getElementById('chatMessages');
  msgs.appendChild(el); msgs.scrollTop = msgs.scrollHeight;
  return id;
}
function removeLoading(id) { const el = document.getElementById(id); if (el) el.remove(); }

// ── Stats ─────────────────────────────────────────────────────
let currentPeriodDays = 30;
function setPeriod(days, btn) {
  currentPeriodDays = days;
  document.querySelectorAll('.period-btn').forEach(b => b.classList.remove('active'));
  btn.classList.add('active');
  loadStats();
}

async function loadStats() {
  const since = currentPeriodDays > 0 ? new Date(Date.now() - currentPeriodDays * 86400000).toISOString().slice(0,10) : undefined;
  const qs = since ? '?since=' + since : '';
  try {
    const [rSum, rTl, rTr, rN] = await Promise.all([
      fetch('/api/usage/summary' + qs),
      fetch('/api/usage/timeline' + qs),
      fetch('/api/usage/by-trigger' + qs),
      fetch('/api/neurons'),
    ]);
    if (rSum.ok) renderSummary(await rSum.json());
    if (rTl.ok)  renderTimeline(await rTl.json());
    if (rTr.ok)  renderTriggers(await rTr.json());
    if (rN.ok)   renderNeuronsKpi(await rN.json());
  } catch { /* ignore */ }
}

function renderSummary(d) {
  document.getElementById('kpiRuns').textContent   = (d.total_runs ?? 0).toLocaleString();
  document.getElementById('kpiRunsSub').textContent = 'total executions';
  document.getElementById('kpiTokens').textContent  = fmtK(d.total_tokens ?? 0);
  document.getElementById('kpiTokensSub').textContent = 'input ' + fmtK(d.total_input_tokens??0) + ' / output ' + fmtK(d.total_completion_tokens??0);
  const avg = d.total_runs > 0 ? Math.round((d.total_tokens??0) / d.total_runs) : 0;
  document.getElementById('kpiAvg').textContent = avg.toLocaleString();
  // By model breakdown
  const models = Object.entries(d.by_model ?? {}).sort((a,b) => b[1].tokens - a[1].tokens);
  const totalTok = d.total_tokens || 1;
  const colors = ['#bf00ff','#00d4ff','#00ff9f','#ffaa00','#ff006e'];
  document.getElementById('modelBreakdown').innerHTML = models.slice(0,6).map(([m, v], i) => {
    const pct = Math.round((v.tokens/totalTok)*100);
    return `<div class="bd-row">
      <span class="bd-name" style="color:${colors[i]||'#aaa'}">${m}</span>
      <div class="bd-bar-bg"><div class="bd-bar" style="width:${pct}%;background:${colors[i]||'#aaa'}"></div></div>
      <span class="bd-cnt">${fmtK(v.tokens)}</span>
    </div>`;
  }).join('');
}

function renderNeuronsKpi(d) {
  const today = d.today_used ?? 0;
  document.getElementById('kpiCf').textContent  = today.toLocaleString();
  document.getElementById('kpiCfSub').textContent = ((today/10000)*100).toFixed(1) + '% of 10,000 limit';
}

function renderTimeline(rows) {
  if (!rows.length) return;
  const maxT = Math.max(...rows.map(r => r.total_tokens ?? (r.tokens ?? 0)));
  if (maxT === 0) return;
  const W = 900, H = 120, PAD = 20;
  const xStep = (W - PAD*2) / Math.max(rows.length-1, 1);
  const scale = (v) => PAD + ((H - PAD*2) * (1 - v/maxT));

  const inputPts = rows.map((r,i) => `${PAD + i*xStep},${scale(r.input_tokens??0)}`).join(' ');
  const outPts   = rows.map((r,i) => `${PAD + i*xStep},${scale(r.completion_tokens??0)}`).join(' ');
  const inputArea = 'M' + inputPts.replace(/ /g,' L') + ` L${PAD+(rows.length-1)*xStep},${H} L${PAD},${H} Z`;
  const outArea   = 'M' + outPts.replace(/ /g,' L')   + ` L${PAD+(rows.length-1)*xStep},${H} L${PAD},${H} Z`;

  document.getElementById('timelineChart').innerHTML = `
    <defs>
      <linearGradient id="ig" x1="0" y1="0" x2="0" y2="1"><stop offset="0%" stop-color="#bf00ff" stop-opacity=".35"/><stop offset="100%" stop-color="#bf00ff" stop-opacity=".02"/></linearGradient>
      <linearGradient id="og" x1="0" y1="0" x2="0" y2="1"><stop offset="0%" stop-color="#00d4ff" stop-opacity=".25"/><stop offset="100%" stop-color="#00d4ff" stop-opacity=".02"/></linearGradient>
    </defs>
    <line x1="${PAD}" y1="${PAD}" x2="${PAD}" y2="${H}" stroke="rgba(0,212,255,.1)" stroke-width="1"/>
    <line x1="${PAD}" y1="${H}"  x2="${W-PAD}" y2="${H}" stroke="rgba(0,212,255,.1)" stroke-width="1"/>
    <path d="${inputArea}" fill="url(#ig)"/>
    <polyline points="${inputPts}" fill="none" stroke="#bf00ff" stroke-width="1.5" stroke-linejoin="round"/>
    <path d="${outArea}" fill="url(#og)"/>
    <polyline points="${outPts}" fill="none" stroke="#00d4ff" stroke-width="1.5" stroke-linejoin="round"/>
  `;
  const step = Math.max(1, Math.floor(rows.length / 7));
  document.getElementById('chartXAxis').innerHTML = rows
    .filter((_, i) => i % step === 0 || i === rows.length-1)
    .map(r => `<span>${r.date}</span>`).join('');
}

function renderTriggers(rows) {
  const maxT = Math.max(...rows.map(r => r.tokens ?? 0), 1);
  const colors = ['#bf00ff','#00ff9f','#00d4ff','#ffaa00','#ff006e'];
  document.getElementById('triggerBreakdown').innerHTML = rows.slice(0,6).map((r, i) => {
    const pct = Math.round((r.tokens/maxT)*100);
    return `<div class="bd-row">
      <span class="bd-name" style="color:${colors[i]||'#aaa'}">${r.trigger}</span>
      <div class="bd-bar-bg"><div class="bd-bar" style="width:${pct}%;background:${colors[i]||'#aaa'}"></div></div>
      <span class="bd-cnt">${r.runs} runs</span>
    </div>`;
  }).join('');
}

// ── Polling ───────────────────────────────────────────────────
updateQueue();
updateConcurrency();
updateNeurons();
updateInspector();
updateLog();
setInterval(updateQueue,       1000);
setInterval(updateConcurrency, 1000);
setInterval(updateNeurons,     5000);
setInterval(updateInspector,   2000);
setInterval(updateLog,         2000);
</script>
</body>
</html>
"##.to_string()
}
```

- [ ] **Step 3: cargo check でエラーがないことを確認**

```bash
cargo check -p rustyclaw-gateway 2>&1 | grep "^error" | head -10
```
Expected: エラーなし

- [ ] **Step 4: コミット**

```bash
git add crates/rustyclaw-gateway/src/health.rs
git commit -m "feat(dashboard): cyber CSS + Monitor/Stats 2-tab layout (Phase 28 A)"
```

---

## Task 2: `/api/concurrency` エンドポイント追加（HealthServer に gmn_sem を渡す）

**Files:**
- Modify: `crates/rustyclaw-gateway/src/health.rs`（HealthServer 構造体 + start()）
- Modify: `crates/rustyclaw-gateway/src/lib.rs`（HealthServer::new 呼び出し箇所）

> `gmn_sem.available_permits()` で空きスロット数を取得。`QUEUE_STATE` から queue_depth を算出。

- [ ] **Step 1: HealthServer 構造体に `gmn_sem` フィールドを追加**

`health.rs` の `pub struct HealthServer` を変更:

```rust
pub struct HealthServer {
    addr: SocketAddr,
    reload_tx: tokio::sync::mpsc::Sender<()>,
    bus: Arc<crate::MessageBus>,
    workspace_path: PathBuf,
    gmn_sem: Arc<tokio::sync::Semaphore>,
    gmn_capacity: usize,
}

impl HealthServer {
    pub fn new(
        port: u16,
        reload_tx: tokio::sync::mpsc::Sender<()>,
        bus: Arc<crate::MessageBus>,
        workspace_path: PathBuf,
        gmn_sem: Arc<tokio::sync::Semaphore>,
        gmn_capacity: usize,
    ) -> Self {
        let addr = SocketAddr::from(([0, 0, 0, 0], port));
        Self { addr, reload_tx, bus, workspace_path, gmn_sem, gmn_capacity }
    }
    // ...
```

- [ ] **Step 2: start() 内で gmn_sem を Arc でクローンして tokio::spawn に渡す**

`health.rs` の `pub async fn start(self) -> Result<()>` 内に追加:

```rust
        let gmn_sem_arc = Arc::new(self.gmn_sem.clone());
        let gmn_capacity = self.gmn_capacity;
```

`tokio::spawn(async move {` のクロージャ先頭で:
```rust
                        let gmn_sem_clone = gmn_sem_arc.clone();
```

- [ ] **Step 3: `/api/concurrency` エンドポイントを `health.rs` のルーティングに追加**

既存の `} else if request.starts_with("GET /api/neurons") {` の直後に追加:

```rust
                                } else if request.starts_with("GET /api/concurrency") {
                                    let available = gmn_sem_clone.available_permits();
                                    let active = gmn_capacity.saturating_sub(available);
                                    let queue_state = crate::QUEUE_STATE.lock().unwrap();
                                    let queue_depth = queue_state.items.iter()
                                        .filter(|i| i.status == "Waiting")
                                        .count();
                                    let cooldown_secs = rustyclaw_providers::global_cooldown_remaining()
                                        .map(|d| d.as_secs_f64())
                                        .unwrap_or(0.0);
                                    let json = serde_json::json!({
                                        "active": active,
                                        "available": available,
                                        "capacity": gmn_capacity,
                                        "queue_depth": queue_depth,
                                        "cooldown_secs": cooldown_secs,
                                        "global_cooldown": if cooldown_secs > 0.0 { Some(cooldown_secs) } else { None::<f64> }
                                    });
                                    ("200 OK".to_string(), json.to_string(), "application/json; charset=utf-8")
```

- [ ] **Step 4: lib.rs の HealthServer::new 呼び出しを更新**

`lib.rs` の `health::HealthServer::new(8080, ...)` を:

```rust
        let health_server = health::HealthServer::new(
            8080,
            reload_tx,
            bus.clone(),
            self.workspace_path.clone(),
            self.gmn_sem.clone(),
            1, // gmn_sem capacity = 1
        );
```

- [ ] **Step 5: cargo check + コミット**

```bash
cargo check -p rustyclaw-gateway 2>&1 | grep "^error" | head -5
git add crates/rustyclaw-gateway/src/health.rs crates/rustyclaw-gateway/src/lib.rs
git commit -m "feat(health): add /api/concurrency endpoint with gmn_sem state"
```

---

## Task 3: LlmResponse にトークンフィールドを追加し providers で収集

**Files:**
- Modify: `crates/rustyclaw-providers/src/lib.rs`

> `LlmResponse` に `prompt_tokens: Option<u32>` / `completion_tokens: Option<u32>` / `total_tokens: Option<u32>` / `model_used: Option<String>` を追加。`OpenAiResponse` に `usage` フィールドを追加して値を埋める。

- [ ] **Step 1: `LlmResponse` にフィールドを追加**

```rust
pub struct LlmResponse {
    pub content: String,
    pub role: String,
    pub tool_calls: Option<Vec<ToolCall>>,
    pub prompt_tokens: Option<u32>,      // ← 追加
    pub completion_tokens: Option<u32>,  // ← 追加
    pub total_tokens: Option<u32>,       // ← 追加
    pub model_used: Option<String>,      // ← 追加
}
```

- [ ] **Step 2: `OpenAiResponse` に `usage` フィールドを追加**

```rust
#[derive(Debug, Deserialize)]
struct OpenAiUsage {
    #[serde(default)]
    prompt_tokens: u32,
    #[serde(default)]
    completion_tokens: u32,
    #[serde(default)]
    total_tokens: u32,
}

#[derive(Debug, Deserialize)]
struct OpenAiResponse {
    choices: Vec<OpenAiChoice>,
    #[serde(default)]
    usage: Option<OpenAiUsage>,
    #[serde(default)]
    model: Option<String>,
}
```

- [ ] **Step 3: `OpenAiCompatProvider::complete()` の `Ok(LlmResponse {...})` を更新**

既存の `Ok(LlmResponse { content: ..., role: ..., tool_calls: ... })` を:

```rust
        Ok(LlmResponse {
            content: choice.message.content.clone().unwrap_or_default(),
            role: choice.message.role.clone(),
            tool_calls: choice.message.tool_calls.clone(),
            prompt_tokens: resp_data.usage.as_ref().map(|u| u.prompt_tokens),
            completion_tokens: resp_data.usage.as_ref().map(|u| u.completion_tokens),
            total_tokens: resp_data.usage.as_ref().map(|u| u.total_tokens),
            model_used: resp_data.model.clone(),
        })
```

- [ ] **Step 4: 既存の `LlmResponse { ... }` 構築箇所を全て更新**（NoopProvider 等）

`grep -n "Ok(LlmResponse {" crates/rustyclaw-providers/src/lib.rs` で全箇所を特定し、新フィールドに `None` を追加:

```bash
grep -n "LlmResponse {" /mnt/Projects/RustyClaw/crates/rustyclaw-providers/src/lib.rs
```

見つかった各箇所に `prompt_tokens: None, completion_tokens: None, total_tokens: None, model_used: None,` を追加。

- [ ] **Step 5: cargo check + コミット**

```bash
cargo check 2>&1 | grep "^error" | head -10
git add crates/rustyclaw-providers/src/lib.rs
git commit -m "feat(providers): add token usage fields to LlmResponse"
```

---

## Task 4: usage テーブル拡張 + gateway での usage 記録

**Files:**
- Modify: `crates/rustyclaw-storage/src/lib.rs`（テーブル拡張 + record_usage シグネチャ変更）
- Modify: `crates/rustyclaw-gateway/src/lib.rs`（LLM 呼び出し後に record_usage 呼び出し）

- [ ] **Step 1: usage テーブルのスキーマ拡張**

`storage/src/lib.rs` の `create_tables()` 内の `usage` テーブル定義を変更:

```rust
            CREATE TABLE IF NOT EXISTS usage (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                session_id TEXT NOT NULL,
                prompt_tokens INTEGER NOT NULL DEFAULT 0,
                completion_tokens INTEGER NOT NULL DEFAULT 0,
                total_tokens INTEGER NOT NULL DEFAULT 0,
                model TEXT NOT NULL DEFAULT '',
                trigger_type TEXT NOT NULL DEFAULT 'unknown',
                duration_ms INTEGER NOT NULL DEFAULT 0,
                created_at TEXT NOT NULL
            );
```

マイグレーション（既存 DB への列追加）を `create_tables()` 末尾に追加:

```rust
        // Migration: add columns for existing DBs
        let _ = self.conn.execute_batch(
            "ALTER TABLE usage ADD COLUMN total_tokens INTEGER NOT NULL DEFAULT 0;
             ALTER TABLE usage ADD COLUMN model TEXT NOT NULL DEFAULT '';
             ALTER TABLE usage ADD COLUMN trigger_type TEXT NOT NULL DEFAULT 'unknown';
             ALTER TABLE usage ADD COLUMN duration_ms INTEGER NOT NULL DEFAULT 0;"
        ); // ignore errors (columns may already exist)
```

- [ ] **Step 2: `record_usage()` のシグネチャを拡張**

```rust
pub fn record_usage(
    &self,
    session_id: &str,
    prompt: u32,
    completion: u32,
    total: u32,
    model: &str,
    trigger_type: &str,
    duration_ms: u64,
) -> Result<()> {
    let now = chrono::Utc::now().to_rfc3339();
    self.conn.execute(
        "INSERT INTO usage (session_id, prompt_tokens, completion_tokens, total_tokens, model, trigger_type, duration_ms, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        rusqlite::params![session_id, prompt, completion, total, model, trigger_type, duration_ms as i64, now],
    ).context("Failed to record usage in SQLite")?;
    Ok(())
}
```

- [ ] **Step 3: gateway/src/lib.rs で LLM 呼び出し後に record_usage を呼ぶ**

`execute_with_tools` と `execute` の `Ok(response)` ブロックの中（`AgentResponse` を publish した直後）に追加:

```rust
                                            // トークン使用量を記録
                                            if let Ok(db) = rustyclaw_storage::DbManager::new(&db_path) {
                                                let trigger = if session_id.starts_with("cron:heartbeat") { "heartbeat" }
                                                    else if session_id.starts_with("cron:") { "cron" }
                                                    else if session_id.starts_with("discord-") { "discord" }
                                                    else if session_id.starts_with("cli-") { "cli" }
                                                    else { "unknown" };
                                                let _ = db.record_usage(
                                                    &session_id,
                                                    response.prompt_tokens.unwrap_or(0),
                                                    response.completion_tokens.unwrap_or(0),
                                                    response.total_tokens.unwrap_or(0),
                                                    response.model_used.as_deref().unwrap_or(""),
                                                    trigger,
                                                    0, // duration tracking は将来実装
                                                );
                                            }
```

- [ ] **Step 4: cargo check + コミット**

```bash
cargo check 2>&1 | grep "^error" | head -10
git add crates/rustyclaw-storage/src/lib.rs crates/rustyclaw-gateway/src/lib.rs
git commit -m "feat(storage,gateway): extend usage table and record LLM token usage"
```

---

## Task 5: 集計クエリ + `/api/usage/*` エンドポイント

**Files:**
- Modify: `crates/rustyclaw-storage/src/lib.rs`（集計メソッド追加）
- Modify: `crates/rustyclaw-gateway/src/health.rs`（3エンドポイント追加）

- [ ] **Step 1: `get_usage_summary()` を追加**

`storage/src/lib.rs` の `record_usage()` の後に追加:

```rust
pub fn get_usage_summary(&self, since: Option<&str>) -> serde_json::Value {
    let where_clause = if since.is_some() { "WHERE created_at >= ?1" } else { "" };
    let params: &[&dyn rusqlite::ToSql] = if let Some(s) = since { &[&s] } else { &[] };

    let total: (i64, i64, i64, i64) = self.conn.query_row(
        &format!("SELECT COALESCE(COUNT(*),0), COALESCE(SUM(prompt_tokens),0), COALESCE(SUM(completion_tokens),0), COALESCE(SUM(total_tokens),0) FROM usage {}", where_clause),
        params,
        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
    ).unwrap_or((0,0,0,0));

    let mut by_model = serde_json::Map::new();
    if let Ok(mut stmt) = self.conn.prepare(
        &format!("SELECT model, COUNT(*), COALESCE(SUM(total_tokens),0) FROM usage {} GROUP BY model ORDER BY SUM(total_tokens) DESC LIMIT 10", where_clause)
    ) {
        let _ = stmt.query_map(params, |row| {
            Ok((row.get::<_,String>(0)?, row.get::<_,i64>(1)?, row.get::<_,i64>(2)?))
        }).map(|rows| {
            for row in rows.flatten() {
                by_model.insert(row.0, serde_json::json!({ "runs": row.1, "tokens": row.2 }));
            }
        });
    }

    serde_json::json!({
        "total_runs": total.0,
        "total_input_tokens": total.1,
        "total_completion_tokens": total.2,
        "total_tokens": total.3,
        "by_model": by_model,
    })
}

pub fn get_usage_timeline(&self, since: Option<&str>) -> Vec<serde_json::Value> {
    let where_clause = if since.is_some() { "WHERE created_at >= ?1" } else { "" };
    let params: &[&dyn rusqlite::ToSql] = if let Some(s) = since { &[&s] } else { &[] };
    let mut stmt = match self.conn.prepare(&format!(
        "SELECT DATE(created_at) AS d, COUNT(*), COALESCE(SUM(prompt_tokens),0), COALESCE(SUM(completion_tokens),0), COALESCE(SUM(total_tokens),0) FROM usage {} GROUP BY DATE(created_at) ORDER BY d ASC",
        where_clause
    )) {
        Ok(s) => s,
        Err(_) => return vec![],
    };
    stmt.query_map(params, |row| {
        Ok(serde_json::json!({
            "date": row.get::<_,String>(0)?,
            "runs": row.get::<_,i64>(1)?,
            "input_tokens": row.get::<_,i64>(2)?,
            "completion_tokens": row.get::<_,i64>(3)?,
            "tokens": row.get::<_,i64>(4)?,
        }))
    }).map(|rows| rows.flatten().collect()).unwrap_or_default()
}

pub fn get_usage_by_trigger(&self, since: Option<&str>) -> Vec<serde_json::Value> {
    let where_clause = if since.is_some() { "WHERE created_at >= ?1" } else { "" };
    let params: &[&dyn rusqlite::ToSql] = if let Some(s) = since { &[&s] } else { &[] };
    let mut stmt = match self.conn.prepare(&format!(
        "SELECT trigger_type, COUNT(*), COALESCE(SUM(total_tokens),0) FROM usage {} GROUP BY trigger_type ORDER BY SUM(total_tokens) DESC",
        where_clause
    )) {
        Ok(s) => s,
        Err(_) => return vec![],
    };
    stmt.query_map(params, |row| {
        Ok(serde_json::json!({
            "trigger": row.get::<_,String>(0)?,
            "runs": row.get::<_,i64>(1)?,
            "tokens": row.get::<_,i64>(2)?,
        }))
    }).map(|rows| rows.flatten().collect()).unwrap_or_default()
}
```

- [ ] **Step 2: `/api/usage/*` エンドポイントを health.rs に追加**

`/api/concurrency` のルーティングの直後に追加:

```rust
                                } else if request.starts_with("GET /api/usage/summary") {
                                    let since = extract_since_param(&request);
                                    let db_path = workspace_path_clone.join("memory.db");
                                    let json = if let Ok(db) = rustyclaw_storage::DbManager::new(&db_path) {
                                        db.get_usage_summary(since.as_deref())
                                    } else {
                                        serde_json::json!({ "total_runs": 0, "total_tokens": 0, "by_model": {} })
                                    };
                                    ("200 OK".to_string(), json.to_string(), "application/json; charset=utf-8")

                                } else if request.starts_with("GET /api/usage/timeline") {
                                    let since = extract_since_param(&request);
                                    let db_path = workspace_path_clone.join("memory.db");
                                    let rows = if let Ok(db) = rustyclaw_storage::DbManager::new(&db_path) {
                                        db.get_usage_timeline(since.as_deref())
                                    } else { vec![] };
                                    let json = serde_json::to_string(&rows).unwrap_or_else(|_| "[]".to_string());
                                    ("200 OK".to_string(), json, "application/json; charset=utf-8")

                                } else if request.starts_with("GET /api/usage/by-trigger") {
                                    let since = extract_since_param(&request);
                                    let db_path = workspace_path_clone.join("memory.db");
                                    let rows = if let Ok(db) = rustyclaw_storage::DbManager::new(&db_path) {
                                        db.get_usage_by_trigger(since.as_deref())
                                    } else { vec![] };
                                    let json = serde_json::to_string(&rows).unwrap_or_else(|_| "[]".to_string());
                                    ("200 OK".to_string(), json, "application/json; charset=utf-8")
```

`extract_since_param` ヘルパーを `health.rs` の末尾（`fn get_dashboard_html` の前）に追加:

```rust
/// GET /api/usage/summary?since=2026-05-01 の since パラメータを抽出する
fn extract_since_param(request: &str) -> Option<String> {
    let first_line = request.lines().next()?;
    let query_start = first_line.find('?')?;
    let query = &first_line[query_start+1..];
    let end = query.find(' ').unwrap_or(query.len());
    for pair in query[..end].split('&') {
        if let Some(val) = pair.strip_prefix("since=") {
            if val.len() >= 10 && val.chars().next().map(|c| c.is_ascii_digit()).unwrap_or(false) {
                return Some(val.to_string());
            }
        }
    }
    None
}
```

- [ ] **Step 3: cargo test + コミット**

```bash
cargo test 2>&1 | grep "test result" | grep -v "0 passed"
git add crates/rustyclaw-storage/src/lib.rs crates/rustyclaw-gateway/src/health.rs
git commit -m "feat(storage,health): add usage aggregation queries and /api/usage/* endpoints"
```

---

## Self-Review

### Spec Coverage

| 要件 | Task |
|---|---|
| サイバー CSS（スキャンライン・ネオングロー・グリッチヘッダー） | Task 1 ✅ |
| Monitor 3行レイアウト | Task 1 ✅ |
| Lane Queue パネル | Task 1 ✅ |
| LLM Request/Response Inspector | Task 1 ✅ |
| /api/concurrency（gmn_sem 状態） | Task 2 ✅ |
| Concurrency パネル | Task 1 ✅（JS が /api/concurrency を呼ぶ） |
| Neurons パネル | Task 1 ✅（既存 /api/neurons を使用） |
| Stats タブ（KPI / 時系列 SVG / モデル別・トリガー別） | Task 1 ✅（HTML）|
| LlmResponse トークンフィールド | Task 3 ✅ |
| usage テーブル拡張 + 実際の記録 | Task 4 ✅ |
| /api/usage/* エンドポイント | Task 5 ✅ |
| Stats ページのライブデータ接続 | Task 1 + Task 5 ✅（JS は /api/usage/* を呼ぶ） |

### Placeholder チェック

- TBD / TODO なし ✅
- 全コードブロックに実装済みコードあり ✅
- `extract_since_param` は Task 5 で定義 → Task 5 内で参照 ✅
- `gmn_sem_clone` は Task 2 Step 2 で定義 → Step 3 で参照 ✅

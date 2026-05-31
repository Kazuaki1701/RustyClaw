# Phase 31 STEP 1–3（ダッシュボード）実装計画

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** ダッシュボードの3つの即効〜局所改善を実装する — (1) 表示の体感改善（LLM REQUEST 末尾表示・IP:PORT 表記・QUEUE アイドル文言）、(2) トークン使用量グラフの時間別化（1D/7D/ALL）、(3) LANE QUEUE への cron スケジュール表示。

**Architecture:** ダッシュボードの HTML/CSS/JS は `crates/rustyclaw-gateway/src/health.rs` 内の Rust 文字列リテラル（`get_dashboard_html()`）に埋め込まれている。フロント変更＝この文字列の編集で、JS の単体テストフレームワークは無い → ビルド＋手動（curl/ブラウザ）で検証する。集計クエリと次回実行時刻計算は純粋ロジックとして `rustyclaw-storage` / `rustyclaw-gateway` に置き、`cargo test` で検証する。時刻バケットは epoch フロアで切り（TZ 非依存）、表示ラベルのみフロントでローカル整形する。

**Tech Stack:** Rust（axum 不使用の生 TCP + 手書き HTTP ルーティング）, rusqlite（SQLite）, chrono, バニラ JS + SVG。

**前提コマンド:**
- ビルド: `cargo build -p rustyclaw-gateway`
- テスト: `cargo test -p rustyclaw-storage`, `cargo test -p rustyclaw-gateway`
- ローカル起動（API 手動確認用・実 LLM を呼ばない）:
  `RUSTYCLAW_NO_AGENT=1 cargo run -p rustyclaw-cli -- --config config/config.json --workspace <workspace> gateway`
  → 別端末で `curl -s http://127.0.0.1:8080/...`
- 実機確認は `scripts/deploy.sh` 後に `ssh rp1 'curl -s http://127.0.0.1:8080/...'`

---

## STEP 1: Dashboard 即効表示改善（ISSUE-19 / 24 / 17）

フロント文字列のみの変更。`health.rs` の該当行を編集する。

### Task 1.1: LLM REQUEST パネルを末尾表示に変更（ISSUE-19）

**Files:**
- Modify: `crates/rustyclaw-gateway/src/health.rs:677`

- [ ] **Step 1: 該当行を末尾保持に変更**

`health.rs:677` の現状:
```javascript
    if(rq.ok){const txt=await rq.text();document.getElementById('req-ts').textContent=ts;document.getElementById('reqPanel').textContent=txt.length>4000?txt.substring(0,4000)+'\n...(truncated)':txt}
```
を次に置換（先頭4000字→末尾4000字。リクエストは可変部＝会話が末尾にあるため）:
```javascript
    if(rq.ok){const txt=await rq.text();document.getElementById('req-ts').textContent=ts;document.getElementById('reqPanel').textContent=txt.length>4000?'...(truncated head)\n'+txt.slice(-4000):txt}
```
※ 直下の RESPONSE パネル（678 行, `txt.substring(0,3000)`）は応答が先頭=本文のため変更しない。

- [ ] **Step 2: ビルドして埋め込み文字列が壊れていないこと**

Run: `cargo build -p rustyclaw-gateway`
Expected: 警告のみ（既存の `JsonRpcResponse` dead_code 警告）でコンパイル成功。

- [ ] **Step 3: コミット**

```bash
git add crates/rustyclaw-gateway/src/health.rs
git commit -m "fix(dashboard): show tail of LLM request so latest turn is visible (ISSUE-19)"
```

### Task 1.2: ヘッダのポート表記を IP:PORT に（ISSUE-24）

**Files:**
- Modify: `crates/rustyclaw-gateway/src/health.rs:526`（表示要素に id 付与）
- Modify: `crates/rustyclaw-gateway/src/health.rs:774` 付近（初期化 JS で host を設定）

- [ ] **Step 1: ヘッダの span に id を付与**

`health.rs:526` の現状:
```html
    <span style="font-size:10px;color:var(--muted);font-family:'Fira Code',monospace">:8080</span>
```
を次に置換:
```html
    <span id="hostLabel" style="font-size:10px;color:var(--muted);font-family:'Fira Code',monospace">:8080</span>
```

- [ ] **Step 2: 初期化 JS に host 設定を追加**

`health.rs:774` の現状（初期呼び出し行）:
```javascript
updateQueue();updateConcurrency();updateNeurons();updateInspector();updateLog();
```
を次に置換（接続に使った `host`＝`IP:PORT` を自動表示。サーバ側で IP を埋め込まず常に正しい）:
```javascript
document.getElementById('hostLabel').textContent=location.host;updateQueue();updateConcurrency();updateNeurons();updateInspector();updateLog();
```

- [ ] **Step 3: ビルド**

Run: `cargo build -p rustyclaw-gateway`
Expected: コンパイル成功。

- [ ] **Step 4: コミット**

```bash
git add crates/rustyclaw-gateway/src/health.rs
git commit -m "feat(dashboard): show IP:PORT in header via location.host (ISSUE-24)"
```

### Task 1.3: LANE QUEUE のアイドル文言を明示（ISSUE-17 表示部）

**Files:**
- Modify: `crates/rustyclaw-gateway/src/health.rs:535`（初期 HTML）
- Modify: `crates/rustyclaw-gateway/src/health.rs:633`（renderQueue の空分岐）

- [ ] **Step 1: 初期 HTML の文言変更**

`health.rs:535` の現状:
```html
      <div class="panel-body" id="queuePanel" style="padding:6px 8px;"><div style="color:var(--muted);text-align:center;padding:10px;font-family:'Fira Code',monospace;font-size:11px;">キューは空（稼働タスクなし）</div></div>
```
の `キューは空（稼働タスクなし）` を `稼働タスクなし（待機中・正常）` に変更。

- [ ] **Step 2: renderQueue の空分岐の文言変更**

`health.rs:633` の現状:
```javascript
    if(items.length===0){panel.innerHTML='<div style="color:var(--muted);text-align:center;padding:10px;font-family:\'Fira Code\',monospace;font-size:11px;">キューは空（稼働タスクなし）</div>';return}
```
の `キューは空（稼働タスクなし）` を `稼働タスクなし（待機中・正常）` に変更。

- [ ] **Step 3: ビルド**

Run: `cargo build -p rustyclaw-gateway`
Expected: コンパイル成功。

- [ ] **Step 4: STEP1 全体の手動確認（任意・ブラウザ）**

ローカル起動（既存ワークスペースを指定）:
```bash
RUSTYCLAW_NO_AGENT=1 cargo run -p rustyclaw-cli -- --config config/config.json --workspace production/workspace gateway
```
ブラウザで `http://127.0.0.1:8080/` を開き、ヘッダが `127.0.0.1:8080` 表記、LANE QUEUE が「稼働タスクなし（待機中・正常）」、LLM REQUEST パネルに `...(truncated head)` 付きで会話末尾が見えること（データがあれば）を確認。Ctrl+C で停止。

- [ ] **Step 5: コミット**

```bash
git add crates/rustyclaw-gateway/src/health.rs
git commit -m "feat(dashboard): clarify idle lane-queue wording (ISSUE-17)"
```

---

## STEP 2: トークン使用量グラフの時間別化（ISSUE-23）

集計を epoch フロアのバケットに変え、窓全体を 0 埋め。期間 1D/7D/ALL で粒度（600s/3600s/3600s）を切替。

### Task 2.1: `get_usage_timeline` を粒度・窓・0埋め対応に変更（＋テスト）

**Files:**
- Modify: `crates/rustyclaw-storage/src/lib.rs:138-160`（`get_usage_timeline`）
- Modify: `crates/rustyclaw-storage/src/lib.rs:485`（既存テストの呼び出し更新）
- Test: `crates/rustyclaw-storage/src/lib.rs`（新規 `test_usage_timeline_hourly`）

- [ ] **Step 1: 失敗するテストを追加**

`crates/rustyclaw-storage/src/lib.rs` の `mod tests` 内（`test_usage_aggregation` の直後）に追加:
```rust
    #[test]
    fn test_usage_timeline_hourly_buckets_and_zero_fill() -> Result<()> {
        let tmp_dir = tempfile::tempdir()?;
        let db = DbManager::new(&tmp_dir.path().join("tl.db"))?;
        // 既知の created_at を 2 件（同一日・2時間離れ）直接挿入する
        db.conn.execute(
            "INSERT INTO usage (session_id, prompt_tokens, completion_tokens, total_tokens, model, trigger_type, duration_ms, created_at) \
             VALUES ('s1', 100, 50, 150, 'm', 'discord', 0, '2026-05-31T01:00:00+00:00')",
            [],
        )?;
        db.conn.execute(
            "INSERT INTO usage (session_id, prompt_tokens, completion_tokens, total_tokens, model, trigger_type, duration_ms, created_at) \
             VALUES ('s2', 10, 5, 15, 'm', 'discord', 0, '2026-05-31T03:00:00+00:00')",
            [],
        )?;
        // 窓: 01:00〜03:00 UTC、粒度 1 時間 → 3 バケット（01,02,03時）、02時は 0 埋め
        let since = 1780189200; // 2026-05-31T01:00:00Z
        let until = 1780196400; // 2026-05-31T03:00:00Z
        let rows = db.get_usage_timeline(Some(since), until, 3600);
        assert_eq!(rows.len(), 3, "01/02/03 時の3バケット（0埋め含む）");
        assert_eq!(rows[0]["tokens"], 150);
        assert_eq!(rows[1]["tokens"], 0);   // 02時は0埋め
        assert_eq!(rows[2]["tokens"], 15);
        assert_eq!(rows[0]["bucket_epoch"], since);
        Ok(())
    }
```

- [ ] **Step 2: テストが（コンパイルエラーで）失敗することを確認**

Run: `cargo test -p rustyclaw-storage test_usage_timeline_hourly_buckets_and_zero_fill`
Expected: コンパイルエラー（`get_usage_timeline` の引数数が合わない）。

- [ ] **Step 3: `get_usage_timeline` を新シグネチャで実装**

`crates/rustyclaw-storage/src/lib.rs:138` の既存 `get_usage_timeline` 関数全体を次に置換:
```rust
    /// 使用量をトークン数で時刻バケット集計する。
    /// - `since_epoch`: 集計開始の unix 秒（None=全期間。窓開始は最初のデータ）
    /// - `until_epoch`: 集計終了の unix 秒（通常は現在時刻）
    /// - `granularity_secs`: バケット幅（600=10分, 3600=1時間, 86400=日）
    /// 戻り値は窓内の全バケットを 0 埋めして昇順で返す（連続した時間軸）。
    pub fn get_usage_timeline(
        &self,
        since_epoch: Option<i64>,
        until_epoch: i64,
        granularity_secs: u64,
    ) -> Vec<serde_json::Value> {
        let g = (granularity_secs.max(1)) as i64;
        let where_clause = if since_epoch.is_some() {
            "WHERE CAST(strftime('%s', created_at) AS INTEGER) >= ?1"
        } else {
            ""
        };
        let params: Vec<&dyn rusqlite::ToSql> = match since_epoch.as_ref() {
            Some(s) => vec![s],
            None => vec![],
        };
        let sql = format!(
            "SELECT (CAST(strftime('%s', created_at) AS INTEGER) / {g}) * {g} AS bucket, \
             COALESCE(SUM(prompt_tokens),0), COALESCE(SUM(completion_tokens),0), COALESCE(SUM(total_tokens),0) \
             FROM usage {where_clause} GROUP BY bucket ORDER BY bucket ASC",
            g = g,
            where_clause = where_clause
        );
        let mut stmt = match self.conn.prepare(&sql) {
            Ok(s) => s,
            Err(_) => return vec![],
        };
        let sparse: std::collections::BTreeMap<i64, (i64, i64, i64)> = stmt
            .query_map(params.as_slice(), |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    (row.get::<_, i64>(1)?, row.get::<_, i64>(2)?, row.get::<_, i64>(3)?),
                ))
            })
            .map(|rows| rows.flatten().collect())
            .unwrap_or_default();

        if sparse.is_empty() {
            return vec![];
        }
        // 窓開始: since 指定があればそのフロア、無ければ最初のデータバケット
        let start = match since_epoch {
            Some(s) => (s / g) * g,
            None => *sparse.keys().next().unwrap(),
        };
        let end = (until_epoch / g) * g;
        let mut out = Vec::new();
        let mut b = start;
        while b <= end {
            let (i, c, t) = sparse.get(&b).copied().unwrap_or((0, 0, 0));
            out.push(serde_json::json!({
                "bucket_epoch": b,
                "input_tokens": i,
                "completion_tokens": c,
                "tokens": t,
            }));
            b += g;
        }
        out
    }
```

- [ ] **Step 4: 既存テストの呼び出しを更新**

`crates/rustyclaw-storage/src/lib.rs:485` 付近、`test_usage_aggregation` 内の現状:
```rust
        let timeline = db.get_usage_timeline(None);
        assert_eq!(timeline.len(), 1); // both rows on the same UTC day
        assert_eq!(timeline[0]["tokens"], 430);
```
を次に置換（日粒度・全期間・until=現在）:
```rust
        let now = chrono::Utc::now().timestamp();
        let timeline = db.get_usage_timeline(None, now, 86400);
        assert!(!timeline.is_empty());
        let day_total: i64 = timeline.iter().map(|r| r["tokens"].as_i64().unwrap_or(0)).sum();
        assert_eq!(day_total, 430);
```

- [ ] **Step 5: テストが通ることを確認**

Run: `cargo test -p rustyclaw-storage`
Expected: `test_usage_timeline_hourly_buckets_and_zero_fill` と `test_usage_aggregation` を含む全テスト PASS。

- [ ] **Step 6: コミット**

```bash
git add crates/rustyclaw-storage/src/lib.rs
git commit -m "feat(storage): bucketed usage timeline with granularity + zero-fill (ISSUE-23)"
```

### Task 2.2: `/api/usage/timeline` を granularity / from 対応に

**Files:**
- Modify: `crates/rustyclaw-gateway/src/health.rs:179-186`（timeline ハンドラ）
- Modify: `crates/rustyclaw-gateway/src/health.rs:312` 付近（クエリ抽出ヘルパ追加）

- [ ] **Step 1: 汎用クエリ抽出ヘルパを追加**

`crates/rustyclaw-gateway/src/health.rs` の `fn extract_since_param` 定義（312 行付近）の直前に追加:
```rust
/// GET /path?key=value のクエリから整数値を取り出す（無ければ None）。
fn extract_query_i64(request: &str, key: &str) -> Option<i64> {
    let first_line = request.lines().next()?;
    let query_start = first_line.find('?')?;
    let query = &first_line[query_start + 1..];
    let end = query.find(' ').unwrap_or(query.len());
    for pair in query[..end].split('&') {
        if let Some(val) = pair.strip_prefix(&format!("{}=", key)) {
            return val.parse::<i64>().ok();
        }
    }
    None
}
```

- [ ] **Step 2: timeline ハンドラを差し替え**

`crates/rustyclaw-gateway/src/health.rs:179-186` の現状:
```rust
                                } else if request.starts_with("GET /api/usage/timeline") {
                                    let since = extract_since_param(&request);
                                    let db_path = workspace_path_clone.join("memory.db");
                                    let rows = if let Ok(db) = rustyclaw_storage::DbManager::new(&db_path) {
                                        db.get_usage_timeline(since.as_deref())
                                    } else { vec![] };
                                    let json = serde_json::to_string(&rows).unwrap_or_else(|_| "[]".to_string());
                                    ("200 OK".to_string(), json, "application/json; charset=utf-8")
```
を次に置換（`gran`＝粒度秒、`from`＝開始 epoch。未指定時は日粒度・全期間で後方互換）:
```rust
                                } else if request.starts_with("GET /api/usage/timeline") {
                                    let gran = extract_query_i64(&request, "gran").unwrap_or(86400).max(1) as u64;
                                    let from = extract_query_i64(&request, "from");
                                    let now = chrono::Utc::now().timestamp();
                                    let db_path = workspace_path_clone.join("memory.db");
                                    let rows = if let Ok(db) = rustyclaw_storage::DbManager::new(&db_path) {
                                        db.get_usage_timeline(from, now, gran)
                                    } else { vec![] };
                                    let json = serde_json::to_string(&rows).unwrap_or_else(|_| "[]".to_string());
                                    ("200 OK".to_string(), json, "application/json; charset=utf-8")
```

- [ ] **Step 3: ビルド**

Run: `cargo build -p rustyclaw-gateway`
Expected: コンパイル成功。

- [ ] **Step 4: API 手動確認**

ローカル起動（Task 1.3 Step4 と同様）後、別端末で:
```bash
# 直近24h・10分粒度
curl -s "http://127.0.0.1:8080/api/usage/timeline?gran=600&from=$(($(date +%s)-86400))" | head -c 400; echo
```
Expected: `[{"bucket_epoch":...,"input_tokens":...,"completion_tokens":...,"tokens":...}, ...]` が連続した bucket_epoch（差600）で並ぶ。データが無ければ `[]`。

- [ ] **Step 5: コミット**

```bash
git add crates/rustyclaw-gateway/src/health.rs
git commit -m "feat(dashboard): timeline API accepts gran/from params (ISSUE-23)"
```

### Task 2.3: 期間ボタンを 1D/7D/ALL に・粒度切替・X軸ラベルを時刻に（フロント）

**Files:**
- Modify: `crates/rustyclaw-gateway/src/health.rs:588-591`（期間ボタン）
- Modify: `crates/rustyclaw-gateway/src/health.rs:709-714`（`currentPeriodDays` / `setPeriod` / `loadStats`）
- Modify: `crates/rustyclaw-gateway/src/health.rs:744`,`749-750`,`763-764`（`renderTimeline`）

- [ ] **Step 1: 期間ボタンを置換**

`health.rs:588-591` の現状:
```html
    <div class="period-bar">
      <button class="period-btn" onclick="setPeriod(7,this)">7D</button>
      <button class="period-btn active" onclick="setPeriod(30,this)">30D</button>
      <button class="period-btn" onclick="setPeriod(0,this)">ALL</button>
```
を次に置換:
```html
    <div class="period-bar">
      <button class="period-btn active" onclick="setPeriod('1d',this)">1D</button>
      <button class="period-btn" onclick="setPeriod('7d',this)">7D</button>
      <button class="period-btn" onclick="setPeriod('all',this)">ALL</button>
```

- [ ] **Step 2: `currentPeriodDays`/`setPeriod`/`loadStats` を置換**

`health.rs:709-714` の現状:
```javascript
let currentPeriodDays=30;
function setPeriod(days,btn){currentPeriodDays=days;document.querySelectorAll('.period-btn').forEach(b=>b.classList.remove('active'));btn.classList.add('active');loadStats()}
async function loadStats(){
  const since=currentPeriodDays>0?new Date(Date.now()-currentPeriodDays*86400000).toISOString().slice(0,10):undefined;
  const qs=since?'?since='+since:'';
  try{
    const[rSum,rTl,rTr,rN]=await Promise.all([fetch('/api/usage/summary'+qs),fetch('/api/usage/timeline'+qs),fetch('/api/usage/by-trigger'+qs),fetch('/api/neurons')]);
```
を次に置換（timeline は gran/from、summary/by-trigger は従来の since=日付。`window.statGran` に粒度を保持して renderTimeline がラベル整形に使う）:
```javascript
let currentPeriod='1d';
const PERIOD_CFG={'1d':{gran:600,secs:86400},'7d':{gran:3600,secs:604800},'all':{gran:3600,secs:null}};
function setPeriod(p,btn){currentPeriod=p;document.querySelectorAll('.period-btn').forEach(b=>b.classList.remove('active'));btn.classList.add('active');loadStats()}
async function loadStats(){
  const cfg=PERIOD_CFG[currentPeriod];
  window.statGran=cfg.gran;
  const now=Math.floor(Date.now()/1000);
  const tlQs=`?gran=${cfg.gran}`+(cfg.secs?`&from=${now-cfg.secs}`:'');
  const sinceDate=cfg.secs?new Date(Date.now()-cfg.secs*1000).toISOString().slice(0,10):undefined;
  const sumQs=sinceDate?'?since='+sinceDate:'';
  try{
    const[rSum,rTl,rTr,rN]=await Promise.all([fetch('/api/usage/summary'+sumQs),fetch('/api/usage/timeline'+tlQs),fetch('/api/usage/by-trigger'+sumQs),fetch('/api/neurons')]);
```

- [ ] **Step 3: `renderTimeline` の集計参照と X 軸ラベルを更新**

`health.rs:744` の現状:
```javascript
  const maxT=Math.max(...rows.map(r=>r.total_tokens??r.tokens??0));
```
を次に置換:
```javascript
  const maxT=Math.max(...rows.map(r=>r.tokens??0));
```

`health.rs:763-764` の現状:
```javascript
  const step=Math.max(1,Math.floor(rows.length/7));
  document.getElementById('chartXAxis').innerHTML=rows.filter((_,i)=>i%step===0||i===rows.length-1).map(r=>`<span>${r.date}</span>`).join('');
```
を次に置換（粒度に応じて 1D=`HH:MM`、それ以外=`MM/DD HH:00` をローカル時刻で整形）:
```javascript
  const step=Math.max(1,Math.floor(rows.length/7));
  const fmt=ep=>{const d=new Date(ep*1000);const p=n=>String(n).padStart(2,'0');return window.statGran<3600?`${p(d.getHours())}:${p(d.getMinutes())}`:`${p(d.getMonth()+1)}/${p(d.getDate())} ${p(d.getHours())}:00`};
  document.getElementById('chartXAxis').innerHTML=rows.filter((_,i)=>i%step===0||i===rows.length-1).map(r=>`<span>${fmt(r.bucket_epoch)}</span>`).join('');
```
※ `inputPts`/`outPts`（749-750 行）は `r.input_tokens`/`r.completion_tokens` を参照しており、新しい行も同名フィールドを返すため変更不要。

- [ ] **Step 4: ビルド**

Run: `cargo build -p rustyclaw-gateway`
Expected: コンパイル成功。

- [ ] **Step 5: 手動確認（ブラウザ）**

ローカル起動後、`http://127.0.0.1:8080/` の STATS タブを開き、期間ボタンが `1D / 7D / ALL`、1D 選択時に横軸が `HH:MM`（10分刻み）で複数点のグラフが描画されること、7D/ALL で `MM/DD HH:00`（1時間刻み）になることを確認（データがある範囲）。

- [ ] **Step 6: コミット**

```bash
git add crates/rustyclaw-gateway/src/health.rs
git commit -m "feat(dashboard): 1D/7D/ALL period with 10m/1h time-axis token graph (ISSUE-23)"
```

---

## STEP 3: LANE QUEUE への cron スケジュール表示（ISSUE-18、＋ISSUE-17 Waiting）

`cron.json` から各有効ジョブの次回実行時刻を算出し、LANE QUEUE に `SCHED` として表示する。

### Task 3.1: 次回実行時刻の計算関数（純粋ロジック）＋テスト

**Files:**
- Modify: `crates/rustyclaw-gateway/src/cron.rs`（`next_run_epoch` 追加・`use chrono::TimeZone` 等）
- Test: `crates/rustyclaw-gateway/src/cron.rs`（`mod tests`）

- [ ] **Step 1: 失敗するテストを追加**

`crates/rustyclaw-gateway/src/cron.rs` の末尾に追加（既存 `mod tests` があればその中、無ければ新規）:
```rust
#[cfg(test)]
mod next_run_tests {
    use super::*;
    use chrono::TimeZone;

    #[test]
    fn cron_type_returns_today_if_future_else_tomorrow() {
        // 現在 10:00 とする
        let now = chrono::Local.with_ymd_and_hms(2026, 5, 31, 10, 0, 0).unwrap();
        // 22:00 は当日の未来 → 同日 22:00
        let n1 = next_run_epoch("cron", Some("22:00"), None, now, None).unwrap();
        assert_eq!(n1, chrono::Local.with_ymd_and_hms(2026, 5, 31, 22, 0, 0).unwrap().timestamp());
        // 04:45 は当日の過去 → 翌日 04:45
        let n2 = next_run_epoch("cron", Some("04:45"), None, now, None).unwrap();
        assert_eq!(n2, chrono::Local.with_ymd_and_hms(2026, 6, 1, 4, 45, 0).unwrap().timestamp());
    }

    #[test]
    fn interval_type_uses_last_run_plus_minutes() {
        let now = chrono::Local.with_ymd_and_hms(2026, 5, 31, 10, 0, 0).unwrap();
        let last = now.timestamp() - 60 * 60; // 1時間前
        // 360分間隔 → last + 360分
        let n = next_run_epoch("interval", None, Some(360), now, Some(last)).unwrap();
        assert_eq!(n, last + 360 * 60);
        // last_run 無し → 即時（now）
        let n0 = next_run_epoch("interval", None, Some(360), now, None).unwrap();
        assert_eq!(n0, now.timestamp());
    }
}
```

- [ ] **Step 2: テストが失敗（未定義関数）することを確認**

Run: `cargo test -p rustyclaw-gateway next_run`
Expected: コンパイルエラー（`next_run_epoch` 未定義）。

- [ ] **Step 3: `next_run_epoch` を実装**

`crates/rustyclaw-gateway/src/cron.rs` の `Job`/`Trigger` 定義より下、`impl` の外（モジュール関数として）に追加:
```rust
/// トリガ種別から次回実行の unix 秒を算出する（純粋関数）。
/// - cron: `expression`="HH:MM"。`now` より未来なら当日、過去なら翌日。
/// - interval: `last_run` + `minutes`。`last_run` が無ければ `now`（即時）。
pub fn next_run_epoch(
    trigger_type: &str,
    expression: Option<&str>,
    minutes: Option<u64>,
    now: chrono::DateTime<chrono::Local>,
    last_run: Option<i64>,
) -> Option<i64> {
    use chrono::TimeZone;
    match trigger_type {
        "cron" => {
            let expr = expression?;
            let (h, m) = expr.split_once(':')?;
            let h: u32 = h.parse().ok()?;
            let m: u32 = m.parse().ok()?;
            let naive_today = now.date_naive().and_hms_opt(h, m, 0)?;
            let today = chrono::Local.from_local_datetime(&naive_today).single()?;
            let target = if today > now { today } else { today + chrono::Duration::days(1) };
            Some(target.timestamp())
        }
        "interval" => {
            let mins = minutes? as i64;
            match last_run {
                Some(lr) => Some(lr + mins * 60),
                None => Some(now.timestamp()),
            }
        }
        _ => None,
    }
}
```
※ ファイル冒頭の `use` に `chrono` が無ければ追加（多くの場合 `use chrono::...;` は既存）。`chrono::TimeZone` は関数内 `use` で取り込んでいるため追加不要。

- [ ] **Step 4: テストが通ることを確認**

Run: `cargo test -p rustyclaw-gateway next_run`
Expected: `cron_type_...` と `interval_type_...` が PASS。

- [ ] **Step 5: コミット**

```bash
git add crates/rustyclaw-gateway/src/cron.rs
git commit -m "feat(cron): next_run_epoch pure fn for schedule computation (ISSUE-18)"
```

### Task 3.2: スケジュール一覧を組み立てる関数 ＋ `/api/schedule`

**Files:**
- Modify: `crates/rustyclaw-gateway/src/cron.rs`（`compute_schedule` 追加）
- Modify: `crates/rustyclaw-gateway/src/health.rs`（`/api/schedule` ルート追加）

- [ ] **Step 1: `compute_schedule` を実装**

`crates/rustyclaw-gateway/src/cron.rs` に追加（`Job` 構造体・`next_run_epoch` を利用。`DbManager` から interval の last_run を取得）:
```rust
/// cron.json の全有効ジョブについて次回実行時刻を計算し JSON 配列で返す。
/// 返却: [{ "id", "name", "next_run_epoch", "trigger_type" }] を next_run 昇順。
pub fn compute_schedule(
    workspace_dir: &std::path::Path,
    db: &rustyclaw_storage::DbManager,
) -> Vec<serde_json::Value> {
    let cron_json_path = workspace_dir.join("cron.json");
    let content = match std::fs::read_to_string(&cron_json_path) {
        Ok(c) => c,
        Err(_) => return vec![],
    };
    let jobs: Vec<Job> = match serde_json::from_str(&content) {
        Ok(j) => j,
        Err(_) => return vec![],
    };
    let now = chrono::Local::now();
    let mut out: Vec<serde_json::Value> = Vec::new();
    for job in jobs.iter().filter(|j| j.enabled) {
        let last_run = if job.trigger.trigger_type == "interval" {
            let state_key = format!("cron_last_run:{}", job.id);
            db.get_last_patrol_run(&state_key)
                .ok()
                .flatten()
                .and_then(|s| s.parse::<i64>().ok())
        } else {
            None
        };
        if let Some(next) = next_run_epoch(
            &job.trigger.trigger_type,
            job.trigger.expression.as_deref(),
            job.trigger.minutes,
            now,
            last_run,
        ) {
            out.push(serde_json::json!({
                "id": job.id,
                "name": job.name,
                "next_run_epoch": next,
                "trigger_type": job.trigger.trigger_type,
            }));
        }
    }
    out.sort_by_key(|v| v["next_run_epoch"].as_i64().unwrap_or(i64::MAX));
    out
}
```
※ `Job` に `id`/`name` フィールドがあること（cron.rs 既存定義, 52-58 行）と、`get_last_patrol_run(&str) -> Result<Option<String>>`（storage 既存、cron ローダで使用済み）を前提とする。`interval` の last_run は秒で保存されている（ローダ `set_state_value(&state_key, &now_sec.to_string())`）。

- [ ] **Step 2: `/api/schedule` ルートを追加**

`crates/rustyclaw-gateway/src/health.rs` の `/api/usage/by-trigger` ハンドラ（187-194 行）の直後、`// ── ダッシュボード` コメントの前に追加:
```rust
                                } else if request.starts_with("GET /api/schedule") {
                                    let db_path = workspace_path_clone.join("memory.db");
                                    let rows = if let Ok(db) = rustyclaw_storage::DbManager::new(&db_path) {
                                        crate::cron::compute_schedule(&workspace_path_clone, &db)
                                    } else { vec![] };
                                    let json = serde_json::to_string(&rows).unwrap_or_else(|_| "[]".to_string());
                                    ("200 OK".to_string(), json, "application/json; charset=utf-8")
```
※ `crate::cron::compute_schedule` がパス解決できること（`cron` モジュールが `pub mod cron;` で公開されていること）を確認。非公開なら `lib.rs` の `mod cron;` を `pub mod cron;` に変更。

- [ ] **Step 3: ビルド**

Run: `cargo build -p rustyclaw-gateway`
Expected: コンパイル成功。`compute_schedule` 未使用警告が出る場合は次タスクで解消。

- [ ] **Step 4: API 手動確認**

ローカル起動（`--workspace production/workspace`、`cron.json` がある WS を指定）後:
```bash
curl -s "http://127.0.0.1:8080/api/schedule"; echo
```
Expected: `[{"id":"karakeep-cleanup","name":...,"next_run_epoch":<未来のepoch>,"trigger_type":"cron"}, ...]` が next_run 昇順で返る。

- [ ] **Step 5: コミット**

```bash
git add crates/rustyclaw-gateway/src/cron.rs crates/rustyclaw-gateway/src/health.rs
git commit -m "feat(dashboard): /api/schedule lists upcoming cron jobs (ISSUE-18)"
```

### Task 3.3: LANE QUEUE に SCHED 行を表示（フロント）

**Files:**
- Modify: `crates/rustyclaw-gateway/src/health.rs:627-644`（`updateQueue`）

- [ ] **Step 1: `updateQueue` を schedule マージ表示に拡張**

`health.rs:627-644` の `updateQueue` 関数全体を次に置換（実行中アイテムの下に SCHED 行を追加表示）:
```javascript
async function updateQueue(){
  try{
    const[rq,rs]=await Promise.all([fetch('/api/queue'),fetch('/api/schedule')]);
    if(!rq.ok)return;
    const items=await rq.json();
    const sched=rs.ok?await rs.json():[];
    document.getElementById('queue-ts').textContent='↻ '+now();
    const panel=document.getElementById('queuePanel');
    let html='';
    items.forEach((item)=>{
      const cls=item.status==='Executing'?'pill-exec':item.status==='Waiting'?'pill-wait':'pill-cool';
      const lbl=item.status==='Executing'?'EXEC':item.status==='Waiting'?'WAIT':'COOL';
      const elapsed=Math.floor((Date.now()-item.enqueued_at_ms)/1000);
      html+=`<div class="q-item"><span class="q-pill ${cls}">${lbl}</span><span class="q-sid">${item.session_id}</span><span class="q-desc">${item.description||''}</span><span class="q-time">${elapsed}s</span></div>`;
      if(item.status==='Cooldown'&&item.cooldown_left_secs>0){const pct=Math.min(100,(item.cooldown_left_secs/60)*100);html+=`<div class="cool-bar"><div class="cool-fill" style="width:${pct}%"></div></div>`}
    });
    sched.forEach((s)=>{
      const left=Math.max(0,s.next_run_epoch-Math.floor(Date.now()/1000));
      const h=Math.floor(left/3600),m=Math.floor((left%3600)/60);
      const eta=h>0?`${h}h${m}m`:`${m}m`;
      html+=`<div class="q-item"><span class="q-pill pill-wait">SCHED</span><span class="q-sid">${s.name}</span><span class="q-desc">${s.trigger_type}</span><span class="q-time">in ${eta}</span></div>`;
    });
    if(!html){html='<div style="color:var(--muted);text-align:center;padding:10px;font-family:\'Fira Code\',monospace;font-size:11px;">稼働タスク・予定なし</div>'}
    panel.innerHTML=html;
  }catch{}
}
```
※ `pill-wait` クラスは既存（WAIT 用）を流用。SCHED 専用スタイルが欲しければ後続で CSS 追加可（本計画では流用で可）。

- [ ] **Step 2: ビルド**

Run: `cargo build -p rustyclaw-gateway`
Expected: コンパイル成功。

- [ ] **Step 3: 手動確認（ブラウザ）**

ローカル起動（`cron.json` のある WS）後、`http://127.0.0.1:8080/` の MONITOR タブの LANE QUEUE に、各 cron が `SCHED <ジョブ名> <種別> in <h>h<m>m` で next_run 昇順表示されることを確認。

- [ ] **Step 4: コミット**

```bash
git add crates/rustyclaw-gateway/src/health.rs
git commit -m "feat(dashboard): show scheduled cron jobs in LANE QUEUE (ISSUE-18)"
```

### Task 3.4（任意）: Waiting 状態を bus 受信時に記録（ISSUE-17 capture）

> 単一ワーカーが bus からイベントを逐次取得する構造上、`Waiting` はほぼ可視化されない。bus 購読者を1つ増やし、`IncomingMessage` 受信時に即 `Waiting` を記録する。レース回避のため、`Executing`/`queue_remove` を行う既存ワーカー側が常に最終権を持つ（後勝ち）点に注意。優先度低のため、STEP3 の主目的（SCHED 表示）完了後に着手可。

**Files:**
- Modify: `crates/rustyclaw-gateway/src/lib.rs`（`LaneRegistry::run` 起動時に購読タスク追加）

- [ ] **Step 1: bus 購読タスクを追加**

`crates/rustyclaw-gateway/src/lib.rs` の `run`（各スケジューラ/ワーカーを spawn している箇所、188 行付近）に、次の購読タスクを追加（`self.bus.subscribe()` を1つ取得して spawn）:
```rust
        // ISSUE-17: IncomingMessage 受信直後に Waiting を可視化する観測タスク
        {
            let mut wait_rx = self.bus.subscribe();
            tokio::spawn(async move {
                loop {
                    match wait_rx.recv().await {
                        Ok(SystemEvent::IncomingMessage { session_id, content, .. }) => {
                            let desc = {
                                let t: String = content.chars().take(40).collect();
                                format!("User Prompt: \"{}\"", t.replace('\n', " "))
                            };
                            crate::queue_update_or_insert(&session_id, "Waiting", 0.0, &desc);
                        }
                        Ok(_) => {}
                        Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                        Err(_) => {} // Lagged は無視
                    }
                }
            });
        }
```
※ `SystemEvent`/`Priority` の import は既存。`queue_update_or_insert` は同 crate の `pub fn`。

- [ ] **Step 2: ビルド**

Run: `cargo build -p rustyclaw-gateway`
Expected: コンパイル成功。

- [ ] **Step 3: 手動確認**

ローカル起動後、`curl -X POST http://127.0.0.1:8080/chat -d '{"message":"test"}'` を投げ、即座に別端末で `curl -s http://127.0.0.1:8080/api/queue` を高速ポーリングし、`Waiting`→`Executing` の遷移が観測できること（`--no-agent` では Executing は一瞬）。

- [ ] **Step 4: コミット**

```bash
git add crates/rustyclaw-gateway/src/lib.rs
git commit -m "feat(dashboard): record Waiting state on event receipt (ISSUE-17)"
```

---

## 完了確認（全 STEP 後）

- [ ] `cargo build -p rustyclaw-gateway` 成功
- [ ] `cargo test -p rustyclaw-storage` / `cargo test -p rustyclaw-gateway` 全 PASS
- [ ] `scripts/deploy.sh` で rp1 反映 → `ssh rp1 'curl -s http://127.0.0.1:8080/api/schedule'` と `?gran=600&from=...` の timeline が期待どおり返る
- [ ] ブラウザ（rp1 IP:8080）で 1D グラフ・SCHED 表示・IP:PORT ヘッダを目視確認

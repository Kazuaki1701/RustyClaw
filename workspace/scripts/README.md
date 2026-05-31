# RustyClaw ワークスペース・スクリプト説明書 (README)

本ディレクトリ（`workspace/scripts/`）には、エージェントが特定の複雑なタスクを一括処理し、LLM トークン消費量を節約するためのカスタムスクリプトが配置されています。

エージェントは `run_workspace_script` ツールを使用し、これらのスクリプトを実行して結果を直接回収することができます。

---

## 利用可能なスクリプト一覧

### 1. Garmin バイタルデータ一括取得スクリプト
* **ファイル名**: `500_get-vital-data-garmin.sh`
* **実行目的**: 
  Home Assistant API と連携し、Garmin ウェアラブルデバイスから同期された心拍、睡眠時間、歩数などのヘルスセンサー情報（`sensor.garmin_*`）の最新値を一括取得し、クリーンな JSON 形式で出力します。
* **使い方 (引数なし)**:
  `run_workspace_script(script_name: "500_get-vital-data-garmin.sh")`
* **前提条件**: 環境変数 `HOMEASSISTANT_TOKEN` が設定されていること。

### 2. Karakeep RSS 自動クリーンアップスクリプト
* **ファイル名**: `501_karakeep-cleanup.sh`
* **実行目的**: 
  ブックマーク管理ツール Karakeep 内の RSS 由来アイテムのうち、「2週間（14日間）以上経過」かつ「お気に入りに登録されていない（`favourited == false`）」かつ「保護タグ（`_bookmarked`, `_star`, `_doitlater`, `_recommended`）が付与されていない」古いブックマークを自動的に一括削除します。
* **使い方 (引数なし)**:
  `run_workspace_script(script_name: "501_karakeep-cleanup.sh")`
* **前提条件**: 環境変数 `KARAKEEP_API_KEY` および `KARAKEEP_SERVER_ADDR` が設定されていること。

### 3. Karakeep ブックマーク一括タグ付けスクリプト
* **ファイル名**: `502_karakeep-tag-items.sh`
* **実行目的**: 
  指定された複数のブックマークIDに対して、特定のタグ（例: `_bookmarked` や `_doitlater`）を一括で高速に付与します。ネイティブAPIを個別に何十回も呼ぶのを防ぎます。
* **使い方 (引数が必要)**:
  * 第一引数: 付与するタグ名
  * 第二引数以降: 対象となるブックマークIDリスト（スペース区切り）
* **実行例**:
  `run_workspace_script(script_name: "502_karakeep-tag-items.sh", args: ["_doitlater", "12345", "67890"])`

### 4. Google Cloud プロジェクト & API セットアップ
* **ファイル名**: `setup-gog.sh`
* **実行目的**: 
  Google Workspace 連携（Gmail / Calendar）のための Google Cloud プロジェクトの作成、必要な API の有効化、Desktop OAuth クライアント認証情報のセットアップを半自動化する対話型スクリプトです。
* **注意**: インタラクティブなブラウザログインやエンターキー入力を伴うため、**LLM が自律実行することは推奨されません。** 開発者（K様）が環境構築時に実行するものです。

### 5. TypeScript テンプレート埋め込みスクリプト
* **ファイル名**: `embed-templates.ts`
* **実行目的**: 
  システム用の TypeScript ファイルなどにテンプレートコンテンツを埋め込むための補助スクリプトです。
* **注意**: Bun ランタイムがインストールされている環境で使用可能です（`bun run embed-templates.ts`）。

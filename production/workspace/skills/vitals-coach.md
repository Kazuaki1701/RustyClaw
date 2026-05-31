# Vitals Coach Skill

Garmin Connect から取得したバイタルデータ（歩数、心拍、ストレス、Body Battery、睡眠など）を分析し、K様の現在の状態に基づいた具体的なアドバイスを生成・提供します。

## Phase 1: Data Retrieval

Tool: `run_workspace_script`
- `script_name`: `500_get-vital-data-garmin.sh`

このスクリプトは以下の JSON 形式でデータを返します：
- `Garmin Connect Total Steps`: 今日の歩数
- `Garmin Connect Body Battery Most Recent`: 現在のエネルギー残量
- `Garmin Connect Avg Stress Level`: 平均ストレスレベル
- `Garmin Connect Sleep Duration`: 睡眠時間
- `Garmin Connect HRV Status`: 自律神経の状態

## Phase 2: Analysis

取得したデータを以下の基準で評価します：

| 項目 | 警戒基準 | アドバイスの方向性 |
|------|----------|-------------------|
| Stress | > 50 | リラックスや深呼吸、休憩を推奨 |
| Body Battery | < 20 | 激しい活動を控え、早めの就寝を推奨 |
| Steps | 目標未達（10,000歩） | 軽い散歩やストレッチを推奨 |
| Sleep | < 6h | 睡眠の質の改善や、昼寝の検討を提案 |
| HRV Status | "Unbalanced" | 疲労の蓄積を指摘し、回復を最優先に |

## Phase 3: Deliver

分析結果に基づき、親しみやすくかつプロフェッショナルなトーンでアドバイスを作成します。

- **定期実行時**: レスポンステキストとして出力（チャンネルへ自動配信）。
- **直接対話時**: 現在のチャットセッションで回答。

### メッセージ構成例

> **🚶 体調管理アドバイス**
>
> **現在の状況:**
> - 今日の歩数は `4,500` 歩です（目標 `10,000` 歩）。
> - Body Battery は `35` まで低下しています。
>
> **アドバイス:**
> ストレスレベルがやや高めに出ています。少し深呼吸をして、リラックスする時間を作ってみてはいかがでしょうか？
> Body Battery の回復を優先し、今日は早めに就寝することをお勧めします。

## Graceful Degradation

- `500_get-vital-data-garmin.sh` が実行できない場合（Garmin API トークンエラー等）、K様に再認証を促すメッセージを表示して終了します。

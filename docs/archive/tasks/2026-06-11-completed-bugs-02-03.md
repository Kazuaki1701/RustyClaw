> [!IMPORTANT]
> **ステータス**: `[HISTORICAL]` (過去の完了済みタスク)  
> **完了日**: 2026-06-11  
> **備考**: 最新の動作仕様については、`docs/specs/` 配下の最新仕様書を参照してください。

# 完了済みタスク — 2026-06-11 (BUG-02, BUG-03)

## バグ修正 (BUG-02, BUG-03)

### BUG-02: `gws_calendar_list_events` 未定義ツール呼び出しによるセッションクラッシュ
- **完了日**: 2026-06-11
- **概要**: 
  - **BUG-02-a**: `daily-briefing.md` の GWS カレンダーツール呼び出しを、`run_workspace_script` 経由の `calendar-ops.sh` と `506_get-gmail.sh` に修正。`calendar-ops.sh` のデフォルトコマンドを `list_family` に設定してクラッシュを防止。
  - **BUG-02-b**: GWS カレンダー連携を廃止してスクリプト経由に統一。`config.release.json` / `config.debug.json` から `google-workspace` ツール設定を削除。
  - **BUG-02-c**: `UnknownToolCall` を捕捉してセッションを終了させる代わりにエラーメッセージをモデルにフィードバックして代替ツールを使わせる自動回復ロジックを `rustyclaw-agent` に実装。
- **関連ファイル**: 
  - [daily-briefing.md](file:///home/kazuaki/Projects/RustyClaw/workspace/skills/daily-briefing.md)
  - [SKILL.md](file:///home/kazuaki/Projects/RustyClaw/production/workspace/skills/daily-briefing/SKILL.md)
  - [calendar-ops.sh](file:///home/kazuaki/Projects/RustyClaw/calendar-ops.sh)
  - [config.release.json](file:///home/kazuaki/Projects/RustyClaw/production/config/config.release.json)
  - [config.debug.json](file:///home/kazuaki/Projects/RustyClaw/production/config/config.debug.json)
  - [lib.rs (rustyclaw-agent)](file:///home/kazuaki/Projects/RustyClaw/crates/rustyclaw-agent/src/lib.rs)

### BUG-03: `yolp_weather` 廃止対応と気象庁・Open-Meteo ハイブリッド天気予報への移行
- **完了日**: 2026-06-11
- **概要**:
  - **BUG-03-a**: `504_get-weather.sh` を改修し、`city` から `lat`/`lon` へマッピングする処理と、Open-Meteo API からの 15分刻み降水量予測のフェッチ・JSONマージ処理を追加。
  - **BUG-03-b**: 指示書内の `yolp_weather` への参照を削除し、`run_workspace_script` 経由の `504_get-weather.sh` 呼び出しに更新。
- **関連計画書**: [2026-06-11-weather-hybrid-migration.md](file:///home/kazuaki/Projects/RustyClaw/docs/plans/2026-06-11-weather-hybrid-migration.md)
- **関連ファイル**:
  - [504_get-weather.sh](file:///home/kazuaki/Projects/RustyClaw/production/workspace/skills/weather/scripts/504_get-weather.sh)
  - [daily-briefing.md](file:///home/kazuaki/Projects/RustyClaw/workspace/skills/daily-briefing.md)
  - [SKILL.md](file:///home/kazuaki/Projects/RustyClaw/production/workspace/skills/daily-briefing/SKILL.md)

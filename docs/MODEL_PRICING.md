# 預設喚醒模型與官方價格

2026-10-05 Codex 核對。價格單位為美元／百萬 token，使用標準同步 API、一般短上下文、未快取文字輸入；不套用 Batch、Flex、Fast 或其他折扣。API 單價是選型依據，訂閱 CLI 的額度扣除不一定等同 API 帳單。

| 目標 | 本工具預設模型 | 輸入 | 輸出 | 選擇依據 |
| --- | --- | ---: | ---: | --- |
| AGY Gemini | gemini-3.8-flash，Low | 0.75 | 3.75 | AGY 模型清單內 3.6／3.7／3.8 Flash 目前同價，選已驗證的 3.8；Flash-Lite 雖更便宜，但本機 AGY 未提供 |
| Codex | gpt-6-luna，Low | 0.10 | 0.50 | 本機支援的低價 Luna 模型，明確指定以免跟隨高價 CLI 預設 |
| Claude | claude-haiku-4-5-20251001 | 1.00 | 5.00 | 官方現役 Claude 中最低的標準輸入與輸出單價；不傳不支援的 effort |
| AGY Claude／GPT | claude-sonnet-5-5-low | 2.00 | 10.00 | Sonnet 較 Opus 便宜；此為 Anthropic API 參考價，非 AGY 官方扣額報價 |

Google Flash 的表列促銷價格到 2026-12-31；2027-01-01 起官方目前列為輸入 1.50／輸出 7.50。請日後再次查價。

AGY 沒有公開完整的 Claude／GPT 模型扣額比較，因此不能宣稱 Sonnet 是該池所有模型中的絕對最低。GPT-OSS 120B 有提供，但無可核對的 AGY 官方單價，且官方標示將於 2026-11-02 移除，未改為預設。AGY 官方說明 Gemini 共享額度依 API 定價扣除，非 Gemini 模型則使用獨立固定限制。

新設定直接顯示預設模型；舊設定的空白模型也會使用本工具預設。使用者已明確選擇的其他模型與額外參數仍保留。節省 Token 模式預設開啟，停用可停用的工具／MCP／規則，使用低推理並只要求回覆 OK。Haiku 不支援 adaptive effort，因此 UI 與參數只保留預設程度。

## 官方來源

- [OpenAI API pricing](https://developers.openai.com/api/docs/pricing)
- [Claude pricing](https://platform.claude.com/docs/en/about-claude/pricing)
- [Haiku 4.5 模型規格](https://platform.claude.com/docs/en/models/haiku-4-5/overview)
- [Gemini API pricing](https://ai.google.dev/gemini-api/docs/pricing)
- [AGY 模型可用性與 GPT-OSS 移除日期](https://antigravity.google/docs/models)
- [AGY 共享額度計價說明](https://antigravity.google/blog/changes-to-antigravity-plans)
- [AGY 現行方案與額度](https://antigravity.google/docs/plans)

## 實際驗證

`dotnet run --project tests/AiWakeScheduler.Tests/AiWakeScheduler.Tests.csproj -c Release -- --wake-smoke` 使用空白 CliProfile，確保走真正預設。四個模型全部 exit 0 且回覆 OK，1/1 通過。核心舊參數期望與視窗隔離測試已更新為新預設，完整建置驗證結果見 FEATURE_VERIFICATION.md。

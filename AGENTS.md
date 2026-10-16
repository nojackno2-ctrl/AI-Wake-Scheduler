# AI 協作規則

- 開始工作前先讀取本檔與 `AI_HANDOFF.md`，並檢查 Git 狀態與差異（若此目錄已初始化為 Git 儲存庫）。
- `AI_HANDOFF.md` 是即時交接紀錄；發現重要證據、完成程式變更、執行建置或測試後立即更新。
- 不得覆蓋或捨棄使用者及其他代理人的未提交變更。
- 未經明確授權，不得進行破壞性操作（force-push、merge、rebase、reset、刪除分支或發布版本）；完成任務驗證後自動 commit 與 push 已獲授權（詳見後文）。
- 未實際執行驗證前，不得宣稱建置、測試或功能成功。
- 專案以繁體中文 UI、純 Rust 語言（Cargo workspace、原生 Win32 API 控制項）、零額外執行階段依賴為優先。

## Automatic commits and push to GitHub (user authorization, 2026-10-14)

- The user has authorized automatic local commits and automatic push to GitHub for all projects.
- After completing a task and appropriate verification, commit the task changes and push to GitHub (`origin`) without asking for confirmation again; do not create empty commits.
- Review the diff and preserve existing work. Include unrelated pre-existing changes only when the user explicitly requests committing them. Never commit secrets, credentials, or personal runtime data.
- This standing authorization covers local commits and pushing verified work to GitHub. Destructive operations (force-push, release, merge, rebase, reset, branch deletion) still require explicit authorization.
- A Git `post-commit` hook is configured in `.git/hooks/post-commit` to automatically push the current branch to `origin` upon commit.
- Record what was verified and any unverified behavior in `AI_HANDOFF.md`; never present a commit as proof that functionality works.

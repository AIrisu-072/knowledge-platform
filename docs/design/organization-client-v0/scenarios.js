window.ORG_SCENARIOS = {
  "archetypes": [
    "context",
    "queue"
  ],
  "scenarios": [
    {
      "id": "normal",
      "label": "通常",
      "attention": "対応中",
      "work": "内容を確認し、次の対応を判断します",
      "module": "evidence"
    },
    {
      "id": "newly_assigned",
      "label": "新規Assignment",
      "attention": "新しい担当",
      "work": "担当に割り当てられました。提出済みの資料から開始します",
      "module": "history"
    },
    {
      "id": "returned",
      "label": "差戻",
      "attention": "差戻対応 · 試行2",
      "work": "前回提出は保持されています。新しい非公開下書きで追加確認します",
      "module": "return"
    },
    {
      "id": "working_draft",
      "label": "Working Draft",
      "attention": "下書き · 次工程には非公開",
      "work": "編集中の内容は現在の担当責任の範囲だけで共有されます",
      "module": "resources"
    },
    {
      "id": "handed_off",
      "label": "Handoff済み",
      "attention": "提出済み · 次工程へ",
      "work": "提出時の内容は固定されています。次工程の下書きは表示しません",
      "module": "history"
    },
    {
      "id": "due_soon",
      "label": "期限接近",
      "attention": "期限接近 · 業務別設定",
      "work": "期限と未解決事項を確認し、次の操作を決めます",
      "module": "evidence"
    },
    {
      "id": "blocked",
      "label": "Blocked",
      "submitUnavailableReason": "原本の現在権限を確認できません。再確認が必要です。入力内容は保持されます。",
      "attention": "保留 · 原本の現在権限を確認できません",
      "work": "取得失敗を0件や確認完了とは扱いません。入力内容は保持されます",
      "module": "document"
    },
    {
      "id": "agent_active",
      "label": "Agent利用中",
      "attention": "Agent調査中 · 業務は未完了",
      "work": "Agentの実行と業務の状態は別です。他の作業を続けられます",
      "module": "agent"
    },
    {
      "id": "evidence_review",
      "label": "Evidence確認中",
      "attention": "根拠を確認中",
      "work": "候補と根拠を確認してから、人間の判断を記録します",
      "module": "evidence"
    },
    {
      "id": "document_compare",
      "label": "Document比較中",
      "attention": "比較 · 未比較箇所あり",
      "work": "原本と改訂を確認します。Partial / Unknownを変更なしにしません",
      "module": "document"
    }
  ]
};

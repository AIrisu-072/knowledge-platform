'use strict';
(() => {
  const $ = id => document.getElementById(id);
  const data = window.ORG_SCENARIOS;
  const context = document.documentElement.dataset.archetype === 'context';
  const initial = new URLSearchParams(location.search);
  let scenario = data.scenarios.find(s => s.id === initial.get('scenario')) ?? data.scenarios[0];
  let currentModule = initial.get('module') || scenario.module;
  let selectedId = context ? 'context-sales-001' : 'task-review-001';
  let eligibilityOnly = false;
  let workType = '内容確認';
  let draft = '確認事項と説明内容をまとめています。追加資料の確認後に提出します。';
  const draftByItem = new Map();
  const stateByItem = new Map();
  const claimedItems = [];
  let currentRows = [];
  let decision = null;
  let agentCompleted = false;
  let agentPrompt = '';
  let returnReason = '資料の確認日が異なるため、原本で確認してください';
  let submission = null;
  let comparisonOpen = scenario.id === 'document_compare';
  let outgoingReturn = false;
  let returnFocus = null;
  let dialogTargetId = null;
  let dialogAction = null;
  const modules = ['evidence', 'agent', 'document', 'search', 'return', 'history', 'resources'];
  if (!modules.includes(currentModule)) currentModule = scenario.module;
  const escape = value => String(value).replace(/[&<>"']/g, c => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' })[c]);
  const notify = message => { $('notice').textContent = `設計デモ：${message}`; };

  function saveItem() {
    stateByItem.set(selectedId, { scenarioId: scenario.id, module: currentModule, draft, decision, agentCompleted, agentPrompt, returnReason, submission, comparisonOpen, outgoingReturn });
  }
  function loadItem(id) {
    selectedId = id;
    const saved = stateByItem.get(id);
    scenario = data.scenarios.find(s => s.id === saved?.scenarioId) ?? data.scenarios[0];
    currentModule = saved?.module ?? scenario.module;
    draft = saved?.draft ?? draftByItem.get(id) ?? 'このタスクの確認事項をまとめています。';
    decision = saved?.decision ?? null;
    agentCompleted = saved?.agentCompleted ?? false;
    agentPrompt = saved?.agentPrompt ?? '';
    returnReason = saved?.returnReason ?? '資料の確認日が異なるため、原本で確認してください';
    submission = saved?.submission ?? null;
    comparisonOpen = saved?.comparisonOpen ?? scenario.id === 'document_compare';
    outgoingReturn = saved?.outgoingReturn ?? false;
  }
  function selectedLabel() { return currentRows.find(row => row[0] === selectedId)?.[1] ?? '現在のタスク'; }
  function focusKey(node) {
    return { node, id: node?.id, action: node?.dataset?.action, decision: node?.dataset?.decision, region: node?.closest?.('#context-body') ? 'context-body' : null };
  }
  function focusReplacement(key) {
    if (key?.node?.isConnected && !key.node.disabled) return key.node;
    const root = key?.region ? $(key.region) : document;
    const replacement = key?.id ? $(key.id) : key?.decision
      ? [...root.querySelectorAll('[data-decision]')].find(node => node.dataset.decision === key.decision)
      : key?.action ? [...root.querySelectorAll('[data-action]')].find(node => node.dataset.action === key.action) : null;
    return replacement && !replacement.disabled ? replacement : $('work-surface');
  }
  function setUrl() {
    const url = new URL(location.href);
    url.searchParams.set('scenario', scenario.id);
    url.searchParams.set('module', currentModule);
    history.replaceState(null, '', url);
  }
  function renderCollection() {
    $('archetype-label').textContent = context ? 'CONTEXT VIEW · 営業型' : 'QUEUE VIEW · 事務型';
    $('collection-title').textContent = context ? '担当する文脈' : '処理するタスク';
    const queuePrefix = { '内容確認': 'task-review', '再確認': 'task-recheck', '書類照合': 'task-reconcile' }[workType];
    const rows = eligibilityOnly
      ? [[`eligible-${queuePrefix}-001`, `${workType} 候補01`, '担当可能 · 内容は担当確定後'], [`eligible-${queuePrefix}-002`, `${workType} 候補02`, '担当可能 · 対象名は非開示']].filter(row => !claimedItems.some(claimed => claimed[0] === row[0]))
      : context
        ? [['context-sales-001', 'サンプル商事', '設備更新相談 · 内容確認'], ['context-sales-002', 'テスト工業', '定期相談 · 回答待ち'], ['context-sales-003', '合成販売', '資料作成 · 期限接近']]
        : [[`${queuePrefix}-001`, `${workType} #01`, '自分の担当 · 審査Profile'], [`${queuePrefix}-002`, `${workType} #02`, '自分の担当 · 新規'], [`${queuePrefix}-003`, `${workType} #03`, '自分の担当 · 差戻']];
    if (!eligibilityOnly) rows.unshift(...claimedItems.filter(row => context || row[3] === workType));
    currentRows = rows;
    if (!rows.some(r => r[0] === selectedId) && rows.length) { saveItem(); loadItem(rows[0][0]); }
    $('collection-list').innerHTML = rows.map(([id, title, subtitle]) => `<button type="button" class="collection-row" data-item="${id}" aria-pressed="${id === selectedId}"><strong>${title}</strong><span>${subtitle}</span></button>`).join('');
    $('collection-foot').textContent = eligibilityOnly ? '候補者用の最小表示です。顧客名・下書き・根拠は担当可能という理由だけでは開示しません。' : context ? '案件と関連タスクは同じWorkContext × WorkItemを参照します。' : '審査は事務型のWorkViewProfileです。差戻は別の画面型ではありません。';
  }
  function renderWork() {
    const handed = scenario.id === 'handed_off';
    if (handed && !submission) submission = Object.freeze({ seeded: true, draft: '合成の既存提出：確認資料を照合済み', evidenceCount: 2, decision: Object.freeze({ kind: '採用', text: '合成Fixtureとして事前に記録された人間の判断' }) });
    const emptyQueue = eligibilityOnly && currentRows.length === 0;
    const blocked = scenario.id === 'blocked';
    $('responsibility').textContent = context ? '営業担当として作業' : '融資審査担当として作業';
    $('principal').textContent = context ? 'sales-01' : 'review-01';
    const label = selectedLabel();
    $('breadcrumb').textContent = eligibilityOnly ? '担当可能なタスク / 内容は未開示' : `タスク / ${label} / ${context ? '担当文脈' : '担当済み'}`;
    $('work-title').textContent = eligibilityOnly ? (emptyQueue ? '担当可能なタスクはありません' : `${label}を担当する`) : context ? `${label}の次の対応` : `提出資料の${label}`;
    $('work-kind').textContent = eligibilityOnly ? 'Queue projection · 非公開内容なし' : context ? 'Case · WorkItem 内容確認' : `RoutineRun · WorkType ${workType}`;
    $('attention-label').textContent = eligibilityOnly ? (emptyQueue ? '0件 · 現在の権限で取得済み' : '担当可能 ≠ 担当中') : outgoingReturn ? '差戻指示済み · 現在の試行は完了' : scenario.attention;
    $('attention-label').className = `status ${handed ? 'success' : blocked || scenario.id === 'returned' || scenario.id === 'due_soon' ? 'warning' : ''}`;
    $('state-description').textContent = eligibilityOnly ? '担当を確定するまで、下書き・資料・Evidenceは取得しません。' : outgoingReturn ? '差戻先の新しい非公開下書きは表示しません。現在の提出・指示を保持します。' : scenario.work;
    $('submit-action').textContent = eligibilityOnly ? '担当する' : outgoingReturn ? '差戻指示を確認' : handed ? '提出内容を確認' : '提出';
    $('submit-action').disabled = emptyQueue || (blocked && !eligibilityOnly);
    for (const id of ['hold-action', 'return-action', 'assignment-action']) $(id).disabled = eligibilityOnly || handed || outgoingReturn;
    if (eligibilityOnly) {
      $('work-body').innerHTML = '<section class="section"><h2>内容は担当確定後</h2><p>閲覧できる業務種別と担当可能状態だけを表示しています。</p><p class="subtle">競合した場合は担当未確定を示し、別のタスクへ自動で切り替えません。</p></section>';
      return;
    }
    const summary = context
      ? '<h2>この文脈の進み具合</h2><dl class="definition"><dt>現在</dt><dd>内容確認。次は審査担当へ提出</dd><dt>自分の対応</dt><dd>確認事項と説明資料をまとめる</dd><dt>次工程</dt><dd>審査 → 承認。進捗のみ閲覧可能</dd></dl>'
      : '<h2>今回確認すること</h2><dl class="definition"><dt>確認対象</dt><dd>提出スナップショット #1</dd><dt>作業</dt><dd>資料の整合性と不足事項の確認</dd><dt>Profile</dt><dd>事務型 · Evidence／文書を優先</dd></dl>';
    const returnStrip = scenario.id === 'returned' ? '<div class="warning-panel"><strong>前回提出 #1 は変更しません</strong><p>新しい試行2の下書きです。差戻理由は右のContext Surfaceから確認できます。</p></div>' : '';
    const compare = comparisonOpen ? '<section class="section"><h2>文書比較</h2><p class="subtle">Document機能の比較契約を再利用 · 改訂4.0 → 4.1</p><div class="diff"><section><h3>改訂4.0</h3><p>確認資料を<del>提出前に確認</del>する。</p></section><section><h3>改訂4.1</h3><p>確認資料を<ins>提出前に相互照合</ins>する。</p></section></div><div class="warning-panel">Partial：図の領域は未比較です。変更なしとは判断できません。</div><button type="button" data-action="open-original">両方の原本を確認</button><button type="button" data-action="close-compare">比較を閉じる</button></section>' : '';
    const working = handed
      ? `<section class="section"><h2>提出スナップショット #1</h2><span class="status success">handoff · 提出時の内容を固定</span><p class="spaced">確認メモ、根拠${submission.evidenceCount}件、人間の判断${submission.decision ? 1 : 0}件を提出済みです。下書きへの参照ではありません。</p><p>${escape(submission.draft)}</p><p class="subtle">${submission.seeded ? 'この状態は明示的に事前登録した合成Fixtureです。' : '確認操作で選択した内容だけを固定した設計プレビューです。'} 次工程の進捗：ready。次担当の非公開作業は表示しません。</p><button type="button" data-action="show-history">提出履歴を確認</button></section>`
      : outgoingReturn ? `<section class="section"><h2>差戻指示を記録しました</h2><span class="status success">現在の試行は完了 · 読み取り専用</span><p class="spaced">${escape(returnReason)}</p><p>差戻先の試行と下書きは、その現在の担当者の権限で保護されます。</p><button type="button" data-action="show-history">過去提出と指示の履歴</button></section>`
      : `<section class="section"><h2>作業メモ</h2><span class="status">work_item_private · 次工程には非公開</span><div class="field"><label for="draft">現在の試行の下書き</label><textarea id="draft">${escape(draft)}</textarea></div><p class="subtle">Workの現在認可で保護。既存の共有文書を非公開下書きと言い換えません。</p><div class="artifact-row"><div><strong>確認メモ.txt</strong><span class="subtle">Work-owned generation · 非公開 · 180B（合成）</span></div><button type="button" data-action="show-resources">資料を確認</button></div></section>`;
    $('work-body').innerHTML = `<section class="section">${summary}</section>${returnStrip}${compare}${working}`;
    $('draft')?.addEventListener('input', event => { draft = event.target.value; draftByItem.set(selectedId, draft); });
  }

  function evidenceHtml() {
    const visibleDecision = scenario.id === 'handed_off' ? submission?.decision : decision;
    const decisionRecord = visibleDecision ? `<div class="record"><strong>HumanDecision · ${escape(visibleDecision.kind)}</strong><p>${escape(visibleDecision.text)}</p><span>実行者 ${context ? 'sales-01' : 'review-01'} · 元のFindingは保持</span></div>` : '<p class="subtle">人間の判断はまだ記録されていません</p>';
    return `<p class="eyebrow">EVIDENCE + FINDING + HUMAN DECISION</p><h2>候補と根拠を分けて確認</h2><div class="finding"><div class="label">Finding · 判断候補（元の主張）</div><strong>追加確認が必要な可能性があります</strong><p>2つの資料で確認日の表記が異なります。原本を確認して判断してください。</p><span class="status warning">不確実性あり · AIの確定判断ではありません</span></div><div class="evidence-item"><span class="source">EvidenceRecord · 人間が登録</span><strong>資料サンプル P.12</strong><p>確認日：2026/09/29。原本の該当箇所への参照</p><div class="source">Document · 改訂2.0 / Version2<br>取得 2026/10/02 10:00 Asia/Tokyo（UTC+09:00）<br>Coverage：該当箇所のみ · 全資料の確認ではありません</div><button type="button" data-action="open-original">原本・位置を確認</button></div><div class="evidence-item"><span class="source">EvidenceRecord · 検索で発見</span><strong>規程サンプル 改訂4.1 §8</strong><p>資料の確認日を相互照合する旨の記載</p><div class="source">authoritative locatorあり · providerの現在認可を再確認<br>原文は非保持。参考位置から原本を開きます</div></div><h3>人間の判断</h3><div class="decision-actions"><button type="button" data-decision="採用" ${scenario.id === 'handed_off' || outgoingReturn ? 'disabled' : ''}>採用</button><button type="button" data-decision="修正" ${scenario.id === 'handed_off' || outgoingReturn ? 'disabled' : ''}>修正</button><button type="button" data-decision="却下" ${scenario.id === 'handed_off' || outgoingReturn ? 'disabled' : ''}>却下</button></div>${decisionRecord}<p class="subtle">判断の採用は、提出・承認の実行とは別です</p>`;
  }
  function agentHtml() {
    return `<p class="eyebrow">AGENT CHAT · CURRENT TASK</p><h2>このタスクについて調べる</h2><p class="subtle">現在のTask・責任・許可された資料だけを文脈に含めます</p><div class="chat-turn"><strong>あなた · ${context ? '営業担当' : '融資審査担当'}</strong>${escape(agentPrompt || '提出資料と関連規程を確認し、不足している確認事項を挙げてください。')}</div><div class="chat-turn agent"><strong>AgentExecution · ${agentCompleted ? '結果プレビュー' : '調査中プレビュー'}</strong><p>${agentCompleted ? '候補1件と根拠2件を整理しました。追加確認が必要か、人間が判断してください。' : '許可された資料を確認しています。調査中でも他の作業を続けられます。'}</p><button type="button" data-action="agent-result">${agentCompleted ? 'Findingと根拠を見る' : '構造化結果の状態を見る'}</button></div><div class="field"><label for="agent-prompt">追加の依頼</label><textarea id="agent-prompt" placeholder="このタスクについて依頼する">${escape(agentPrompt)}</textarea></div><button type="button" data-action="agent-request">依頼内容をプレビュー</button><button type="button" data-action="agent-cancel">実行停止をプレビュー</button><details><summary>文脈・実行者・権限の範囲</summary><p>requester：${context ? 'sales-01' : 'review-01'}<br>executor：organization-synthetic/agent-01<br>Document provider：poc/poc-agent（別Identity）</p><p>現在の依頼者・実行者・provider権限の積集合。Chat本文は業務正本ではありません。</p></details>`;
  }
  function documentHtml() {
    if (scenario.id === 'blocked') return '<p class="eyebrow">DOCUMENT · CURRENT AUTHORIZATION</p><h2>原本を現在確認できません</h2><div class="warning-panel">providerの現在認可が不明です。古い検索結果や提出参照があっても内容を開示しません。</div><button type="button" data-action="retry-provider">再確認</button><p class="subtle spaced">下書きは保持。0件・確認完了・変更なしへ置き換えません。</p>';
    return '<p class="eyebrow">DOCUMENT FEATURE · REUSED</p><h2>原本と改訂</h2><dl class="definition"><dt>文書</dt><dd>規程サンプル</dd><dt>正式改訂</dt><dd>改訂4.1</dd><dt>内容世代</dt><dd>DocumentVersion #4</dd><dt>比較</dt><dd>4.0 → 4.1 · Partial</dd></dl><p class="subtle">OCC revisionは内部競合検知値。正式改訂番号と混同しません。</p><div class="warning-panel">未比較箇所あり。Unknown / Partialを変更なしへ変換しません。</div><button type="button" data-action="compare">主作業で比較する</button><button type="button" data-action="open-original">原本を確認</button><details><summary>再利用するDocument境界</summary><p>既存の型付きclient・BinaryTransportBridge・認可・Version/Revision・比較projectionを再利用します。このSource Designは原本をparseしません。</p></details>';
  }
  function searchHtml() {
    return '<p class="eyebrow">SEARCH PLATFORM · INTEGRATION CONTRACT</p><h2>関連情報を探す</h2><div class="field"><label for="search-query">現在のタスク内で検索</label><input id="search-query" value="確認資料 相互照合"></div><button type="button" data-action="search-preview">検索状態をプレビュー</button><div class="evidence-item"><strong>規程サンプル 改訂4.1 §8</strong><p>SearchResultは発見情報です。Evidenceへの登録は出所・位置・coverageを保持した別操作です。</p><span class="status warning">Partial · 一部Source未取得</span></div><button type="button" data-action="register-evidence">根拠として確認</button><p class="subtle spaced">Discovery ≠ Authorization ≠ Execution<br>Search WIPの稼働・検索成功は主張しません</p>';
  }
  function returnHtml() {
    if (outgoingReturn) return `<p class="eyebrow">RETURN INSTRUCTION · SENT</p><h2>差戻指示済み</h2><p>${escape(returnReason)}</p><p>現在の試行は完了しています。次担当の新しい非公開下書きは表示しません。</p><button type="button" data-action="show-history">提出と指示の履歴を確認</button>`;
    if (scenario.id !== 'returned') return '<p class="eyebrow">RETURN CONTEXT</p><h2>差戻指示はありません</h2><p>差戻は独立画面ではなく、同じタスクの文脈です。</p>';
    return `<p class="eyebrow">RETURN / REWORK · ATTEMPT2</p><h2>前回提出からの追加確認</h2><dl class="definition"><dt>前回提出</dt><dd>Snapshot #1 · 試行1（完了）</dd><dt>指示</dt><dd>${escape(returnReason)}</dd><dt>新しい作業</dt><dd>試行2 · work_item_private</dd><dt>関係</dt><dd>前回提出 #1 → 差戻指示 #1 → 試行2</dd></dl><div class="warning-panel">前回の完了履歴・提出内容は上書きしません。再提出までは次工程に新しい下書きを開示しません。</div><button type="button" data-action="show-history">前回提出と履歴を確認</button>`;
  }
  function historyHtml() {
    return `<p class="eyebrow">WORKFLOW HISTORY · AUTHORIZED PROJECTION</p><h2>進捗と提出の履歴</h2><ol class="history"><li><time>2026/10/02 09:00 Asia/Tokyo（UTC+09:00）</time><strong>内容確認 · 担当確定</strong><br>現在の責任で作業を開始</li><li><time>2026/10/01 16:00 Asia/Tokyo（UTC+09:00）</time><strong>受付 · 提出スナップショット #1</strong><br>提出時点の内容を固定</li>${scenario.id === 'returned' ? '<li><time>2026/10/02 10:00 Asia/Tokyo（UTC+09:00）</time><strong>差戻指示 #1 → 新しい試行2</strong><br>試行1は完了のまま保持</li>' : ''}</ol><p class="subtle">context.history.readの範囲だけを表示します。別担当者の下書き・理由本文・Identityは追加の現在権限なしに表示しません。</p>`;
  }
  function resourcesHtml() {
    return '<p class="eyebrow">EFFECTIVE WORKSPACE · LOGICAL CONTEXT</p><h2>このタスクの資料</h2><p>WorkspaceはTaskから自動解決しています</p><dl class="definition"><dt>Workspace</dt><dd>設備更新相談の作業環境</dd><dt>managed</dt><dd>端末内の管理領域 · principal/device-local</dd><dt>explicit</dt><dd>添付資料フォルダ · opaque binding</dd><dt>policy-derived</dt><dd>現在の責任で閲覧できる規程</dd></dl><div class="artifact-row"><div><strong>資料サンプル</strong><span class="subtle">InputResourceRef · 既存共有資料。非公開下書きではありません</span></div></div><div class="artifact-row"><div><strong>ローカル確認メモ</strong><span class="subtle">端末内のみ · 引継ぎ対象にできません</span></div><button type="button" data-action="promote">Workへアップロード</button></div><label class="subtle"><input type="checkbox" id="enabled-binding" checked class="inline-checkbox"> Workspaceの通常探索で使用する</label><p class="subtle">AccessibleとEnabledは別。切替で権限は変わりません。</p><button type="button" data-action="create-workspace">新しいWorkspace</button><details><summary>Runtimeの状態</summary><p>この設計デモはブラウザーです。実フォルダ選択・managed root作成は行いません。v0 Browser adapterが未対応なら機能をUnavailableとして示します。</p></details>';
  }
  function renderModule() {
    const previousFocus = focusKey(document.activeElement);
    document.querySelectorAll('[data-module]').forEach(button => {
      button.setAttribute('aria-pressed', String(button.dataset.module === currentModule));
      button.disabled = eligibilityOnly;
    });
    if (eligibilityOnly) {
      $('context-body').innerHTML = '<h2>文脈情報は未開示</h2><p>担当可能であるだけでは、Evidence・Agent・資料へアクセスできません。</p>';
      return;
    }
    const renderers = { evidence: evidenceHtml, agent: agentHtml, document: documentHtml, search: searchHtml, return: returnHtml, history: historyHtml, resources: resourcesHtml };
    $('context-body').innerHTML = renderers[currentModule]();
    $('agent-prompt')?.addEventListener('input', event => { agentPrompt = event.target.value; });
    setUrl();
    if (previousFocus.region && !previousFocus.node.isConnected) focusReplacement(previousFocus).focus();
  }
  function render() { renderCollection(); renderWork(); renderModule(); $('scenario').value = scenario.id; }
  function openModule(name) { currentModule = name; renderModule(); }
  function setScenario(id) {
    scenario = data.scenarios.find(s => s.id === id) ?? data.scenarios[0];
    currentModule = scenario.module;
    comparisonOpen = id === 'document_compare'; outgoingReturn = false;
    render();
  }
  function openDialog(title, body, action, trigger) {
    returnFocus = focusKey(trigger ?? document.activeElement);
    dialogTargetId = selectedId;
    dialogAction = action;
    $('dialog-title').textContent = title;
    $('dialog-body').innerHTML = body;
    $('dialog-confirm').disabled = action === 'workspace';
    $('action-dialog').showModal();
    $('dialog-cancel').focus();
  }
  function closeDialog() { $('action-dialog').close(); }
  $('action-dialog').addEventListener('close', () => {
    focusReplacement(returnFocus).focus();
  });
  $('dialog-cancel').addEventListener('click', closeDialog);
  $('dialog-confirm').addEventListener('click', () => {
    const action = dialogAction;
    if (dialogTargetId !== selectedId) { closeDialog(); notify('確認中に対象が変わったため操作を適用しません'); return; }
    const adopted = $('adopted-claim')?.value;
    const workspaceName = $('workspace-name')?.value;
    const reasonInput = $('return-reason')?.value;
    const requiredField = action === 'decision:修正' ? $('adopted-claim') : action === 'return' ? $('return-reason') : null;
    if (requiredField && !requiredField.value.trim()) {
      let error = $('dialog-error');
      if (!error) { error = document.createElement('p'); error.id = 'dialog-error'; error.setAttribute('role', 'alert'); $('dialog-body').append(error); }
      error.textContent = action === 'return' ? '差戻理由を入力してください' : '採用する内容を入力してください';
      requiredField.setAttribute('aria-invalid', 'true'); requiredField.setAttribute('aria-describedby', 'dialog-error'); requiredField.focus();
      return;
    }
    closeDialog();
    if (action === 'submit') { submission = Object.freeze({ seeded: false, draft, evidenceCount: 2, decision: decision ? Object.freeze({ ...decision }) : null }); setScenario('handed_off'); notify('提出後の状態プレビューです。実際の業務は送信していません'); }
    else if (action === 'claim') {
      const row = currentRows.find(row => row[0] === selectedId);
      if (!row) { notify('選択対象が失われたため担当を変更しません'); return; }
      if (!claimedItems.some(claimed => claimed[0] === selectedId)) claimedItems.push([selectedId, row[1], '自分の担当 · 新規Assignment', workType]);
      eligibilityOnly = false; $('collection-filter').selectedIndex = 0; setScenario('newly_assigned');
      notify('同じ選択IDの担当確定後プレビューです。実BackendではOCCと現在権限を再確認します');
    }
    else if (action?.startsWith('decision:')) {
      const kind = action.split(':')[1];
      decision = { kind, text: kind === '修正' ? adopted : kind === '採用' ? '追加確認の必要性を採用しました（設計プレビュー）' : 'この候補は採用しません（設計プレビュー）' };
      openModule('evidence'); notify('HumanDecisionの表示をプレビューしました。Finding・Evidenceは変更しません');
    } else if (action === 'workspace') notify(`Workspace「${workspaceName || '新しい作業環境'}」の作成後プレビュー。実フォルダは作成していません`);
    else if (action === 'return') { returnReason = reasonInput; outgoingReturn = true; currentModule = 'return'; render(); notify('現在の試行を完了した差戻指示のプレビュー。受け手の非公開下書きは開示しません'); }
    else notify('確認後の表示例です。サーバーやファイルへの操作は行いません');
  });
  $('submit-action').addEventListener('click', event => {
    if (eligibilityOnly) openDialog('担当を確定しますか', `<p>対象：${escape(selectedLabel())}</p><p>実行時に現在のRole/Delegationと競合を確認します。候補表示は担当確定ではありません。</p>`, 'claim', event.currentTarget);
    else if (outgoingReturn) { openModule('return'); notify('差戻指示と現在の完了状態を表示しています'); }
    else if (scenario.id === 'handed_off') { openModule('history'); notify('固定された提出内容の履歴を表示しています'); }
    else openDialog('この内容を提出しますか', `<p><strong>対象：${escape(selectedLabel())} / 現在の試行</strong></p><ul><li>確認メモの固定generation</li><li>根拠2件・人間の判断${decision ? 1 : 0}件（現在選択した内容）</li><li>次の責任：融資審査担当</li></ul><p>非公開の作業から提出Snapshotを作り、次のタスクをreadyにします。ローカルのみのファイルは対象外です。</p><div class="warning-panel">設計デモです。実装ではPending → Backend成功後だけ確定します。Unknownは成功と表示しません。</div>`, 'submit', event.currentTarget);
  });
  $('return-action').addEventListener('click', event => openDialog('前の工程へ差し戻しますか', '<p>前回提出 #1を保持し、新しい試行を作成します。</p><div class="field"><label for="return-reason">差戻理由</label><textarea id="return-reason">確認日の相違について原本で確認してください</textarea></div>', 'return', event.currentTarget));
  $('hold-action').addEventListener('click', () => notify('保留は実装時に現在権限と状態を確認するWorkflow操作です。このデモは送信しません'));
  $('assignment-action').addEventListener('click', event => openDialog('担当変更の確認', '<p>変更先の現在のEligibilityを確認し、旧担当の非公開アクセスを失効させます。</p><p>Delegationや部署所属だけでは権限を推測しません。これは表示プレビューです。</p>', 'assignment', event.currentTarget));
  if (!context) {
    const grouping = document.createElement('label'); grouping.className = 'subtle';
    grouping.innerHTML = '<span>業務種別（WorkType）</span><select id="worktype-filter"><option>内容確認</option><option>再確認</option><option>書類照合</option></select>';
    document.querySelector('.collection-controls').prepend(grouping);
    $('worktype-filter').addEventListener('change', event => { saveItem(); workType = event.target.value; render(); notify('WorkTypeによる同じWorkItem群のQueue projectionです'); });
  }
  $('scenario').innerHTML = data.scenarios.map(s => `<option value="${s.id}">${s.label}</option>`).join('');
  $('scenario').addEventListener('change', event => setScenario(event.target.value));
  $('collection-filter').addEventListener('change', event => { saveItem(); eligibilityOnly = event.target.selectedIndex === 1; render(); notify('選択対象を明示的に切り替えました。未開示情報は読み込みません'); });
  document.addEventListener('click', event => {
    const item = event.target.closest('[data-item]');
    if (item) {
      if (selectedId !== item.dataset.item) {
        saveItem(); loadItem(item.dataset.item);
      }
      render();
      document.querySelector(`[data-item="${selectedId}"]`)?.focus();
      notify('stable IDで対象を切り替えました。前のTaskのFinding・Agent結果を持ち越しません');
    }
    const moduleButton = event.target.closest('[data-module]');
    if (moduleButton) openModule(moduleButton.dataset.module);
    const decisionButton = event.target.closest('[data-decision]');
    if (decisionButton) {
      const kind = decisionButton.dataset.decision;
      const body = kind === '修正' ? '<p>元のFindingを残して採用する主張を別に記録します。</p><div class="field"><label for="adopted-claim">採用する内容</label><textarea id="adopted-claim">原本で確認日を照合してから、次工程へ提出する</textarea></div>' : `<p>Finding「追加確認が必要な可能性があります」を${kind}する表示例です。</p><p>根拠と元の候補は変更しません。提出や承認は別操作です。</p>`;
      openDialog(`人間の判断：${kind}`, body, `decision:${kind}`, decisionButton);
    }
    const actionButton = event.target.closest('[data-action]');
    if (actionButton) {
      const action = actionButton.dataset.action;
      if (action === 'show-resources') openModule('resources');
      else if (action === 'show-history') openModule('history');
      else if (action === 'open-original') { openModule('document'); notify('原本は現在のprovider権限で開きます。このデモに原本バイトはありません'); }
      else if (action === 'compare') { comparisonOpen = true; render(); }
      else if (action === 'close-compare') { comparisonOpen = false; render(); (document.querySelector('[data-action="compare"]') ?? $('work-surface')).focus(); }
      else if (action === 'agent-result') { if (agentCompleted) openModule('evidence'); else { agentCompleted = true; renderModule(); notify('構造化Resultのプレビューです。実Agentは実行していません'); } }
      else if (action === 'agent-request') { agentCompleted = false; renderModule(); notify('現在の認可済み文脈で依頼する表示例です。外部モデルへ送信しません'); }
      else if (action === 'agent-cancel') notify('停止後も不明な結果はoutcome_unknownとして扱います。業務完了にはしません');
      else if (action === 'register-evidence') openModule('evidence');
      else if (action === 'create-workspace') openDialog('新しいWorkspace', '<div class="field"><label for="workspace-name">名前</label><input id="workspace-name" value="新しい作業環境"></div><div class="field"><label>アクセス可能なローカルフォルダ（任意）</label><button type="button" disabled>＋ フォルダを追加（Browserでは未対応）</button></div><p class="subtle">未指定なら対応Runtimeがmanaged rootを作成。Workspace名とFolder名/pathは連動しません。このデモでは実行しません。</p>', 'workspace', actionButton);
      else if (action === 'promote') notify('明示選択したファイルをWorkの非公開storageへ転送してから提出します。ローカルパスを引き継ぎません');
      else if (action === 'retry-provider') notify('現在認可の再確認中を表す操作です。実providerに問い合わせていません');
      else notify('検索のPartial/Unavailable状態例です。Search WIPは接続していません');
    }
    const navigation = event.target.closest('[data-primary]');
    if (navigation?.dataset.primary === 'documents') { event.preventDefault(); openModule('document'); notify('既存Document機能への導線です。実装では選択・戻り位置を保持してDocument routeへ移動します'); }
    if (navigation?.dataset.primary === 'search') { event.preventDefault(); openModule('search'); notify('Search Platform統合契約の表示例です。検索システムの稼働証明ではありません'); }
  });
  render();
})();

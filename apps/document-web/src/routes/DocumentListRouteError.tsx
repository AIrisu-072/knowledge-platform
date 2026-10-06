import { Link, useRouterState } from '@tanstack/react-router';
import { metadataFilterFields } from '../application/document-metadata-filters';
import { validateListSearch } from '../application/search-state';

const reasons = new Set(metadataFilterFields.flatMap(({ label }) => [
  `${label}は文字列で指定してください。URLの条件を確認してください。`,
  `${label}に不正なUnicode文字が含まれています。入力を確認してください。`,
]));

export function DocumentListRouteError({ error }: { error: unknown }) {
  const search = useRouterState({ select: state => state.location.search });
  const reason = error instanceof Error && reasons.has(error.message)
    ? error.message : '一覧の条件を確認できません。条件を解除して再度お試しください。';
  // Clear only the invalid attribute conditions/cursor. Router navigation keeps unresolved operations alive.
  const returnSearch = validateListSearch({ ...search, documentType: undefined, owningDepartment: undefined, category: undefined, cursor: undefined });
  return (
    <section className="notice notice-error" role="alert">
      <h1>一覧の条件を確認してください</h1>
      <p>{reason}</p>
      <Link to="/documents" search={returnSearch}>条件を解除して一覧へ戻る</Link>
    </section>
  );
}

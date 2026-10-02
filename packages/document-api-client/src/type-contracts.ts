import type {
  CommandsRevisionComparisonRequest,
  ModelsDiffDisplayProjection,
  ModelsDisplayFragment,
  ModelsGuiVersionSummary,
  ModelsRevisionComparisonResponse,
} from './generated/types.gen';

type Equal<Actual, Expected> =
  (<Value>() => Value extends Actual ? 1 : 2) extends
  (<Value>() => Value extends Expected ? 1 : 2)
    ? true
    : false;
type Assert<Value extends true> = Value;

type RevisionProjectionIsComplete = Assert<Equal<
  CommandsRevisionComparisonRequest['projection'],
  'diff' | 'comparisonTable' | 'display'
>>;
type DisplayFragmentUnionIsComplete = Assert<Equal<
  ModelsDisplayFragment['kind'],
  'text' | 'table' | 'structural' | 'unavailable'
>>;
type DisplayVerdictIsComplete = Assert<Equal<
  ModelsDiffDisplayProjection['verdict'],
  'same' | 'different' | 'unknown'
>>;
type PageSizeRetainsNullability = Assert<Equal<
  ModelsRevisionComparisonResponse['pageSize'],
  number | null
>>;
type BaseVersionRetainsNullability = Assert<Equal<
  ModelsGuiVersionSummary['baseVersionId'],
  string | null
>>;

export type GeneratedContractAssertions =
  | RevisionProjectionIsComplete
  | DisplayFragmentUnionIsComplete
  | DisplayVerdictIsComplete
  | PageSizeRetainsNullability
  | BaseVersionRetainsNullability;

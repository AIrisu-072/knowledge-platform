import type {
  CommandsRevisionComparisonRequest,
  GetVersionEditManifestData,
  ModelsEditManifest,
  ModelsVersionMutationResult,
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

type EditPurposeExcludesHistory = Assert<Equal<
  GetVersionEditManifestData['query']['purpose'],
  'published' | 'authoring'
>>;
type ManifestRoleIsComplete = Assert<Equal<
  ModelsEditManifest['items'][number]['representations'][number]['role'],
  'authoritative' | 'rendition'
>>;
type ManifestFileIdIsRequired = Assert<Equal<
  ModelsEditManifest['items'][number]['representations'][number]['fileId'],
  string
>>;
type ManifestOriginalFilenameIsRequired = Assert<Equal<
  ModelsEditManifest['items'][number]['representations'][number]['originalFilename'],
  string
>>;
type InitialMutationBaseRetainsNull = Assert<Equal<
  ModelsVersionMutationResult['baseVersionId'],
  string | null
>>;

export type GeneratedContractAssertions =
  | RevisionProjectionIsComplete
  | DisplayFragmentUnionIsComplete
  | DisplayVerdictIsComplete
  | PageSizeRetainsNullability
  | BaseVersionRetainsNullability
  | EditPurposeExcludesHistory
  | ManifestRoleIsComplete
  | ManifestFileIdIsRequired
  | ManifestOriginalFilenameIsRequired
  | InitialMutationBaseRetainsNull;

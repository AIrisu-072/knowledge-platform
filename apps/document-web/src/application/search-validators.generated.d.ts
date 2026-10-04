import type { ValidateFunction } from 'ajv';
import type { ListSearch, DetailSearch } from './search-state';
export declare const validateList: ValidateFunction<ListSearch>;
export declare const validateDetail: ValidateFunction<DetailSearch>;

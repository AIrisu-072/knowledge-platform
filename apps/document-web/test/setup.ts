import '@testing-library/jest-dom';
import { cleanup } from '@testing-library/react';

Object.defineProperty(window, 'scrollTo', { configurable: true, value: jest.fn() });

afterEach(() => cleanup());

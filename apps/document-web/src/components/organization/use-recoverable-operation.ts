import { useEffect, useRef, useState } from 'react';
import { isOperationNotFound, isUnknownOutcome } from '../../application/work-workspace';

/** One fixed operation ID and payload per user intent. An unknown outcome is
 * recovered with the same ID before any explicit same-payload resend. */
export type PendingOperation<R> = { operationId: string; send: () => Promise<R>; recover: () => Promise<R> };
export function useRecoverableOperation<R>(onConfirmed: (result: R) => void) {
  const [operation, setOperation] = useState<PendingOperation<R> | null>(null);
  const [pending, setPending] = useState(false);
  const [unknown, setUnknown] = useState(false);
  const [error, setError] = useState<unknown>(null);
  const mounted = useRef(true);
  useEffect(() => { mounted.current = true; return () => { mounted.current = false; }; }, []);
  async function run(next: PendingOperation<R>, mode: 'send' | 'recover') {
    setOperation(next); setPending(true); setError(null);
    try {
      const result = await (mode === 'send' ? next.send() : next.recover());
      if (!mounted.current) return;
      setPending(false); setUnknown(false); setOperation(null);
      onConfirmed(result);
    } catch (failure) {
      if (!mounted.current) return;
      setPending(false); setError(failure);
      setUnknown(mode === 'recover' || isUnknownOutcome(failure));
    }
  }
  return {
    operation, pending, unknown, error,
    recoverable: unknown && Boolean(operation),
    canResend: unknown && isOperationNotFound(error),
    start: (next: PendingOperation<R>) => run(next, 'send'),
    recover: () => operation && run(operation, 'recover'),
    resend: () => operation && run(operation, 'send'),
    reset: () => { setOperation(null); setUnknown(false); setError(null); },
  };
}

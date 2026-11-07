// A single bounded screening callback can reject candidates without marking them factually verified.
import { z } from 'zod';
import type { StoredResearch } from './research-store';
import type { LearningPolicy } from './learning-types';
import { identity } from './values';

export function policyIdentity(policy?: LearningPolicy): string {
  if (policy !== undefined && typeof policy.screen !== 'function')
    throw new Error('Invalid learning policy');
  if (policy === undefined) return 'source-agreement-v1';
  const id = identity.parse(policy.id);
  if (id === 'source-agreement-v1')
    throw new Error('Custom policies require their own versioned identifier');
  return id;
}

export async function selectFacts(
  research: StoredResearch,
  policy?: LearningPolicy,
  signal?: AbortSignal,
): Promise<number[]> {
  signal?.throwIfAborted();
  const facts = research.report.facts
    .map((fact, index) => ({ fact, index }))
    .filter(
      ({ fact }) => fact.status === 'Verified' && fact.sources.length >= 3 && !fact.contradiction,
    );
  if (!policy) return facts.map(({ index }) => index);
  const controller = new AbortController();
  const combined = signal ? AbortSignal.any([signal, controller.signal]) : controller.signal;
  let timer: NodeJS.Timeout | undefined;
  let onAbort: (() => void) | undefined;
  try {
    const interrupted = new Promise<never>((_, reject) => {
      onAbort = () => reject(new Error('Learning policy was cancelled or exceeded 30 seconds'));
      combined.addEventListener('abort', onAbort, { once: true });
      if (combined.aborted) onAbort();
      timer = setTimeout(() => controller.abort(), 30000);
    });
    const input = structuredClone({
      requestId: research.requestId,
      facts,
      pages: research.report.pages,
    });
    const output = await Promise.race([
      Promise.resolve().then(() => policy.screen(input, combined)),
      interrupted,
    ]);
    combined.throwIfAborted();
    const selected = z.array(z.int().min(0).max(299)).max(300).parse(output);
    const eligible = new Set(facts.map(({ index }) => index));
    if (
      new Set(selected).size !== selected.length ||
      selected.some((index) => !eligible.has(index))
    )
      throw new Error('Learning policy selected an ineligible or repeated fact');
    return selected.sort((left, right) => left - right);
  } finally {
    clearTimeout(timer);
    if (onAbort) combined.removeEventListener('abort', onAbort);
    controller.abort();
  }
}

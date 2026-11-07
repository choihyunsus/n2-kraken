// Host policy may narrow eligible source-agreement candidates before the atomic knowledge write.
import type { CrawledPage, SourceFact } from './report';

export interface LearningPolicy {
  id: string;
  screen(
    input: {
      requestId: string;
      facts: { index: number; fact: SourceFact }[];
      pages: CrawledPage[];
    },
    signal: AbortSignal,
  ): Promise<readonly number[]> | readonly number[];
}
export interface LearningReceipt {
  requestId: string;
  policyId: string;
  reportHash: string;
  accepted: number[];
  sources: { url: string; id: string }[];
  insights: { index: number; id: string }[];
  learnedAt: number;
}
export interface LearnResult extends LearningReceipt {
  status: 'learned' | 'already_learned';
  verified: false;
  newSources: number;
  newInsights: number;
}

// Standalone library entry point with explicit shared-database scope and source evidence types.
export { Kraken } from './kraken';
export type { KrakenOptions } from './kraken';
export type {
  ExtractInput,
  FetchInput,
  RenderInput,
  SearchInput,
  ResearchInput,
  StudyConfig,
} from './input';
export type {
  ExtractedDocument,
  RenderedDocument,
  SearchResult,
  CrawledPage,
  SourceFact,
  ResearchReport,
} from './report';
export type { StoredResearch } from './research-store';
export type { LearningPolicy, LearningReceipt, LearnResult } from './learning-types';

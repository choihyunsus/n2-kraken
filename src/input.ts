// Caller options are resolved before execution; trusted service endpoints are host configuration.
import { z } from 'zod';
import { identity, MIB, query, utf8, webUrl } from './values';

export const htmlConfig = z.strictObject({
  include_links: z.boolean().default(true),
  include_images: z.boolean().default(false),
  max_length: z.int().min(0).max(MIB).default(50000),
});
export const studyConfig = z
  .strictObject({
    max_results: z.int().min(1).max(40).default(10),
    max_crawl_pages: z.int().min(1).max(20).default(5),
    min_confidence: z.number().min(0.5).max(1).default(0.6),
    max_content_length: z.int().min(50).max(100000).default(5000),
    timeout_ms: z.int().min(100).max(120000).default(30000),
    total_timeout_ms: z.int().min(100).max(600000).default(180000),
    time_range: z.enum(['', '1w', '1m', '3m', '6m', '1y']).default('3m'),
    engines: z
      .array(z.enum(['ddg', 'bing', 'naver', 'google', 'searxng']))
      .min(1)
      .max(5)
      .refine((value) => new Set(value).size === value.length)
      .default(['ddg', 'bing', 'naver']),
  })
  .refine(
    (value) => value.max_crawl_pages <= value.max_results,
    'Crawl page limit cannot exceed the search result limit',
  );

export const extractInput = z.strictObject({
  html: utf8(8 * MIB),
  source_url: webUrl.optional(),
  config: htmlConfig.prefault({}),
});
export const fetchInput = z.strictObject({ url: webUrl, config: htmlConfig.prefault({}) });
export const renderInput = z.strictObject({
  url: webUrl,
  format: z.enum(['text', 'html']).default('text'),
  max_length: z.int().min(0).max(MIB).default(50000),
});
export const searchInput = z.strictObject({ query, config: studyConfig.prefault({}) });
export const researchInput = z.strictObject({
  requestId: identity,
  topic: query,
  config: studyConfig.prefault({}),
  semantic: z.boolean().default(false),
});
export type ExtractInput = z.input<typeof extractInput>;
export type FetchInput = z.input<typeof fetchInput>;
export type RenderInput = z.input<typeof renderInput>;
export type SearchInput = z.input<typeof searchInput>;
export type ResearchInput = z.input<typeof researchInput>;
export type StudyConfig = z.output<typeof studyConfig>;

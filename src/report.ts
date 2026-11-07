// Native output must preserve exact content hashes, source membership and summary accounting.
import { z } from 'zod';
import { digest, hash, MIB, query, utf8, webUrl } from './values';

export const extractedDocument = z.strictObject({
  title: utf8(8192),
  source_url: z.union([z.literal(''), webUrl]),
  markdown: utf8(MIB),
  truncated: z.boolean(),
  iframe_srcs: z.array(utf8(8192)).max(32),
});
export const renderedDocument = z.strictObject({
  url: webUrl,
  title: utf8(8192),
  content: utf8(MIB),
  iframe_count: z.int().min(0).max(3),
  format: z.enum(['text', 'html']),
  truncated: z.boolean(),
});
export const searchResults = z
  .array(
    z.strictObject({
      title: utf8(1024),
      url: webUrl,
      snippet: utf8(4096),
    }),
  )
  .max(40)
  .refine((rows) => new Set(rows.map((row) => row.url)).size === rows.length);

export const crawledPage = z
  .strictObject({
    url: webUrl,
    title: utf8(8192),
    content: utf8(100000),
    content_length: z.int().min(0).max(100000),
    content_hash: hash,
    fetched_at: z.string().regex(/^\d{1,12}$/),
    truncated: z.boolean(),
  })
  .refine(
    (page) =>
      page.content_length === Buffer.byteLength(page.content) &&
      page.content_hash === digest(page.content),
    'Source hash or byte length does not match its content',
  );
export const fact = z
  .strictObject({
    claim: utf8(1500).min(1),
    confidence: z.number().min(0).max(1),
    sources: z
      .array(webUrl)
      .min(1)
      .max(20)
      .refine((values) => new Set(values).size === values.length),
    status: z.enum(['Verified', 'Pending', 'Unverified', 'Flagged']),
    contradiction: utf8(8192).nullable(),
  })
  .refine(
    (value) =>
      value.status !== 'Verified' ||
      (value.sources.length >= 3 && value.confidence >= 0.5 && value.contradiction === null),
    'Source agreement requires three distinct source URLs without a flagged contradiction',
  );
const count = z.int().min(0).max(300);
export const researchReport = z
  .strictObject({
    summary: z.strictObject({
      topic: query,
      semantic_mode: z.boolean(),
      search_results: z.int().min(0).max(40),
      pages_crawled: z.int().min(0).max(20),
      claims_extracted: count,
      facts_verified: count,
      facts_pending: count,
      facts_rejected: count,
    }),
    pages: z.array(crawledPage).max(20),
    facts: z.array(fact).max(300),
    failures: z.array(z.strictObject({ url: webUrl, stage: utf8(64), reason: utf8(8192) })).max(80),
  })
  .superRefine((report, context) => {
    const sources = new Set(report.pages.map((page) => page.url));
    const summary = report.summary;
    const verified = report.facts.filter((value) => value.status === 'Verified').length;
    const pending = report.facts.filter((value) => value.status === 'Pending').length;
    const uniqueFacts = new Set(
      report.facts.map((value) => JSON.stringify([value.claim, [...value.sources].sort()])),
    );
    if (
      sources.size !== report.pages.length ||
      summary.pages_crawled !== sources.size ||
      summary.search_results < sources.size ||
      summary.facts_verified !== verified ||
      summary.facts_pending !== pending ||
      summary.facts_rejected !== report.facts.length - verified - pending ||
      uniqueFacts.size !== report.facts.length ||
      report.facts.length > summary.claims_extracted ||
      report.facts.some(
        (value) =>
          value.sources.some((url) => !sources.has(url)) ||
          !report.pages.some(
            (page) => value.sources.includes(page.url) && page.content.includes(value.claim),
          ),
      )
    )
      context.addIssue({
        code: 'custom',
        message: 'Research source references or summary counts are inconsistent',
      });
  });
export type ExtractedDocument = z.output<typeof extractedDocument>;
export type RenderedDocument = z.output<typeof renderedDocument>;
export type SearchResult = z.output<typeof searchResults>[number];
export type CrawledPage = z.output<typeof crawledPage>;
export type SourceFact = z.output<typeof fact>;
export type ResearchReport = z.output<typeof researchReport>;

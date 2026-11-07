// MCP responses expose bounded previews while full provenance stays in the scoped research receipt.
import type { CallToolResult } from '@modelcontextprotocol/sdk/types.js';
import type { StoredResearch } from './research-store';
import type { ExtractedDocument, RenderedDocument } from './report';
import type { LearnResult } from './learning-types';
import { encode } from './values';
const METADATA_CHARACTERS = 256;

export async function result(operation: () => unknown | Promise<unknown>): Promise<CallToolResult> {
  try {
    return { content: [{ type: 'text', text: encode(await operation(), 60000) }] };
  } catch (error) {
    const message = error instanceof Error ? error.message : String(error);
    return {
      isError: true,
      content: [{ type: 'text', text: Array.from(message).slice(0, 2000).join('') }],
    };
  }
}

export function researchSummary(value: StoredResearch | undefined) {
  if (!value) throw new Error('Research was not found in this project and agent scope');
  return {
    requestId: value.requestId,
    reportHash: value.reportHash,
    workerHash: value.workerHash,
    createdAt: value.createdAt,
    summary: value.report.summary,
    sourceCount: value.report.pages.length,
    factCount: value.report.facts.length,
    failureCount: value.report.failures.length,
    failures: value.report.failures.slice(0, 5).map((failure) => ({
      url: failure.url,
      stage: failure.stage,
      reason: Array.from(failure.reason).slice(0, METADATA_CHARACTERS).join(''),
    })),
    verified: false,
  };
}

export function documentPreview(value: ExtractedDocument | RenderedDocument) {
  const text = 'markdown' in value ? value.markdown : value.content;
  const content = Array.from(text).slice(0, 4000).join('');
  return {
    title: Array.from(value.title).slice(0, METADATA_CHARACTERS).join(''),
    url: 'source_url' in value ? value.source_url : value.url,
    content,
    truncated:
      value.truncated || content !== text || Array.from(value.title).length > METADATA_CHARACTERS,
  };
}

export function sourcePreview(
  value: StoredResearch | undefined,
  index: number,
  offset: number,
  limit: number,
) {
  researchSummary(value);
  const page = value!.report.pages[index];
  if (!page) throw new Error('Source index is outside this research report');
  const characters = Array.from(page.content),
    content = characters.slice(offset, offset + limit).join('');
  return {
    index,
    url: page.url,
    title: Array.from(page.title).slice(0, METADATA_CHARACTERS).join(''),
    contentHash: page.content_hash,
    fetchedAt: page.fetched_at,
    sourceTruncated: page.truncated,
    offset,
    totalCharacters: characters.length,
    nextOffset: offset + limit < characters.length ? offset + limit : null,
    content,
  };
}

export function factPreview(value: StoredResearch | undefined, offset: number, limit: number) {
  researchSummary(value);
  const report = value!.report;
  return {
    total: report.facts.length,
    offset,
    nextOffset: offset + limit < report.facts.length ? offset + limit : null,
    facts: report.facts.slice(offset, offset + limit).map((fact, position) => ({
      index: offset + position,
      claim: fact.claim,
      agreementScore: fact.confidence,
      agreementStatus: fact.status,
      verified: false,
      sourceIndices: fact.sources.map((url) => report.pages.findIndex((page) => page.url === url)),
      contradiction:
        fact.contradiction && Array.from(fact.contradiction).slice(0, METADATA_CHARACTERS).join(''),
    })),
  };
}

export function learningSummary(value: LearnResult) {
  return {
    requestId: value.requestId,
    policyId: value.policyId,
    reportHash: value.reportHash,
    status: value.status,
    acceptedFacts: value.accepted.length,
    sourceCount: value.sources.length,
    insightCount: value.insights.length,
    newSources: value.newSources,
    newInsights: value.newInsights,
    verified: false,
  };
}

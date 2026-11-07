// Validate UTF-8 identities and bounded JSON before crossing storage or process boundaries.
import { createHash } from 'node:crypto';
import { z } from 'zod';

export const MIB = 1_048_576;
export function utf8(maximum: number) {
  return z
    .string()
    .max(maximum)
    .refine(
      (value) =>
        Buffer.byteLength(value) <= maximum && Buffer.from(value).toString('utf8') === value,
      'Expected bounded, well-formed UTF-8 text',
    );
}
export const identity = utf8(128)
  .min(1)
  .regex(/^[^\s\x00-\x1f\x7f]+$/u);
export const query = utf8(4096).refine(
  (value) => !!value.trim() && !/[\x00-\x1f\x7f]/u.test(value),
);
export const hash = z.string().regex(/^[a-f0-9]{64}$/);
export const reference = z.string().uuid();
export const webUrl = utf8(8192).refine((value) => {
  try {
    const url = new URL(value);
    return (
      /^https?:$/.test(url.protocol) &&
      !!url.hostname &&
      !url.username &&
      !url.password &&
      url.port !== '0' &&
      !/[\x00-\x20\x7f]/u.test(value)
    );
  } catch {
    return false;
  }
}, 'Expected an HTTP(S) URL without credentials or control characters');

export function digest(value: string | Buffer): string {
  return createHash('sha256').update(value).digest('hex');
}
export function encode(value: unknown, maximum: number): string {
  const json = JSON.stringify(value);
  if (typeof json !== 'string' || Buffer.byteLength(json) > maximum)
    throw new Error('JSON exceeds its supported byte limit');
  return json;
}
export function parseJson(value: unknown, maximum: number): unknown {
  return JSON.parse(utf8(maximum).parse(value));
}

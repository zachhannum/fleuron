/**
 * One place the worker is opened from, so one worker is bundled.
 *
 * A bundler keys a module worker on the `new URL` that names it, and
 * two callers writing that URL from two directories get two copies
 * of the same worker.
 */
export function spawn(): Worker {
  return new Worker(new URL('./worker.ts', import.meta.url), { type: 'module' });
}

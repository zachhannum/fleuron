/**
 * A playground in a URL.
 *
 * What the reader changed goes in the fragment, and what they did
 * not is left to the seed the page was built with. So a link to a
 * one-line stylesheet tweak is short, and a bug report against the
 * CSS subset is a link rather than a paragraph.
 *
 * The fragment never reaches a server, which is the other reason it
 * is the right place for someone else's manuscript.
 */

/** What is in a link. */
export interface Shared {
  markdown?: string;
  css?: string;
  page?: number;
  stylesheet?: boolean;
}

/** Reads a shared playground out of the current URL. */
export function read(): Shared | null {
  const hash = globalThis.location?.hash ?? '';
  const at = hash.indexOf('=');
  if (!hash.startsWith('#try=') || at === -1) {
    return null;
  }
  try {
    return JSON.parse(decode(hash.slice(at + 1))) as Shared;
  } catch {
    return null;
  }
}

/**
 * Writes one into it, without adding a history entry: paging through
 * a demo should not fill the back button.
 */
export function write(shared: Shared): void {
  const empty = Object.values(shared).every((value) => value === undefined);
  const url = new URL(globalThis.location.href);
  url.hash = empty ? '' : `try=${encode(JSON.stringify(shared))}`;
  history.replaceState(null, '', url);
}

function encode(text: string): string {
  const bytes = new TextEncoder().encode(text);
  let binary = '';
  for (const byte of bytes) {
    binary += String.fromCharCode(byte);
  }
  return btoa(binary).replace(/\+/g, '-').replace(/\//g, '_').replace(/=+$/, '');
}

function decode(text: string): string {
  const binary = atob(text.replace(/-/g, '+').replace(/_/g, '/'));
  const bytes = Uint8Array.from(binary, (character) => character.charCodeAt(0));
  return new TextDecoder().decode(bytes);
}

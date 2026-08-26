import { relative, resolve, dirname } from 'node:path';

/**
 * Rewrites relative links between prose files to site paths.
 *
 * Prose lives in `docs/` and is read both there and here, so links are
 * written the way they have to be to work on GitHub — `../cli/reference.md`
 * — and turned into `/<base>/cli/reference/` on the way in.
 */
export function remarkDocLinks({ root, base }) {
  const prefix = base.replace(/\/$/, '');
  return (tree, file) => {
    const from = file.history[0];
    if (!from) return;
    visit(tree, (node) => {
      if (node.type !== 'link' || typeof node.url !== 'string') return;
      const [target, hash] = splitHash(node.url);
      if (!target.endsWith('.md') || /^[a-z]+:/i.test(target)) return;
      const abs = resolve(dirname(from), target);
      const slug = relative(root, abs)
        .replace(/\\/g, '/')
        .replace(/\.md$/, '')
        .replace(/(^|\/)index$/, '');
      node.url = `${prefix}/${slug}${slug ? '/' : ''}${hash}`;
    });
  };
}

function splitHash(url) {
  const at = url.indexOf('#');
  return at === -1 ? [url, ''] : [url.slice(0, at), url.slice(at)];
}

function visit(node, fn) {
  fn(node);
  for (const child of node.children ?? []) visit(child, fn);
}

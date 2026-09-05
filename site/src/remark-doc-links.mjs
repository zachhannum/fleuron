import { realpathSync } from 'node:fs';
import { relative, resolve, dirname } from 'node:path';

/**
 * Rewrites relative links between prose files to site paths.
 *
 * Prose lives in `docs/` and is read both there and here, so links are
 * written the way they have to be to work on GitHub — `../cli/reference.md`
 * — and turned into `/<base>/cli/reference/` on the way in. A page
 * with a demo on it is `.mdx`, and is linked to by that name.
 *
 * Both ends are resolved through the symlink `docs/` is mounted at:
 * markdown arrives at the path inside `src/`, MDX at the real one,
 * and a slug taken against the wrong one is a link out of the site.
 */
export function remarkDocLinks({ root, base }) {
  const prefix = base.replace(/\/$/, '');
  const real = realpathSync(root);
  return (tree, file) => {
    const from = file.history[0];
    if (!from) return;
    visit(tree, (node) => {
      if (node.type !== 'link' || typeof node.url !== 'string') return;
      const [target, hash] = splitHash(node.url);
      if (!/\.mdx?$/.test(target) || /^[a-z]+:/i.test(target)) return;
      const abs = resolve(dirname(realpathSync(from)), target);
      const slug = relative(real, abs)
        .replace(/\\/g, '/')
        .replace(/\.mdx?$/, '')
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

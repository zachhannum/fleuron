// @ts-check
import { defineConfig } from 'astro/config';
import starlight from '@astrojs/starlight';
import mdx from '@astrojs/mdx';
import react from '@astrojs/react';
import { parse } from 'acorn';
import tailwindcss from '@tailwindcss/vite';
import starlightLinksValidator from 'starlight-links-validator';
import { unified } from '@astrojs/markdown-remark';
import { fileURLToPath } from 'node:url';
import { remarkDocLinks } from './src/remark-doc-links.mjs';

const docsRoot = fileURLToPath(new URL('./src/content/docs', import.meta.url));

/**
 * Stops the bindgen glue from carrying a copy of the module.
 *
 * The glue falls back to fetching `fleuron_bg.wasm` from beside
 * itself, and a bundler that sees that line emits the six megabytes
 * as an asset, once per build target, whether or not anything ever
 * asks for it. The worker names the module explicitly, under the
 * base path, so the fallback is unreachable and is made to say so.
 */
function moduleServedFromPublic() {
  return {
    name: 'fleuron-module-served-from-public',
    enforce: 'pre',
    transform(code, id) {
      if (!id.includes('fleuron-wasm/npm/wasm/fleuron.js')) return null;
      return code.replace(
        "module_or_path = new URL('fleuron_bg.wasm', import.meta.url);",
        "throw new Error('the module is fetched by src/demos/worker.ts, not by the glue');",
      );
    },
  };
}

const docLinks = [remarkDocLinks, { root: docsRoot, base: '/' }];

/**
 * Puts the demo components in scope for every prose page.
 *
 * Prose is read on GitHub as well as here, and an import line at the
 * top of a chapter is a line of code in the middle of a sentence
 * there. A page that wants a demo writes the tag and nothing else.
 */
function demoComponents() {
  const source = "import Playground from '~/components/demos/Playground.astro';";
  const estree = parse(source, { ecmaVersion: 'latest', sourceType: 'module' });
  return () => (tree) => {
    tree.children.unshift({ type: 'mdxjsEsm', value: source, data: { estree } });
  };
}

export default defineConfig({
  site: 'https://fleuron.typeworks.dev',
  // A stylesheet in the document is a stylesheet the reader is not
  // waiting a round trip for. The site's sheets are small enough
  // that carrying them costs less than fetching them.
  build: { inlineStylesheets: 'always' },

  trailingSlash: 'always',
  // Starlight turns prefetching on for every page, and a page here
  // ships either nothing or a demo it was asked for.
  prefetch: false,
  markdown: {
    processor: unified({ remarkPlugins: [docLinks] }),
  },
  vite: {
    plugins: [tailwindcss(), moduleServedFromPublic()],
    // Worker bundles are built with their own plugin list, and the
    // glue is only ever imported from inside one.
    worker: { format: 'es', plugins: () => [moduleServedFromPublic()] },
    // The docs collection is a symlink out of src/; dev has to be told
    // it may serve what it points at.
    server: { fs: { allow: ['..'] } },
  },
  integrations: [
    react(),
    starlight({
      title: 'fleuron',
      description:
        'A paged-media layout engine for book-shaped documents, in Rust.',
      customCss: ['./src/styles/tailwind.css', './src/styles/theme.css'],
      // Code keeps one key ground in both themes, the same as the
      // blocks on the landing page: one palette, one contrast check.
      expressiveCode: { themes: ['github-dark-default'] },
      social: [
        {
          icon: 'github',
          label: 'GitHub',
          href: 'https://github.com/zachhannum/fleuron',
        },
      ],
      components: {
        Header: './src/components/Header.astro',
        Sidebar: './src/components/Sidebar.astro',
        Footer: './src/components/Footer.astro',
        PageTitle: './src/components/PageTitle.astro',
      },
      // Nav order is a decision, not a consequence of file paths.
      sidebar: [
        { label: 'Overview', link: '/overview/' },
        { label: 'Install', link: '/install/' },
        {
          label: 'Library',
          items: [
            { label: 'Quickstart', link: '/library/quickstart/' },
            { label: 'Fonts', link: '/library/fonts/' },
            { label: 'Sessions', link: '/library/sessions/' },
            { label: 'Diagnostics', link: '/library/diagnostics/' },
          ],
        },
        {
          label: 'CLI',
          items: [
            { label: 'Quickstart', link: '/cli/quickstart/' },
            { label: 'Reference', link: '/cli/reference/' },
          ],
        },
        {
          label: 'WebAssembly',
          items: [
            { label: 'Quickstart', link: '/wasm/quickstart/' },
            { label: 'The preview', link: '/wasm/preview/' },
            { label: 'The wire', link: '/wasm/wire/' },
          ],
        },
        {
          label: 'Reference',
          items: [
            { label: 'CSS subset', link: '/css-subset/' },
            { label: 'Markdown mapping', link: '/reference/markdown/' },
            { label: 'Content tree', link: '/reference/content-tree/' },
            { label: 'Display structure', link: '/reference/display-structure/' },
            { label: 'API (rustdoc)', link: '/api/fleuron/', attrs: { target: '_blank' } },
          ],
        },
        { label: 'Demos', link: '/demos/' },
      ],
      plugins: [
        starlightLinksValidator({
          // rustdoc is copied in after the Astro build; the validator
          // cannot see files this build did not produce.
          //
          // The demo pages are `.mdx`, which this validator collects
          // no headings from, so every link to one reads as broken.
          // `scripts/check-doc-links.mjs` resolves prose links
          // against the files themselves, which is the check that
          // matters for a reader on GitHub anyway.
          exclude: [
            '/api/**',
            '/css-subset/',
            '/cli/quickstart/',
            '/library/diagnostics/',
            '/reference/display-structure/',
            '/reference/markdown/',
            '/wasm/preview/',
            '/wasm/preview/**',
          ],
        }),
      ],
      editLink: {
        baseUrl: 'https://github.com/zachhannum/fleuron/edit/main/docs/',
      },
      lastUpdated: true,
      pagination: false,
    }),
    // After Starlight: it brings the code-block renderer with it, and
    // that has to be registered before MDX pages are compiled.
    mdx({ remarkPlugins: [docLinks, demoComponents()] }),
  ],
});

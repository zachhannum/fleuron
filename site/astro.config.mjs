// @ts-check
import { defineConfig } from 'astro/config';
import starlight from '@astrojs/starlight';
import react from '@astrojs/react';
import tailwindcss from '@tailwindcss/vite';
import starlightLinksValidator from 'starlight-links-validator';
import { unified } from '@astrojs/markdown-remark';
import { fileURLToPath } from 'node:url';
import { remarkDocLinks } from './src/remark-doc-links.mjs';

const docsRoot = fileURLToPath(new URL('./src/content/docs', import.meta.url));

export default defineConfig({
  site: 'https://zachhannum.github.io',
  base: '/fleuron',
  trailingSlash: 'always',
  // Starlight turns prefetching on for every page, and the landing page
  // is the one page that must ship no script at all.
  prefetch: false,
  markdown: {
    processor: unified({
      remarkPlugins: [[remarkDocLinks, { root: docsRoot, base: '/fleuron' }]],
    }),
  },
  vite: {
    plugins: [tailwindcss()],
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
      customCss: ['./src/styles/theme.css'],
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
            { label: 'Display list', link: '/reference/display-list/' },
            { label: 'API (rustdoc)', link: '/fleuron/api/fleuron/', attrs: { target: '_blank' } },
          ],
        },
        { label: 'Demos', link: '/demos/' },
      ],
      plugins: [
        starlightLinksValidator({
          // rustdoc is copied in after the Astro build; the validator
          // cannot see files this build did not produce.
          exclude: ['/fleuron/api/**'],
        }),
      ],
      editLink: {
        baseUrl: 'https://github.com/zachhannum/fleuron/edit/main/docs/',
      },
      lastUpdated: true,
      pagination: false,
    }),
  ],
});

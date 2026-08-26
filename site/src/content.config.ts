import { defineCollection } from 'astro:content';
import { docsLoader } from '@astrojs/starlight/loaders';
import { docsSchema } from '@astrojs/starlight/schema';

// `src/content/docs` is a symlink to the repository's `docs/`. Prose is
// authored and read there; the site serves those files rather than a
// copy of them, so editing one changes both.
export const collections = {
  docs: defineCollection({ loader: docsLoader(), schema: docsSchema() }),
};

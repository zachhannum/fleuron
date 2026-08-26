/// <reference path="../.astro/types.d.ts" />
/// <reference types="astro/client" />
// Starlight ships these as loose declaration files rather than through
// its exports map, so they are referenced by path.
/// <reference path="../node_modules/@astrojs/starlight/virtual.d.ts" />
/// <reference path="../node_modules/@astrojs/starlight/virtual-internal.d.ts" />
/// <reference path="../node_modules/@astrojs/starlight/locals.d.ts" />

declare module '*.woff2' {
  const url: string;
  export default url;
}

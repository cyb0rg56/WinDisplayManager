// @ts-check
import { defineConfig } from 'astro/config';
import { unified } from '@astrojs/markdown-remark';
import starlight from '@astrojs/starlight';

const base = '/WinDisplayManager';

/** Prefix root-relative Markdown links with the Pages base. Starlight only does this for its own chrome. */
function rehypeBaseLinks() {
  return (tree) => {
    visit(tree);
  };

  function visit(node) {
    if (!node || typeof node !== 'object') return;
    if (node.type === 'element' && node.tagName === 'a') {
      const href = node.properties?.href;
      if (
        typeof href === 'string' &&
        href.startsWith('/') &&
        !href.startsWith('//') &&
        !href.startsWith(base)
      ) {
        node.properties.href = `${base}${href}`;
      }
    }
    if (Array.isArray(node.children)) {
      for (const child of node.children) visit(child);
    }
  }
}

// https://astro.build/config
export default defineConfig({
  site: 'https://cyb0rg56.github.io',
  base,
  trailingSlash: 'always',
  markdown: {
    processor: unified({ rehypePlugins: [rehypeBaseLinks] }),
  },
  integrations: [
    starlight({
      title: 'WinDisplayManager',
      description:
        'DDC/CI monitor control for Windows — brightness, contrast, input switching, power mode, hotkeys, and display profiles.',
      social: [
        {
          icon: 'github',
          label: 'GitHub',
          href: 'https://github.com/cyb0rg56/WinDisplayManager',
        },
      ],
      sidebar: [
        { label: 'Home', slug: 'index' },
        { label: 'Hotkeys', slug: 'docs/hotkeys' },
        { label: 'Profiles', slug: 'docs/profiles' },
        { label: 'Testing', slug: 'docs/testing' },
        { label: 'Privacy', slug: 'privacy' },
      ],
    }),
  ],
});

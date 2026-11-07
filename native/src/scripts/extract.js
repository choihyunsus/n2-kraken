// Live-DOM extraction expression evaluated by CDP in the page or an iframe.
new (class KrakenTextReader {
  noiseTags = new Set([
    'script',
    'style',
    'noscript',
    'svg',
    'path',
    'nav',
    'aside',
    'footer',
    'header',
    'dialog',
    'select',
    'option',
    'datalist',
    'form',
    'iframe',
    'canvas',
    'video',
    'audio',
    'fieldset',
    'template',
    'figure',
    'figcaption',
    'menu',
    'menuitem',
    'button',
    'input',
  ]);
  noiseRoles = new Set([
    'navigation',
    'banner',
    'contentinfo',
    'complementary',
    'dialog',
    'alertdialog',
    'toolbar',
    'menubar',
    'search',
    'status',
    'log',
    'alert',
    'tooltip',
    'presentation',
    'none',
  ]);
  adPattern =
    /\b(ads?[_-]?|sponsor|advert|promoted|cookie|popup|modal|overlay|banner|newsletter|sidebar|promo|gdpr|consent)\b/i;
  metadataLimit = 8192;
  truncated = false;

  cut(value, limit) {
    if (value.length > limit) {
      this.truncated = true;
      const last = value.charCodeAt(limit - 1);
      if (last >= 0xd800 && last <= 0xdbff) limit--;
      value = value.slice(0, limit);
    }
    return value.toWellFormed();
  }

  hidden(element, stop) {
    for (let current = element; current && current !== stop; current = current.parentElement) {
      if (!current.tagName) break;
      if (this.noiseTags.has(current.tagName.toLowerCase())) return true;
      if (current.getAttribute) {
        if (
          this.noiseRoles.has(current.getAttribute('role')) ||
          current.getAttribute('aria-hidden') === 'true'
        )
          return true;
        const classes =
          (typeof current.className === 'string' ? current.className : '') +
          ' ' +
          (current.id || '');
        if (this.adPattern.test(classes)) return true;
      }
      try {
        const style = window.getComputedStyle(current);
        if (style.display === 'none' || style.visibility === 'hidden' || style.opacity === '0')
          return true;
      } catch (_) {
        return true;
      } // Detached or inaccessible nodes cannot be verified as visible.
    }
    return false;
  }

  line(element, text) {
    const tag = element.tagName.toLowerCase();
    if (['h1', 'h2', 'h3', 'h4'].includes(tag))
      return '#'.repeat(parseInt(tag[1], 10)) + ' ' + text;
    if (tag === 'strong' || tag === 'b') return '**' + text + '**';
    if (tag === 'code') return '`' + text + '`';
    if (tag === 'a' && element.href) {
      try {
        const href = new URL(element.href);
        if (
          ['http:', 'https:'].includes(href.protocol) &&
          !href.username &&
          !href.password &&
          href.href.length <= 2048
        )
          return '[' + text + '](' + href.href + ')';
      } catch (_) {
        /* Preserve visible text when the target is invalid. */
      }
    }
    if (tag === 'li') return '- ' + text;
    if (tag === 'blockquote') return '> ' + text;
    return text;
  }

  extract() {
    const lines = [],
      root = document.querySelector('main, article, [role="main"]') || document.body;
    if (root) {
      const walker = document.createTreeWalker(root, NodeFilter.SHOW_TEXT, null);
      let node,
        visited = 0;
      while ((node = walker.nextNode())) {
        if (lines.length >= 800 || ++visited > 100000) {
          this.truncated = true;
          break;
        }
        const text = node.textContent ? this.cut(node.textContent, 4000).trim() : '';
        if (
          text.length < 2 ||
          !node.parentElement ||
          this.hidden(node.parentElement, root.parentElement)
        )
          continue;
        lines.push(this.line(node.parentElement, text));
      }
    }
    if (location.href.length > this.metadataLimit) throw Error('Source URL exceeds limit');
    const title = this.cut(document.title || '', this.metadataLimit);
    const content = this.cut(lines.join('\n'), 2000000);
    return JSON.stringify({ title, url: location.href, content, truncated: this.truncated });
  }
})().extract();

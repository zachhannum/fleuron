/**
 * The fleuron preview, as a React component.
 *
 * There is no engine logic in here and there is not meant to be:
 * everything this file does is hand `fleuron`'s
 * `Preview` an element and pass props along to it. React stays out
 * of the binding package for the same reason, that a host which does
 * not use it should not download it, and a host that deletes this one
 * still has a preview a plain page can mount.
 */

import { Preview as Mounted, type PreviewOptions } from 'fleuron';
import { useEffect, useRef, useState, type CSSProperties } from 'react';

/** What the component takes, over what a preview takes. */
export interface PreviewProps extends PreviewOptions {
  /** The manuscript, as markdown. */
  markdown?: string;
  /** The author stylesheet, as CSS text. */
  css?: string;
  /** What the manuscript is called, which is what an edit replaces. */
  name?: string;
  /** The page to show, counting from 1. */
  page?: number;
  /** Points to CSS pixels. */
  zoom?: number;
  /**
   * The images the manuscript refers to, by the url it names them
   * by. One that arrives after the book does is registered when it
   * arrives, and the pages are laid out again around it.
   */
  images?: Record<string, Uint8Array>;
  /** Passed to the element the preview mounts into. */
  className?: string;
  /** The same. */
  style?: CSSProperties;
  /** The preview itself, once it is mounted. */
  onMount?: (preview: Mounted) => void;
}

/**
 * A book on screen. The manuscript and the stylesheet are props, so
 * a keystroke is a re-render and the engine is handed the one input
 * that changed.
 */
export function Preview(props: PreviewProps): React.ReactElement {
  const { markdown, css, name, page, zoom, images, className, style, onMount, ...options } = props;
  const element = useRef<HTMLDivElement>(null);
  const [preview, setPreview] = useState<Mounted | null>(null);

  // Mounted once. The options a preview is opened with are fixed for
  // its life; everything that can change afterwards is a prop below.
  useEffect(() => {
    let wanted = true;
    let opened: Mounted | null = null;
    void Mounted.mount(element.current as Element, options).then((mounted) => {
      opened = mounted;
      if (!wanted) {
        mounted.destroy();
        return;
      }
      setPreview(mounted);
      onMount?.(mounted);
    });
    return () => {
      wanted = false;
      opened?.destroy();
    };
  }, []);

  useEffect(() => {
    if (preview !== null && css !== undefined) {
      void preview.setStyle(css);
    }
  }, [preview, css]);

  useEffect(() => {
    if (preview !== null && markdown !== undefined) {
      void preview.setMarkdown(markdown, name);
    }
  }, [preview, markdown, name]);

  useEffect(() => {
    if (preview !== null && page !== undefined) {
      preview.page = page;
    }
  }, [preview, page]);

  useEffect(() => {
    if (preview !== null && zoom !== undefined) {
      preview.zoom = zoom;
    }
  }, [preview, zoom]);

  // Images are registered rather than passed at mount, so one that
  // arrives after the manuscript still reaches the page. A url the
  // session already has costs no layout.
  useEffect(() => {
    if (preview !== null && images !== undefined) {
      for (const [url, bytes] of Object.entries(images)) {
        void preview.addImage(url, bytes);
      }
    }
  }, [preview, images]);

  return <div ref={element} className={className} style={style} />;
}

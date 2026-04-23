import { useCallback, useLayoutEffect, useState } from "react";

/**
 * Measures the content width of a DOM element via ResizeObserver.
 *
 * Returns a stable ref callback and the current width (0 until mounted).
 * Uses useLayoutEffect to read the initial size synchronously before the
 * browser paints, preventing the flash where a sidebar briefly renders wide
 * before collapsing.
 *
 * Usage:
 *   const [containerRef, containerWidth] = useContainerWidth();
 *   const collapsed = containerWidth > 0 && containerWidth < 720;
 *   return <div ref={containerRef}>...</div>;
 */
export function useContainerWidth(): [
  ref: (el: HTMLDivElement | null) => void,
  width: number,
] {
  const [el, setEl] = useState<HTMLDivElement | null>(null);
  const [width, setWidth] = useState(0);

  const ref = useCallback((node: HTMLDivElement | null) => {
    setEl(node);
  }, []);

  useLayoutEffect(() => {
    if (!el) {
      setWidth(0);
      return;
    }

    // Read initial width synchronously before the browser paints —
    // this prevents the "flash" where the sidebar briefly shows at full
    // width before collapsing on narrow containers.
    setWidth(el.getBoundingClientRect().width);

    const ro = new ResizeObserver((entries) => {
      setWidth(entries[0]?.contentRect.width ?? 0);
    });
    ro.observe(el);

    return () => ro.disconnect();
  }, [el]);

  return [ref, width];
}

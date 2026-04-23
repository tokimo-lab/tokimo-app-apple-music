import { useCallback, useState } from "react";

export function useSidebarCollapsed(
  componentId: string,
  autoCollapsed: boolean,
) {
  const storageKey = `sidebar-collapsed-${componentId}`;
  const [manuallyCollapsed, setManuallyCollapsed] = useState<boolean>(() => {
    try {
      return localStorage.getItem(storageKey) === "true";
    } catch {
      return false;
    }
  });
  const collapsed = autoCollapsed || manuallyCollapsed;

  const onToggleCollapse = useCallback(() => {
    const next = !collapsed;
    setManuallyCollapsed(next);
    try {
      localStorage.setItem(storageKey, String(next));
    } catch {
      /* ignore */
    }
  }, [collapsed, storageKey]);

  return { collapsed, onToggleCollapse };
}

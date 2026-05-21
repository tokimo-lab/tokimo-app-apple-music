import type { MenuBarConfig } from "@tokimo/sdk";
import {
  useShellAppearance,
  useShellMenuBar,
  useShellToast,
  useShellWindowNav,
} from "@tokimo/sdk/react";
import { useAppCtx } from "../AppContext";

export function useMessage() {
  const ctx = useAppCtx();
  return useShellToast(ctx);
}

export function useWindowNavHook() {
  const ctx = useAppCtx();
  return useShellWindowNav(ctx);
}

export function useMenuBar(config: MenuBarConfig | null) {
  const ctx = useAppCtx();
  useShellMenuBar(ctx, config);
}

export function useThemeCore() {
  const ctx = useAppCtx();
  const appearance = useShellAppearance(ctx);
  return {
    isMacStyle: appearance.isMacStyle,
    theme: appearance.theme,
    titleBarStyle: appearance.titleBarStyle,
  };
}

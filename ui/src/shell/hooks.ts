import type { MediaSessionSource, MenuBarConfig } from "@tokimo/app-sdk";
import {
  useShellMediaSession,
  useShellMenuBar,
  useShellToast,
  useShellWindowNav,
} from "@tokimo/app-sdk/react";
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

export function useMediaSessionRegister(source: MediaSessionSource | null) {
  const ctx = useAppCtx();
  useShellMediaSession(ctx, source);
}

export function useMediaSessionOptional() {
  const ctx = useAppCtx();
  return {
    requestPlay: (id: string, provider?: string) =>
      ctx.shell.media.requestPlay(id, provider),
    notifyPause: (id: string, provider?: string) =>
      ctx.shell.media.notifyPause(id, provider),
    notifySaveNeeded: (_id: string, _provider?: string): void => {
      /* no-op in standalone */
    },
    rawPlaybackData: null as null,
    rawPlaybackDataReady: true as boolean,
    activeSource: null as import("@tokimo/app-sdk").MediaSessionSource | null,
  };
}

export function useThemeCore() {
  return { isMacStyle: false };
}

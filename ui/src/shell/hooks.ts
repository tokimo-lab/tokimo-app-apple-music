import type { MediaSessionSource, MenuBarConfig } from "@tokimo/app-sdk";
import {
  useShellMediaSession,
  useShellMediaSessionSnapshot,
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
  const snapshot = useShellMediaSessionSnapshot(ctx);
  return {
    requestPlay: (id: string, provider?: string) =>
      ctx.shell.media.requestPlay(id, provider),
    notifyPause: (id: string, provider?: string) =>
      ctx.shell.media.notifyPause(id, provider),
    notifySaveNeeded: (
      id: string,
      provider?: string,
      immediate?: boolean,
    ): void => ctx.shell.media.notifySaveNeeded(id, provider, immediate),
    activeSource: snapshot.activeSource,
    rawPlaybackData: snapshot.rawPlaybackData as
      | import("../api-types/PlaybackStateData").PlaybackStateData
      | null,
    rawPlaybackDataReady: snapshot.rawPlaybackDataReady,
  };
}

export function useThemeCore() {
  return { isMacStyle: false };
}

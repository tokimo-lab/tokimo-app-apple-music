import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import type { Dispose } from "@tokimo/sdk";
import { defineApp } from "@tokimo/sdk";
import {
  ConfigProvider,
  ToastProvider,
  enUS as uiEnUS,
  zhCN as uiZhCN,
} from "@tokimo/ui";
import { StrictMode } from "react";
import { createRoot, type Root } from "react-dom/client";
import { AppCtxProvider } from "./AppContext";
import AppleMusicContent from "./AppleMusicContent";
import { initEngine } from "./shell/engine-ref";
import "./index.css";

export default defineApp({
  id: "apple-music",
  manifest: {
    id: "apple-music",
    appName: "Apple Music",
    icon: "ListMusic",
    image: "icon.png",
    color: "#FA2D48",
    windowType: "apple-music",
    defaultSize: { width: 1280, height: 850 },
    category: "app",
  },
  mount(container, ctx): Dispose {
    initEngine(ctx.shell.media);

    const queryClient = new QueryClient({
      defaultOptions: { queries: { retry: 1, staleTime: 30_000 } },
    });
    const locale = ctx.locale.startsWith("zh") ? uiZhCN : uiEnUS;
    const root: Root = createRoot(container);

    root.render(
      <StrictMode>
        <AppCtxProvider value={ctx}>
          <QueryClientProvider client={queryClient}>
            <ConfigProvider locale={locale}>
              <ToastProvider>
                <AppleMusicContent />
              </ToastProvider>
            </ConfigProvider>
          </QueryClientProvider>
        </AppCtxProvider>
      </StrictMode>,
    );
    return () => root.unmount();
  },
});

import {
  type AppRuntimeCtx,
  type Dispose,
  defineApp,
  makeTranslator,
} from "@tokimo/app-sdk";
import {
  Button,
  Card,
  ConfigProvider,
  Empty,
  Input,
  ToastProvider,
  enUS as uiEnUS,
  zhCN as uiZhCN,
} from "@tokimo/ui";
import { Sparkles, Trash2 } from "lucide-react";
import { StrictMode, useCallback, useEffect, useState } from "react";
import { createRoot, type Root } from "react-dom/client";
import { enUS, zhCN } from "./i18n";
import "./index.css";

interface Item {
  id: string;
  content: string;
  created_at: string;
}

const SERVICE = "helloworld";

function HelloworldWindow({ ctx }: { ctx: AppRuntimeCtx }) {
  const t = makeTranslator({ "zh-CN": zhCN, "en-US": enUS }, ctx.locale);
  const [items, setItems] = useState<Item[]>([]);
  const [content, setContent] = useState("");
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const refresh = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      const r = await fetch(`/api/apps/${SERVICE}/items`, {
        credentials: "include",
      });
      if (!r.ok) throw new Error(`${r.status} ${await r.text()}`);
      const res = (await r.json()) as { items: Item[] };
      setItems(res.items ?? []);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  const add = useCallback(
    async (notify: boolean) => {
      const text = content.trim();
      if (!text) return;
      setError(null);
      try {
        const url = notify
          ? `/api/apps/${SERVICE}/items/notify`
          : `/api/apps/${SERVICE}/items`;
        const r = await fetch(url, {
          method: "POST",
          credentials: "include",
          headers: { "content-type": "application/json" },
          body: JSON.stringify({ content: text }),
        });
        if (!r.ok) throw new Error(`${r.status} ${await r.text()}`);
        setContent("");
        await refresh();
      } catch (e) {
        setError(e instanceof Error ? e.message : String(e));
      }
    },
    [content, refresh],
  );

  const remove = useCallback(
    async (id: string) => {
      try {
        const r = await fetch(
          `/api/apps/${SERVICE}/items/${encodeURIComponent(id)}`,
          { method: "DELETE", credentials: "include" },
        );
        if (!r.ok) throw new Error(`${r.status} ${await r.text()}`);
        await refresh();
      } catch (e) {
        setError(e instanceof Error ? e.message : String(e));
      }
    },
    [refresh],
  );

  const notifyOnly = useCallback(async () => {
    try {
      await ctx.shell.notify({
        categoryId: "manual",
        categoryLabel: "helloworld.notifications.manual",
        title: t("notifyTitle"),
        body: t("notifyBody"),
        level: "info",
      });
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  }, [ctx.shell, t]);

  return (
    <div className="flex h-full w-full flex-col gap-4 overflow-auto p-6 text-[var(--text-primary)]">
      <header className="flex items-center gap-3">
        <Sparkles size={28} className="text-emerald-500" />
        <div>
          <h1 className="text-xl font-semibold">{t("title")}</h1>
          <p className="text-sm opacity-70">{t("subtitle")}</p>
        </div>
      </header>

      <Card className="p-4">
        <div className="flex gap-2">
          <Input
            value={content}
            onChange={(e) => setContent(e.target.value)}
            placeholder={t("inputPlaceholder")}
            onKeyDown={(e) => {
              if (e.key === "Enter") void add(false);
            }}
          />
          <Button onClick={() => add(false)}>{t("add")}</Button>
          <Button variant="primary" onClick={() => add(true)}>
            {t("addAndNotify")}
          </Button>
          <Button variant="ghost" onClick={notifyOnly}>
            {t("notifyOnly")}
          </Button>
          <Button variant="ghost" onClick={refresh}>
            {t("refresh")}
          </Button>
        </div>
        {error && (
          <div className="mt-2 text-sm text-red-500">
            {t("error")}
            {error}
          </div>
        )}
      </Card>

      <Card className="flex-1 p-4">
        {loading ? (
          <div className="opacity-60">{t("loading")}</div>
        ) : items.length === 0 ? (
          <Empty description={t("empty")} />
        ) : (
          <ul className="flex flex-col gap-2">
            {items.map((it) => (
              <li
                key={it.id}
                className="flex items-center justify-between rounded border border-black/10 px-3 py-2 dark:border-white/10"
              >
                <div className="flex flex-col">
                  <span>{it.content}</span>
                  <span className="text-xs opacity-50">{it.created_at}</span>
                </div>
                <Button
                  variant="ghost"
                  size="small"
                  onClick={() => remove(it.id)}
                >
                  <Trash2 size={14} /> {t("delete")}
                </Button>
              </li>
            ))}
          </ul>
        )}
      </Card>
    </div>
  );
}

export default defineApp({
  id: "helloworld",
  manifest: {
    id: "helloworld",
    appName: "Hello World",
    icon: "Sparkles",
    image: "icon.png",
    color: "#10b981",
    windowType: "helloworld",
    defaultSize: { width: 720, height: 560 },
    category: "app",
  },
  translations: { "zh-CN": zhCN, "en-US": enUS },
  mount(container, ctx): Dispose {
    const root: Root = createRoot(container);
    const locale = ctx.locale.startsWith("zh") ? uiZhCN : uiEnUS;
    // shell adapter must inject the full ShellApi (media / menubar / toast / windowNav).
    const fullCtx: AppRuntimeCtx = ctx;
    root.render(
      <StrictMode>
        <ConfigProvider
          locale={locale}
          theme={{ defaultMode: ctx.theme, defaultAccent: "emerald" }}
        >
          <ToastProvider>
            <HelloworldWindow ctx={fullCtx} />
          </ToastProvider>
        </ConfigProvider>
      </StrictMode>,
    );
    return () => root.unmount();
  },
});

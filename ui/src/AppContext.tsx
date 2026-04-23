import { createContext, useContext } from "react";
import type { AppRuntimeCtx } from "@tokimo/app-sdk";

const Ctx = createContext<AppRuntimeCtx | null>(null);
export const AppCtxProvider = Ctx.Provider;

export function useAppCtx(): AppRuntimeCtx {
  const ctx = useContext(Ctx);
  if (!ctx) throw new Error("useAppCtx: missing AppCtxProvider");
  return ctx;
}

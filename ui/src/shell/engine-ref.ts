import type { LoadAndPlayOptions, ShellMediaApi } from "@tokimo/app-sdk";

let _media: ShellMediaApi | null = null;

export function initEngine(media: ShellMediaApi): void {
  _media = media;
}

function get(): ShellMediaApi {
  if (!_media) throw new Error("[AppleMusic] Engine not initialized");
  return _media;
}

export const loadAndPlay = (url: string, opts: LoadAndPlayOptions) => get().loadAndPlay(url, opts);
export const pause = () => get().pause();
export const resume = () => get().resume();
export const stop = () => get().stop();
export const seek = (time: number) => get().seek(time);
export const setVolume = (vol: number) => get().setVolume(vol);
export const getCurrentTime = () => get().getCurrentTime();
export const getDuration = () => get().getDuration();
export const getAnalyser = () => get().getAnalyser();
export const getActiveProvider = () => get().getActiveProvider();
export const getSnapshot = () => {
  const snap = get().getSnapshot();
  return { ...snap, isBuffering: false, error: null as string | null };
};
export const subscribe = (cb: () => void) => get().subscribe(cb);
export const onEnded = (cb: () => void) => get().onEnded(cb);

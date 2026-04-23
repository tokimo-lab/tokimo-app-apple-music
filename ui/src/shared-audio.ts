const VOLUME_KEY = "apple-music-volume";

export function getStoredAppleMusicVolume(): number {
  try {
    const stored = localStorage.getItem(VOLUME_KEY);
    if (stored) {
      const value = Number.parseFloat(stored);
      if (!Number.isNaN(value) && value >= 0 && value <= 1) return value;
    }
  } catch {
    // ignore
  }
  return 0.5;
}

export function saveStoredAppleMusicVolume(volume: number): void {
  try {
    localStorage.setItem(VOLUME_KEY, String(volume));
  } catch {
    // ignore
  }
}

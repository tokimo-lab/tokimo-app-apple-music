import { Music } from "lucide-react";
import { formatArtworkUrl } from "../types";

interface ArtworkImageProps {
  artwork?: MusicKit.Artwork;
  size: number;
  className?: string;
  alt?: string;
  rounded?: boolean;
}

export function ArtworkImage({
  artwork,
  size,
  className = "",
  alt = "",
  rounded = false,
}: ArtworkImageProps) {
  const url = formatArtworkUrl(artwork, size);
  const roundedClass = rounded ? "rounded-full" : "rounded-lg";

  if (!url) {
    return (
      <div
        className={`flex items-center justify-center bg-gradient-to-br from-[var(--fill-tertiary)] to-[var(--bg-glass)] ${roundedClass} ${className}`}
        style={{ width: size, height: size }}
      >
        <Music
          className="text-[var(--text-tertiary)]"
          size={Math.round(size * 0.4)}
        />
      </div>
    );
  }

  return (
    <img
      src={url}
      alt={alt}
      width={size}
      height={size}
      loading="lazy"
      className={`object-cover ${roundedClass} ${className}`}
      onError={(e) => {
        const target = e.currentTarget;
        target.style.display = "none";
        const fallback = target.nextElementSibling;
        if (fallback instanceof HTMLElement) {
          fallback.style.display = "flex";
        }
      }}
    />
  );
}

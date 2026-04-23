import { ListMusic, Music, Play, User } from "lucide-react";
import { useState } from "react";
import { formatArtworkUrl } from "../types";

interface MediaItemCardProps {
  item: MusicKit.Resource;
  onClick?: () => void;
  onPlay?: () => void;
  onSubtitleClick?: () => void;
  type?: "album" | "playlist" | "artist";
  /** Override artwork (e.g. catalog artwork for library artists) */
  overrideArtwork?: MusicKit.Artwork;
  /** Fallback composite images for playlists without artwork */
  compositeImages?: string[];
}

export function MediaItemCard({
  item,
  onClick,
  onPlay,
  onSubtitleClick,
  type = "album",
  overrideArtwork,
  compositeImages,
}: MediaItemCardProps) {
  const [hovered, setHovered] = useState(false);
  const attrs = item.attributes;
  const name = attrs?.name ?? "Unknown";
  const artwork = overrideArtwork ?? attrs?.artwork;
  const isArtist = type === "artist";

  const subtitle = (() => {
    if (type === "album") return attrs?.artistName ?? "";
    if (type === "playlist") return attrs?.curatorName ?? "";
    return attrs?.genreNames?.[0] ?? "";
  })();

  const artworkUrl = formatArtworkUrl(artwork, 160);
  const roundedClass = isArtist ? "rounded-full" : "rounded-lg";

  // Choose fallback icon based on type
  const FallbackIcon =
    type === "artist" ? User : type === "playlist" ? ListMusic : Music;

  const renderArtwork = () => {
    if (artworkUrl) {
      return (
        <img
          src={artworkUrl}
          alt={name}
          width={160}
          height={160}
          loading="lazy"
          className={`h-full w-full object-cover ${roundedClass}`}
        />
      );
    }
    // Composite artwork for playlists (2×2 grid of track covers)
    if (compositeImages && compositeImages.length > 0) {
      if (compositeImages.length >= 4) {
        return (
          <div
            className={`grid h-full w-full grid-cols-2 grid-rows-2 overflow-hidden ${roundedClass}`}
          >
            {compositeImages.slice(0, 4).map((url) => (
              <img
                key={url}
                src={url}
                alt=""
                className="h-full w-full object-cover"
                loading="lazy"
              />
            ))}
          </div>
        );
      }
      return (
        <img
          src={compositeImages[0]}
          alt={name}
          width={160}
          height={160}
          loading="lazy"
          className={`h-full w-full object-cover ${roundedClass}`}
        />
      );
    }
    return (
      <div
        className={`flex h-full w-full items-center justify-center bg-gradient-to-br from-[var(--fill-tertiary)] to-[var(--bg-glass)] ${roundedClass}`}
      >
        <FallbackIcon className="text-[var(--text-tertiary)]" size={40} />
      </div>
    );
  };

  const alignCenter = isArtist;

  return (
    // biome-ignore lint/a11y/useSemanticElements: outer wrapper can't be <button> because it contains nested <button> elements (play, subtitle)
    <div
      role="button"
      tabIndex={0}
      className={`group flex w-[160px] shrink-0 cursor-pointer flex-col gap-2 rounded-xl p-2 transition-colors hover:bg-[var(--fill-tertiary)] ${
        alignCenter ? "items-center text-center" : "items-start text-left"
      }`}
      onClick={onClick}
      onKeyDown={(e) => {
        if (e.key === "Enter" || e.key === " ") {
          e.preventDefault();
          onClick?.();
        }
      }}
      onMouseEnter={() => setHovered(true)}
      onMouseLeave={() => setHovered(false)}
    >
      <div className="relative h-[144px] w-[144px]">
        {renderArtwork()}

        {hovered && (onPlay || isArtist) && (
          <div
            className={`absolute inset-0 flex items-center justify-center bg-black/30 ${roundedClass}`}
          >
            <button
              type="button"
              className="flex h-11 w-11 cursor-pointer items-center justify-center rounded-full bg-white/90 shadow-lg transition-transform hover:scale-105"
              onClick={(e) => {
                e.stopPropagation();
                if (onPlay) onPlay();
                else onClick?.();
              }}
            >
              <Play className="ml-0.5 text-black" size={20} fill="black" />
            </button>
          </div>
        )}
      </div>

      <div className="w-full min-w-0 px-0.5">
        <p className="truncate text-[13px] font-medium text-[var(--text-primary)]">
          {name}
        </p>
        {subtitle &&
          (onSubtitleClick ? (
            <button
              type="button"
              className={`block w-full cursor-pointer truncate text-xs text-[var(--text-secondary)] hover:underline ${
                alignCenter ? "text-center" : "text-left"
              }`}
              onClick={(e) => {
                e.stopPropagation();
                onSubtitleClick();
              }}
            >
              {subtitle}
            </button>
          ) : (
            <p className="truncate text-xs text-[var(--text-secondary)]">
              {subtitle}
            </p>
          ))}
      </div>
    </div>
  );
}

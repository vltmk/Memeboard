import React from 'react';
import { Play, Square } from 'lucide-react';
import { SoundItem } from '../types';
import { cn } from '../lib/utils';

interface SoundPillProps {
  sound: SoundItem;
  isPlaying: boolean;
  isLocalPreviewing: boolean;
  compact?: boolean;
  onBroadcast: (sound: SoundItem) => void;
  onStopBroadcast: (sound: SoundItem) => void;
  onLocalPreview: (sound: SoundItem) => void;
  onEdit?: (sound: SoundItem) => void;
}

export const SoundPill: React.FC<SoundPillProps> = ({
  sound,
  isPlaying,
  isLocalPreviewing,
  compact = false,
  onBroadcast,
  onStopBroadcast,
  onLocalPreview,
  onEdit,
}) => {
  const handlePillClick = () => {
    if (isPlaying && sound.playbackMode === 'stop') {
      onStopBroadcast(sound);
    } else {
      onBroadcast(sound);
    }
  };

  const handleContextMenu = (e: React.MouseEvent) => {
    if (!onEdit) return;
    e.preventDefault();
    onEdit(sound);
  };

  const handleLocalPlayClick = (e: React.MouseEvent) => {
    e.stopPropagation();
    onLocalPreview(sound);
  };

  return (
    <div
      role="button"
      tabIndex={0}
      onClick={handlePillClick}
      onContextMenu={handleContextMenu}
      onKeyDown={(e) => {
        if (e.key === 'Enter' || e.key === ' ') {
          e.preventDefault();
          handlePillClick();
        }
      }}
      title={
        onEdit
          ? `${sound.name}${sound.hotkey ? ` (${sound.hotkey})` : ''} — Click: Broadcast • ▷: Listen locally • Right-click: Edit`
          : `${sound.name}${sound.hotkey ? ` (${sound.hotkey})` : ''} — Click: Broadcast • ▷: Listen locally`
      }
      className={cn(
        'group inline-flex items-center gap-1.5 rounded-full border select-none transition-all cursor-pointer max-w-full',
        compact ? 'h-7 pl-2 pr-1 text-[11px]' : 'h-8 pl-2.5 pr-1 text-xs',
        isPlaying
          ? 'bg-zinc-100 border-white text-zinc-950 shadow-sm'
          : 'bg-zinc-900/90 hover:bg-zinc-800 border-zinc-800 hover:border-zinc-700 text-zinc-100'
      )}
    >
      <span className="text-sm leading-none shrink-0">{sound.emoji || '🔊'}</span>

      <span className={cn('font-medium truncate', compact ? 'max-w-[82px]' : 'max-w-[110px]')}>
        {sound.name}
      </span>

      {sound.hotkey && (
        <span
          className={cn(
            'font-mono text-[9px] px-1 py-0.2 rounded shrink-0 border',
            isPlaying
              ? 'bg-zinc-200 border-zinc-300 text-zinc-700'
              : 'bg-zinc-950/90 border-zinc-800 text-zinc-400'
          )}
        >
          {sound.hotkey}
        </span>
      )}

      {/* Local-only monitor preview button (never broadcasts into the microphone) */}
      <button
        type="button"
        onClick={handleLocalPlayClick}
        title={isLocalPreviewing ? 'Stop local preview' : 'Play locally (headphones only, not mic)'}
        aria-label={isLocalPreviewing ? `Stop local preview of ${sound.name}` : `Play ${sound.name} locally`}
        className={cn(
          'inline-flex items-center justify-center rounded-full transition-colors shrink-0 cursor-pointer',
          compact ? 'w-5 h-5' : 'w-6 h-6',
          isLocalPreviewing
            ? isPlaying
              ? 'bg-zinc-950 text-white'
              : 'bg-zinc-100 text-zinc-950'
            : isPlaying
              ? 'text-zinc-700 hover:bg-zinc-300 hover:text-zinc-950'
              : 'text-zinc-400 hover:bg-zinc-700 hover:text-zinc-50'
        )}
      >
        {isLocalPreviewing ? (
          <Square className="w-2.5 h-2.5 fill-current" />
        ) : (
          <Play className="w-2.5 h-2.5 fill-current ml-0.5" />
        )}
      </button>
    </div>
  );
};

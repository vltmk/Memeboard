import React from 'react';
import { Play, Square, Settings2, Repeat, Disc3 } from 'lucide-react';
import { SoundItem } from '../types';

interface SoundCardProps {
  sound: SoundItem;
  isPlaying: boolean;
  onPlay: (sound: SoundItem) => void;
  onStop: (sound: SoundItem) => void;
  onEdit: (sound: SoundItem) => void;
}

export const SoundCard: React.FC<SoundCardProps> = ({
  sound,
  isPlaying,
  onPlay,
  onStop,
  onEdit,
}) => {
  const handleClick = (e: React.MouseEvent) => {
    // If clicking edit or buttons, don't trigger play
    if ((e.target as HTMLElement).closest('.action-button')) {
      return;
    }
    if (isPlaying && sound.playbackMode === 'stop') {
      onStop(sound);
    } else {
      onPlay(sound);
    }
  };

  const getModeIcon = () => {
    switch (sound.playbackMode) {
      case 'overlap':
        return (
          <span title="Mode: Overlap / Polyphonic">
            <Disc3 className="w-3 h-3 text-[#23a55a]" />
          </span>
        );
      case 'stop':
        return (
          <span title="Mode: Toggle Stop">
            <Square className="w-2.5 h-2.5 text-[#f0b232]" />
          </span>
        );
      case 'restart':
      default:
        return (
          <span title="Mode: Restart from Beginning">
            <Repeat className="w-3 h-3 text-[#5865f2]" />
          </span>
        );
    }
  };

  return (
    <div
      onClick={handleClick}
      className={`group relative flex flex-col justify-between p-3 rounded-xl cursor-pointer transition-all duration-150 select-none border ${
        isPlaying
          ? 'bg-[#313338] border-[#5865f2] ring-2 ring-[#5865f2]/40 shadow-lg shadow-[#5865f2]/20'
          : 'bg-[#2b2d31] hover:bg-[#35373c] border-[#313338] hover:border-[#4e5058]/50 shadow-sm'
      }`}
    >
      {/* Top row: Emoji avatar + Quick Mode Badge + Edit button */}
      <div className="flex items-start justify-between">
        <div className="w-12 h-12 rounded-xl bg-[#1e1f22] flex items-center justify-center text-2xl shadow-inner group-hover:scale-105 transition-transform">
          {sound.emoji || '🎵'}
        </div>

        <div className="flex items-center gap-1.5 opacity-80 group-hover:opacity-100 transition-opacity">
          <div className="p-1 rounded bg-[#1e1f22]/70 text-[#949ba4]">{getModeIcon()}</div>
          <button
            onClick={(e) => {
              e.stopPropagation();
              onEdit(sound);
            }}
            className="action-button p-1.5 rounded-lg bg-[#1e1f22] hover:bg-[#5865f2] text-[#949ba4] hover:text-white transition-colors"
            title="Edit Sound & Waveform Trimmer"
          >
            <Settings2 className="w-3.5 h-3.5" />
          </button>
        </div>
      </div>

      {/* Center: Sound Name & Category */}
      <div className="mt-3">
        <h3 className="font-semibold text-sm text-[#f2f3f5] truncate tracking-tight group-hover:text-white">
          {sound.name}
        </h3>
        <p className="text-[11px] text-[#949ba4] truncate font-medium">
          {sound.category || 'General'}
        </p>
      </div>

      {/* Bottom row: Hotkey Badge & Play / Stop State */}
      <div className="mt-3 pt-2 border-t border-[#35373c]/60 flex items-center justify-between text-xs">
        {sound.hotkey ? (
          <span className="font-mono text-[10px] font-semibold bg-[#1e1f22] text-[#dbdee1] px-2 py-0.5 rounded border border-[#383a40]">
            {sound.hotkey}
          </span>
        ) : (
          <span className="text-[10px] text-[#80848e]">No keybind</span>
        )}

        <div className="flex items-center gap-1.5">
          {isPlaying ? (
            <div className="flex items-center gap-1 text-[#23a55a] font-medium text-[11px]">
              <span className="flex space-x-0.5 items-end h-3">
                <span className="w-0.5 h-3 bg-[#23a55a] animate-pulse"></span>
                <span className="w-0.5 h-2 bg-[#23a55a] animate-pulse delay-75"></span>
                <span className="w-0.5 h-2.5 bg-[#23a55a] animate-pulse delay-150"></span>
              </span>
              <span>Playing</span>
            </div>
          ) : (
            <div className="p-1 rounded-full text-[#949ba4] group-hover:text-[#dbdee1] group-hover:bg-[#1e1f22] transition-colors">
              <Play className="w-3.5 h-3.5 fill-current" />
            </div>
          )}
        </div>
      </div>
    </div>
  );
};

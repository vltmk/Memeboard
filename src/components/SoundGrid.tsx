import React, { useMemo, useState } from 'react';
import { Search, Upload, FolderOpen, Music, Wrench } from 'lucide-react';
import { SoundItem, VirtualMicStatus } from '../types';
import { SoundPill } from './SoundPill';
import { Button } from './ui/button';
import { Input } from './ui/input';
import { cn } from '../lib/utils';

interface SoundGridProps {
  sounds: SoundItem[];
  playingIds: string[];
  localPreviewId: string | null;
  isDraggingFile: boolean;
  virtualMicStatus: VirtualMicStatus | null;
  broadcastStreamActive: boolean;
  onBroadcastSound: (sound: SoundItem) => void;
  onStopBroadcast: (sound: SoundItem) => void;
  onLocalPreview: (sound: SoundItem) => void;
  onEditSound: (sound: SoundItem) => void;
  onAddSoundClick: () => void;
  onOpenSettings: () => void;
  onInstallVirtualDriver: () => void;
  isInstallingDriver: boolean;
}

export const SoundGrid: React.FC<SoundGridProps> = ({
  sounds,
  playingIds,
  localPreviewId,
  isDraggingFile,
  virtualMicStatus,
  broadcastStreamActive,
  onBroadcastSound,
  onStopBroadcast,
  onLocalPreview,
  onEditSound,
  onAddSoundClick,
  onOpenSettings,
  onInstallVirtualDriver,
  isInstallingDriver,
}) => {
  const [searchQuery, setSearchQuery] = useState('');
  const [selectedCategory, setSelectedCategory] = useState<string>('All');

  const categories = useMemo(() => {
    const cats = new Set<string>(['All']);
    sounds.forEach((s) => {
      if (s.category?.trim()) cats.add(s.category.trim());
    });
    return Array.from(cats);
  }, [sounds]);

  const activeCategory = categories.includes(selectedCategory)
    ? selectedCategory
    : 'All';

  const filteredSounds = useMemo(() => {
    const q = searchQuery.toLowerCase().trim();
    return sounds.filter((sound) => {
      const cat = sound.category?.trim() || 'General';
      const matchesCat = activeCategory === 'All' || cat === activeCategory;
      const matchesSearch =
        !q ||
        sound.name.toLowerCase().includes(q) ||
        cat.toLowerCase().includes(q) ||
        (sound.hotkey && sound.hotkey.toLowerCase().includes(q));
      return matchesCat && matchesSearch;
    });
  }, [sounds, searchQuery, activeCategory]);

  const showDriverBanner =
    !broadcastStreamActive &&
    virtualMicStatus &&
    (!virtualMicStatus.driverInstalled || !virtualMicStatus.virtualDeviceAttached);

  return (
    <div className="relative flex-1 flex flex-col overflow-hidden bg-zinc-950">
      {/* Drag-and-Drop Overlay Hint */}
      {isDraggingFile && (
        <div className="absolute inset-0 z-40 m-2.5 rounded-xl border border-dashed border-zinc-300 bg-zinc-950/90 backdrop-blur-xs flex flex-col items-center justify-center pointer-events-none">
          <div className="w-10 h-10 rounded-full bg-zinc-100 text-zinc-950 flex items-center justify-center mb-2">
            <Upload className="w-5 h-5" />
          </div>
          <p className="text-xs font-semibold text-zinc-50">Drop Audio Files</p>
          <p className="text-[10px] text-zinc-400 mt-0.5">
            MP3, WAV, OGG, FLAC, M4A, Opus
          </p>
        </div>
      )}

      {/* Compact Driver Setup Banner */}
      {showDriverBanner && virtualMicStatus && (
        <div className="mx-3 mt-2.5 px-3 py-2 rounded-lg bg-zinc-900 border border-zinc-800 flex items-center justify-between gap-2 shrink-0">
          <div className="min-w-0">
            <p className="text-[11px] font-medium text-zinc-100 truncate">
              {virtualMicStatus.driverInstalled
                ? 'Virtual Mic Standby'
                : 'Setup Built-in Virtual Mic'}
            </p>
            <p className="text-[10px] text-zinc-400 truncate">
              {virtualMicStatus.statusMessage}
            </p>
          </div>
          <div className="flex items-center gap-1 shrink-0">
            <Button type="button" variant="ghost" size="xs" onClick={onOpenSettings}>
              Settings
            </Button>
            <Button
              type="button"
              variant="default"
              size="xs"
              onClick={onInstallVirtualDriver}
              disabled={isInstallingDriver}
            >
              <Wrench className="w-3 h-3" />
              <span>{isInstallingDriver ? 'Wait...' : 'Setup'}</span>
            </Button>
          </div>
        </div>
      )}

      {/* Compact Filter + Search */}
      {sounds.length > 0 && (
        <div className="px-3 pt-2.5 pb-1.5 space-y-2 shrink-0">
          <div className="relative">
            <Search className="w-3.5 h-3.5 text-zinc-500 absolute left-2.5 top-1/2 -translate-y-1/2 pointer-events-none" />
            <Input
              type="text"
              value={searchQuery}
              onChange={(e) => setSearchQuery(e.target.value)}
              placeholder="Search sounds or hotkeys..."
              className="pl-8 h-7 text-[11px] bg-zinc-900 border-zinc-800"
            />
          </div>

          {categories.length > 1 && (
            <div className="flex items-center gap-1 overflow-x-auto pb-0.5">
              {categories.map((cat) => (
                <button
                  key={cat}
                  type="button"
                  onClick={() => setSelectedCategory(cat)}
                  className={cn(
                    'px-2 py-0.5 rounded-full text-[10px] font-medium whitespace-nowrap transition-colors cursor-pointer border',
                    activeCategory === cat
                      ? 'bg-zinc-100 text-zinc-950 border-zinc-100'
                      : 'bg-zinc-900 text-zinc-400 border-zinc-800 hover:text-zinc-100'
                  )}
                >
                  {cat}
                </button>
              ))}
            </div>
          )}
        </div>
      )}

      {/* Dense Wrapping Discord-Style Pills */}
      <div className="flex-1 overflow-y-auto px-3 py-2">
        {sounds.length === 0 ? (
          <div className="h-full flex flex-col items-center justify-center text-center px-4">
            <div className="w-10 h-10 rounded-full bg-zinc-900 border border-zinc-800 flex items-center justify-center mb-3">
              <Music className="w-4 h-4 text-zinc-300" />
            </div>
            <p className="text-xs font-semibold text-zinc-100">No Sounds Yet</p>
            <p className="text-[11px] text-zinc-400 mt-1 max-w-[230px] leading-relaxed">
              Drop audio clips here or click below to build your soundboard.
            </p>
            <Button
              type="button"
              variant="default"
              size="sm"
              onClick={onAddSoundClick}
              className="mt-4"
            >
              <FolderOpen className="w-3.5 h-3.5" />
              <span>Add Audio Files</span>
            </Button>
          </div>
        ) : filteredSounds.length === 0 ? (
          <div className="h-full flex flex-col items-center justify-center text-center py-8 text-zinc-500">
            <Search className="w-5 h-5 mb-1.5 opacity-50" />
            <p className="text-xs text-zinc-300">No matching sounds</p>
          </div>
        ) : (
          <div className="flex flex-wrap gap-1.5 content-start pb-2">
            {filteredSounds.map((sound) => (
              <SoundPill
                key={sound.id}
                sound={sound}
                isPlaying={playingIds.includes(sound.id)}
                isLocalPreviewing={localPreviewId === sound.id}
                onBroadcast={onBroadcastSound}
                onStopBroadcast={onStopBroadcast}
                onLocalPreview={onLocalPreview}
                onEdit={onEditSound}
              />
            ))}
          </div>
        )}
      </div>

      {/* Minimal Bottom Footer */}
      <div className="px-3 py-1.5 border-t border-zinc-900 flex items-center justify-between text-[10px] text-zinc-500 shrink-0">
        <span>Click: Mic • ▷ Local • Right-click: Edit</span>
        <span className="truncate max-w-[130px]" title={virtualMicStatus?.savedPhysicalMic || undefined}>
          {virtualMicStatus?.virtualMicIsDefault ? 'Default: Virtual Mic' : 'Manual Routing'}
        </span>
      </div>
    </div>
  );
};

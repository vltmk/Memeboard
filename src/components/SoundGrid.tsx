import React, { useMemo, useState } from 'react';
import {
  Search,
  Upload,
  FolderOpen,
  Music,
  ShieldCheck,
  Wrench,
  Sparkles,
} from 'lucide-react';
import { SoundItem, VirtualMicStatus } from '../types';
import { SoundCard } from './SoundCard';

interface SoundGridProps {
  sounds: SoundItem[];
  playingIds: string[];
  isDraggingFile: boolean;
  virtualMicStatus: VirtualMicStatus | null;
  broadcastStreamActive: boolean;
  onPlaySound: (id: string) => void;
  onStopSound: (id: string) => void;
  onEditSound: (sound: SoundItem) => void;
  onAddSoundClick: () => void;
  onOpenSettings: () => void;
  onInstallVirtualDriver: () => void;
  isInstallingDriver: boolean;
}

export const SoundGrid: React.FC<SoundGridProps> = ({
  sounds,
  playingIds,
  isDraggingFile,
  virtualMicStatus,
  broadcastStreamActive,
  onPlaySound,
  onStopSound,
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
    <div className="relative flex-1 flex flex-col overflow-hidden bg-[#313338]">
      {/* Drag-and-Drop Overlay Hint */}
      {isDraggingFile && (
        <div className="absolute inset-0 z-40 m-4 rounded-2xl border-2 border-dashed border-[#5865f2] bg-[#5865f2]/15 backdrop-blur-sm flex flex-col items-center justify-center pointer-events-none">
          <div className="w-16 h-16 rounded-2xl bg-[#5865f2] flex items-center justify-center shadow-xl mb-3">
            <Upload className="w-8 h-8 text-white" />
          </div>
          <p className="text-lg font-bold text-white">Drop Audio Files to Add</p>
          <p className="text-xs text-[#dbdee1] mt-1">
            Supports MP3, WAV, OGG, FLAC, M4A &amp; Opus
          </p>
        </div>
      )}

      {/* Virtual Mic Onboarding Banner (only when built-in driver is not yet active) */}
      {showDriverBanner && virtualMicStatus && (
        <div className="mx-5 mt-4 p-3.5 rounded-xl bg-[#2b2d31] border border-[#f0b232]/35 flex flex-wrap items-center justify-between gap-3 shrink-0">
          <div className="flex items-start gap-3 min-w-0">
            <div className="p-2 rounded-lg bg-[#f0b232]/15 text-[#f0b232] shrink-0">
              <ShieldCheck className="w-5 h-5" />
            </div>
            <div className="min-w-0">
              <p className="text-xs font-bold text-white">
                {virtualMicStatus.driverInstalled
                  ? 'Built-in Virtual Mic Standby'
                  : 'Complete Built-in Virtual Microphone Setup'}
              </p>
              <p className="text-[11px] text-[#b5bac1] mt-0.5 truncate max-w-2xl">
                {virtualMicStatus.statusMessage}
              </p>
            </div>
          </div>
          <div className="flex items-center gap-2 shrink-0">
            <button
              type="button"
              onClick={onOpenSettings}
              className="px-3 py-1.5 rounded-lg bg-[#35373c] hover:bg-[#404249] text-xs font-semibold text-[#dbdee1] transition-colors"
            >
              Audio Settings
            </button>
            <button
              type="button"
              onClick={onInstallVirtualDriver}
              disabled={isInstallingDriver}
              className="flex items-center gap-1.5 px-3 py-1.5 rounded-lg bg-[#5865f2] hover:bg-[#4752c4] disabled:opacity-60 text-xs font-semibold text-white transition-colors"
            >
              <Wrench className="w-3.5 h-3.5" />
              <span>
                {isInstallingDriver
                  ? 'Setting up...'
                  : virtualMicStatus.driverInstalled
                  ? 'Attach Virtual Mic'
                  : 'Setup Virtual Mic'}
              </span>
            </button>
          </div>
        </div>
      )}

      {/* Search & Category Filter Bar (shown once user has sounds) */}
      {sounds.length > 0 && (
        <div className="px-5 pt-4 pb-2 flex flex-wrap items-center justify-between gap-3 shrink-0">
          <div className="flex items-center gap-1.5 overflow-x-auto pb-1">
            {categories.map((cat) => (
              <button
                key={cat}
                type="button"
                onClick={() => setSelectedCategory(cat)}
                className={`px-3 py-1.5 rounded-lg text-xs font-semibold whitespace-nowrap transition-colors ${
                  activeCategory === cat
                    ? 'bg-[#5865f2] text-white'
                    : 'bg-[#2b2d31] text-[#b5bac1] hover:bg-[#35373c] hover:text-white'
                }`}
              >
                {cat}
              </button>
            ))}
          </div>

          <div className="relative w-64">
            <Search className="w-4 h-4 text-[#949ba4] absolute left-3 top-1/2 -translate-y-1/2 pointer-events-none" />
            <input
              type="text"
              value={searchQuery}
              onChange={(e) => setSearchQuery(e.target.value)}
              placeholder="Search sounds or hotkeys..."
              className="w-full bg-[#1e1f22] text-sm text-[#f2f3f5] placeholder-[#72767d] pl-9 pr-3 py-1.5 rounded-lg border border-transparent focus:border-[#5865f2] outline-none transition-colors"
            />
          </div>
        </div>
      )}

      {/* Main Content Area */}
      <div className="flex-1 overflow-y-auto px-5 py-4">
        {sounds.length === 0 ? (
          <div className="h-full flex flex-col items-center justify-center text-center max-w-md mx-auto py-8">
            <div className="w-16 h-16 rounded-2xl bg-[#2b2d31] border border-[#35373c] flex items-center justify-center mb-4 shadow-lg">
              <Music className="w-8 h-8 text-[#5865f2]" />
            </div>
            <h2 className="text-lg font-bold text-white">Your Soundboard Is Clean &amp; Ready</h2>
            <p className="text-sm text-[#949ba4] mt-2 leading-relaxed">
              Drag and drop audio clips anywhere into this window, or choose files from your PC to
              build your custom soundboard.
            </p>
            <div className="flex items-center gap-3 mt-6">
              <button
                type="button"
                onClick={onAddSoundClick}
                className="flex items-center gap-2 px-5 py-2.5 rounded-xl bg-[#5865f2] hover:bg-[#4752c4] text-white text-sm font-semibold shadow-lg shadow-[#5865f2]/25 transition-all cursor-pointer"
              >
                <FolderOpen className="w-4 h-4" />
                <span>Add Audio Files</span>
              </button>
            </div>
            <div className="mt-6 flex items-center gap-2 text-[11px] text-[#72767d]">
              <Sparkles className="w-3.5 h-3.5 text-[#5865f2]" />
              <span>Supports MP3, WAV, OGG, FLAC, M4A &amp; Opus • ITU-R BS.1770 Normalized</span>
            </div>
          </div>
        ) : filteredSounds.length === 0 ? (
          <div className="h-full flex flex-col items-center justify-center text-center py-12 text-[#949ba4]">
            <Search className="w-8 h-8 mb-2 opacity-40" />
            <p className="text-sm font-semibold text-[#dbdee1]">No matching sounds</p>
            <p className="text-xs mt-1">Try a different search term or category filter.</p>
          </div>
        ) : (
          <div className="grid grid-cols-2 sm:grid-cols-3 md:grid-cols-4 xl:grid-cols-5 gap-3 pb-4">
            {filteredSounds.map((sound) => (
              <SoundCard
                key={sound.id}
                sound={sound}
                isPlaying={playingIds.includes(sound.id)}
                onPlay={(s) => onPlaySound(s.id)}
                onStop={(s) => onStopSound(s.id)}
                onEdit={onEditSound}
              />
            ))}
          </div>
        )}
      </div>
    </div>
  );
};

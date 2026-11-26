import React, { useEffect, useMemo, useState } from 'react';
import {
  Search,
  Upload,
  FolderOpen,
  Music,
  Wrench,
  Plus,
  Download,
  Pencil,
  Trash2,
  Play,
  Square,
  Headphones,
  Tag,
  Package,
  SlidersHorizontal,
} from 'lucide-react';
import { SoundItem, VirtualMicStatus } from '../types';
import { SoundPill } from './SoundPill';
import { Button } from './ui/button';
import { Input } from './ui/input';
import { cn } from '../lib/utils';

interface SoundGridProps {
  sounds: SoundItem[];
  customCategories: string[];
  playingIds: string[];
  localPreviewId: string | null;
  isDraggingFile: boolean;
  isDocked: boolean;
  virtualMicStatus: VirtualMicStatus | null;
  broadcastStreamActive: boolean;
  onBroadcastSound: (sound: SoundItem) => void;
  onStopBroadcast: (sound: SoundItem) => void;
  onLocalPreview: (sound: SoundItem) => void;
  onEditSound: (sound: SoundItem) => void;
  onDownloadSound: (sound: SoundItem) => void;
  onDeleteSound: (sound: SoundItem) => void;
  onMoveSoundCategory: (sound: SoundItem, category: string) => void;
  onAddSoundClick: (targetCategory?: string) => void;
  onCreateCategory: () => void;
  onEditCategory: (category: string) => void;
  onDeleteCategory: (category: string) => void;
  onExportCategoryPack: (category: string) => void;
  onOpenPacks: () => void;
  onOpenSettings: () => void;
  onInstallVirtualDriver: () => void;
  isInstallingDriver: boolean;
}

type ContextMenuState =
  | { kind: 'sound'; sound: SoundItem; x: number; y: number }
  | { kind: 'category'; category: string; x: number; y: number }
  | null;

export const SoundGrid: React.FC<SoundGridProps> = ({
  sounds,
  customCategories,
  playingIds,
  localPreviewId,
  isDraggingFile,
  isDocked,
  virtualMicStatus,
  broadcastStreamActive,
  onBroadcastSound,
  onStopBroadcast,
  onLocalPreview,
  onEditSound,
  onDownloadSound,
  onDeleteSound,
  onMoveSoundCategory,
  onAddSoundClick,
  onCreateCategory,
  onEditCategory,
  onDeleteCategory,
  onExportCategoryPack,
  onOpenPacks,
  onOpenSettings,
  onInstallVirtualDriver,
  isInstallingDriver,
}) => {
  const [searchQuery, setSearchQuery] = useState('');
  const [selectedCategory, setSelectedCategory] = useState<string>('All');
  const [contextMenu, setContextMenu] = useState<ContextMenuState>(null);

  useEffect(() => {
    if (!contextMenu) return;
    const close = () => setContextMenu(null);
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') close();
    };
    window.addEventListener('pointerdown', close);
    window.addEventListener('keydown', onKey);
    window.addEventListener('blur', close);
    return () => {
      window.removeEventListener('pointerdown', close);
      window.removeEventListener('keydown', onKey);
      window.removeEventListener('blur', close);
    };
  }, [contextMenu]);

  const categories = useMemo(() => {
    const seen = new Map<string, string>();
    const addCat = (raw: string) => {
      const trimmed = raw.trim();
      if (!trimmed || trimmed.toLowerCase() === 'all') return;
      const key = trimmed.toLowerCase();
      if (!seen.has(key)) seen.set(key, trimmed);
    };
    customCategories.forEach(addCat);
    sounds.forEach((s) => addCat(s.category || 'General'));
    return ['All', ...Array.from(seen.values())];
  }, [sounds, customCategories]);

  const activeCategory = categories.find(
    (c) => c.toLowerCase() === selectedCategory.toLowerCase()
  ) ?? 'All';

  const filteredSounds = useMemo(() => {
    const q = searchQuery.toLowerCase().trim();
    return sounds.filter((sound) => {
      const cat = sound.category?.trim() || 'General';
      const matchesCat =
        activeCategory === 'All' ||
        cat.toLowerCase() === activeCategory.toLowerCase();
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

  const assignableCategories = categories.filter((c) => c !== 'All');
  const clampMenuPos = (x: number, y: number, w = 198, h = 250) => ({
    left: Math.max(8, Math.min(x, (typeof window !== 'undefined' ? window.innerWidth : 396) - w - 8)),
    top: Math.max(8, Math.min(y, (typeof window !== 'undefined' ? window.innerHeight : 640) - h - 8)),
  });

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
            MP3, WAV, OGG, FLAC, M4A, Opus or .memeboard pack
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

      {/* Compact Search + Add + Category Filter Pills */}
      {(sounds.length > 0 || customCategories.length > 0) && (
        <div className="px-3 pt-2.5 pb-1.5 space-y-2 shrink-0">
          <div className="flex items-center gap-1.5">
            <div className="relative flex-1 min-w-0">
              <Search className="w-3.5 h-3.5 text-zinc-500 absolute left-2.5 top-1/2 -translate-y-1/2 pointer-events-none" />
              <Input
                dir="auto"
                type="text"
                value={searchQuery}
                onChange={(e) => setSearchQuery(e.target.value)}
                placeholder="Search sounds or hotkeys..."
                className="pl-8 h-7 text-[11px] bg-zinc-900 border-zinc-800"
              />
            </div>
            <Button
              type="button"
              variant="default"
              size="xs"
              onClick={() =>
                onAddSoundClick(activeCategory === 'All' ? undefined : activeCategory)
              }
              title={
                activeCategory === 'All'
                  ? 'Add Audio Files'
                  : `Add Audio Files to ${activeCategory}`
              }
              className="h-7 px-2.5 shrink-0"
            >
              <Plus className="w-3 h-3" />
              <span>Add</span>
            </Button>
          </div>

          <div className="flex items-center gap-1 overflow-x-auto pb-0.5">
            {categories.map((cat) => {
              const isSelected = activeCategory.toLowerCase() === cat.toLowerCase();
              return (
                <button
                  key={cat}
                  type="button"
                  dir="auto"
                  onClick={() => setSelectedCategory(cat)}
                  onContextMenu={(e) => {
                    if (cat === 'All') return;
                    e.preventDefault();
                    e.stopPropagation();
                    setContextMenu({
                      kind: 'category',
                      category: cat,
                      x: e.clientX,
                      y: e.clientY,
                    });
                  }}
                  title={
                    cat === 'All'
                      ? 'Show all sounds'
                      : `${cat} — Click to filter • Right-click to edit or export`
                  }
                  className={cn(
                    'px-2 py-0.5 rounded-full text-[10px] font-medium whitespace-nowrap transition-colors cursor-pointer border shrink-0',
                    isSelected
                      ? 'bg-zinc-100 text-zinc-950 border-zinc-100'
                      : 'bg-zinc-900 text-zinc-400 border-zinc-800 hover:text-zinc-100 hover:border-zinc-700'
                  )}
                >
                  {cat}
                </button>
              );
            })}

            {activeCategory !== 'All' && (
              <button
                type="button"
                onClick={() => onEditCategory(activeCategory)}
                title={`Edit "${activeCategory}" category & clips`}
                className="inline-flex items-center gap-1 px-2 py-0.5 rounded-full text-[10px] font-medium border border-zinc-800 bg-zinc-900/70 text-zinc-300 hover:text-white hover:border-zinc-600 transition-colors cursor-pointer shrink-0"
              >
                <SlidersHorizontal className="w-2.5 h-2.5" />
                <span>Edit</span>
              </button>
            )}

            <button
              type="button"
              onClick={onCreateCategory}
              title="Create new category pill"
              className="inline-flex items-center gap-0.5 px-2 py-0.5 rounded-full text-[10px] font-medium border border-dashed border-zinc-700 bg-zinc-950 text-zinc-400 hover:text-zinc-100 hover:border-zinc-500 transition-colors cursor-pointer shrink-0"
            >
              <Plus className="w-2.5 h-2.5" />
              <span>Category</span>
            </button>
          </div>
        </div>
      )}

      {/* Dense Wrapping Discord-Style Pills */}
      <div
        className="flex-1 overflow-y-auto px-3 py-2"
        onScroll={() => contextMenu && setContextMenu(null)}
      >
        {sounds.length === 0 ? (
          <div className="h-full flex flex-col items-center justify-center text-center px-4">
            <div className="w-10 h-10 rounded-full bg-zinc-900 border border-zinc-800 flex items-center justify-center mb-3">
              <Music className="w-4 h-4 text-zinc-300" />
            </div>
            <p className="text-xs font-semibold text-zinc-100">No Sounds Yet</p>
            <p className="text-[11px] text-zinc-400 mt-1 max-w-[240px] leading-relaxed">
              Drop audio clips or a <span className="font-mono text-zinc-300">.memeboard</span> pack here, or click below to start.
            </p>
            <div className="flex items-center gap-2 mt-4">
              <Button
                type="button"
                variant="default"
                size="sm"
                onClick={() => onAddSoundClick()}
              >
                <FolderOpen className="w-3.5 h-3.5" />
                <span>Add Audio Files</span>
              </Button>
              <Button
                type="button"
                variant="outline"
                size="sm"
                onClick={onOpenPacks}
              >
                <Package className="w-3.5 h-3.5" />
                <span>Import Pack</span>
              </Button>
            </div>
          </div>
        ) : filteredSounds.length === 0 ? (
          <div className="h-full flex flex-col items-center justify-center text-center py-8 text-zinc-500 gap-2">
            <Search className="w-5 h-5 opacity-50" />
            <p className="text-xs text-zinc-300">
              {activeCategory !== 'All'
                ? `No clips in "${activeCategory}"`
                : 'No matching sounds'}
            </p>
            {activeCategory !== 'All' && (
              <div className="flex items-center gap-1.5 mt-1">
                <Button
                  type="button"
                  variant="secondary"
                  size="xs"
                  onClick={() => onEditCategory(activeCategory)}
                >
                  <SlidersHorizontal className="w-3 h-3" /> Assign Clips
                </Button>
                <Button
                  type="button"
                  variant="default"
                  size="xs"
                  onClick={() => onAddSoundClick(activeCategory)}
                >
                  <Plus className="w-3 h-3" /> Add Audio
                </Button>
              </div>
            )}
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
                onContextMenu={(s, x, y) =>
                  setContextMenu({ kind: 'sound', sound: s, x, y })
                }
              />
            ))}
          </div>
        )}
      </div>

      {/* Custom Right-Click Context Menu */}
      {contextMenu?.kind === 'sound' && (
        <div
          onPointerDown={(e) => e.stopPropagation()}
          style={clampMenuPos(contextMenu.x, contextMenu.y, 206, 272)}
          className="fixed z-50 w-[206px] rounded-lg border border-zinc-800 bg-zinc-950/98 shadow-2xl p-1 text-[11px] text-zinc-200 animate-in fade-in"
        >
          <div className="px-2 py-1 border-b border-zinc-800/80 mb-1 flex items-center gap-1.5">
            <span className="text-xs shrink-0">{contextMenu.sound.emoji || '🔊'}</span>
            <span dir="auto" className="font-semibold text-zinc-100 truncate">
              {contextMenu.sound.name}
            </span>
          </div>

          <button
            type="button"
            onClick={() => {
              const s = contextMenu.sound;
              setContextMenu(null);
              if (playingIds.includes(s.id)) onStopBroadcast(s);
              else onBroadcastSound(s);
            }}
            className="w-full px-2 py-1.5 rounded-md hover:bg-zinc-800 flex items-center gap-2 text-left cursor-pointer"
          >
            {playingIds.includes(contextMenu.sound.id) ? (
              <>
                <Square className="w-3.5 h-3.5 text-zinc-300 fill-current" />
                <span>Stop Broadcast</span>
              </>
            ) : (
              <>
                <Play className="w-3.5 h-3.5 text-zinc-300 fill-current" />
                <span>Play in Mic</span>
              </>
            )}
          </button>

          <button
            type="button"
            onClick={() => {
              const s = contextMenu.sound;
              setContextMenu(null);
              onLocalPreview(s);
            }}
            className="w-full px-2 py-1.5 rounded-md hover:bg-zinc-800 flex items-center gap-2 text-left cursor-pointer"
          >
            <Headphones className="w-3.5 h-3.5 text-zinc-400" />
            <span>
              {localPreviewId === contextMenu.sound.id ? 'Stop Local Preview' : 'Preview Locally'}
            </span>
          </button>

          <div className="my-1 border-t border-zinc-800/80" />

          <button
            type="button"
            onClick={() => {
              const s = contextMenu.sound;
              setContextMenu(null);
              onEditSound(s);
            }}
            className="w-full px-2 py-1.5 rounded-md hover:bg-zinc-800 flex items-center gap-2 text-left cursor-pointer"
          >
            <Pencil className="w-3.5 h-3.5 text-zinc-400" />
            <span>Edit Sound…</span>
          </button>

          <button
            type="button"
            onClick={() => {
              const s = contextMenu.sound;
              setContextMenu(null);
              onDownloadSound(s);
            }}
            className="w-full px-2 py-1.5 rounded-md hover:bg-zinc-800 flex items-center gap-2 text-left cursor-pointer"
          >
            <Download className="w-3.5 h-3.5 text-zinc-400" />
            <span>Download Audio…</span>
          </button>

          {assignableCategories.length > 0 && (
            <div className="mt-1 pt-1 border-t border-zinc-800/80 px-2 py-1 space-y-1">
              <div className="flex items-center gap-1 text-[9px] font-mono uppercase tracking-wider text-zinc-500">
                <Tag className="w-2.5 h-2.5" /> Category
              </div>
              <div className="flex flex-wrap gap-1 max-h-20 overflow-y-auto">
                {assignableCategories.map((cat) => {
                  const active =
                    (contextMenu.sound.category?.trim() || 'General').toLowerCase() ===
                    cat.toLowerCase();
                  return (
                    <button
                      key={cat}
                      type="button"
                      dir="auto"
                      onClick={() => {
                        const s = contextMenu.sound;
                        setContextMenu(null);
                        onMoveSoundCategory(s, cat);
                      }}
                      className={cn(
                        'px-1.5 py-0.5 rounded text-[10px] border transition-colors cursor-pointer',
                        active
                          ? 'bg-zinc-100 text-zinc-950 border-zinc-100'
                          : 'bg-zinc-900 text-zinc-300 border-zinc-800 hover:border-zinc-600'
                      )}
                    >
                      {cat}
                    </button>
                  );
                })}
              </div>
            </div>
          )}

          <div className="my-1 border-t border-zinc-800/80" />

          <button
            type="button"
            onClick={() => {
              const s = contextMenu.sound;
              setContextMenu(null);
              onDeleteSound(s);
            }}
            className="w-full px-2 py-1.5 rounded-md hover:bg-red-950/60 text-red-400 flex items-center gap-2 text-left cursor-pointer"
          >
            <Trash2 className="w-3.5 h-3.5" />
            <span>Delete</span>
          </button>
        </div>
      )}

      {contextMenu?.kind === 'category' && (
        <div
          onPointerDown={(e) => e.stopPropagation()}
          style={clampMenuPos(contextMenu.x, contextMenu.y, 196, 160)}
          className="fixed z-50 w-[196px] rounded-lg border border-zinc-800 bg-zinc-950/98 shadow-2xl p-1 text-[11px] text-zinc-200 animate-in fade-in"
        >
          <div className="px-2 py-1 border-b border-zinc-800/80 mb-1 flex items-center gap-1.5">
            <Tag className="w-3 h-3 text-zinc-400 shrink-0" />
            <span dir="auto" className="font-semibold text-zinc-100 truncate">
              {contextMenu.category}
            </span>
          </div>

          <button
            type="button"
            onClick={() => {
              const cat = contextMenu.category;
              setContextMenu(null);
              onEditCategory(cat);
            }}
            className="w-full px-2 py-1.5 rounded-md hover:bg-zinc-800 flex items-center gap-2 text-left cursor-pointer"
          >
            <SlidersHorizontal className="w-3.5 h-3.5 text-zinc-400" />
            <span>Edit Category &amp; Clips…</span>
          </button>

          <button
            type="button"
            onClick={() => {
              const cat = contextMenu.category;
              setContextMenu(null);
              onAddSoundClick(cat);
            }}
            className="w-full px-2 py-1.5 rounded-md hover:bg-zinc-800 flex items-center gap-2 text-left cursor-pointer"
          >
            <Plus className="w-3.5 h-3.5 text-zinc-400" />
            <span>Add Audio Here…</span>
          </button>

          <button
            type="button"
            onClick={() => {
              const cat = contextMenu.category;
              setContextMenu(null);
              onExportCategoryPack(cat);
            }}
            className="w-full px-2 py-1.5 rounded-md hover:bg-zinc-800 flex items-center gap-2 text-left cursor-pointer"
          >
            <Package className="w-3.5 h-3.5 text-zinc-400" />
            <span>Export Category Pack…</span>
          </button>

          {contextMenu.category.toLowerCase() !== 'general' && (
            <>
              <div className="my-1 border-t border-zinc-800/80" />
              <button
                type="button"
                onClick={() => {
                  const cat = contextMenu.category;
                  setContextMenu(null);
                  onDeleteCategory(cat);
                }}
                className="w-full px-2 py-1.5 rounded-md hover:bg-red-950/60 text-red-400 flex items-center gap-2 text-left cursor-pointer"
              >
                <Trash2 className="w-3.5 h-3.5" />
                <span>Delete Category</span>
              </button>
            </>
          )}
        </div>
      )}

      {/* Minimal Bottom Docked Footer */}
      <div className="px-3 py-1.5 border-t border-zinc-900 bg-zinc-950 flex items-center justify-between text-[10px] text-zinc-500 shrink-0">
        <span className="truncate">Click: Mic • ▷ Local • Right-click: Menu</span>
        <div className="flex items-center gap-2 shrink-0 font-mono text-[9.5px]">
          <span
            className={cn(
              'px-1.5 py-0.2 rounded border',
              isDocked
                ? 'border-zinc-800 bg-zinc-900/80 text-zinc-300'
                : 'border-zinc-800/70 text-zinc-500'
            )}
          >
            {isDocked ? 'DOCKED' : 'FLOAT'}
          </span>
          <span
            className="truncate max-w-[96px]"
            title={virtualMicStatus?.savedPhysicalMic || undefined}
          >
            {virtualMicStatus?.virtualMicIsDefault ? 'VMIC' : 'MANUAL'}
          </span>
        </div>
      </div>
    </div>
  );
};

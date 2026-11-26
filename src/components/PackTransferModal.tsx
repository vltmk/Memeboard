import React, { useState } from 'react';
import {
  Archive,
  Download,
  FolderUp,
  Layers,
  Package,
  Upload,
} from 'lucide-react';
import { PlaybackMode, SoundItem } from '../types';
import { Dialog, DialogContent, DialogDescription, DialogHeader, DialogTitle } from './ui/dialog';
import { Button } from './ui/button';
import { Input } from './ui/input';
import { cn } from '../lib/utils';

interface PackTransferModalProps {
  isOpen: boolean;
  sounds: SoundItem[];
  categories: string[];
  isBusy: boolean;
  onClose: () => void;
  onExportPack: (categoryFilter: string | null) => Promise<void>;
  onImportPack: () => Promise<void>;
  onBulkImportWithTemplate: (template: {
    category: string;
    emoji: string;
    playbackMode: PlaybackMode;
  }) => Promise<void>;
}

const QUICK_EMOJIS = ['🔊', '🔥', '😂', '🎙️', '⚡', '🎵', '💀', '📣'];

export const PackTransferModal: React.FC<PackTransferModalProps> = ({
  isOpen,
  sounds,
  categories,
  isBusy,
  onClose,
  onExportPack,
  onImportPack,
  onBulkImportWithTemplate,
}) => {
  const [exportCategory, setExportCategory] = useState<string>('All');
  const [tplCategory, setTplCategory] = useState<string>('General');
  const [tplEmoji, setTplEmoji] = useState<string>('🔊');
  const [tplMode, setTplMode] = useState<PlaybackMode>('restart');

  const selectableCategories = Array.from(
    new Set(['General', 'Voice', 'Memes', ...categories.filter((c) => c !== 'All')])
  );

  const exportCount =
    exportCategory === 'All'
      ? sounds.length
      : sounds.filter(
          (s) =>
            (s.category?.trim() || 'General').toLowerCase() ===
            exportCategory.toLowerCase()
        ).length;

  return (
    <Dialog open={isOpen} onOpenChange={(open) => !open && onClose()}>
      <DialogContent className="max-w-sm">
        <DialogHeader>
          <div className="flex items-center gap-2 pr-6">
            <Package className="w-4 h-4 text-zinc-200" />
            <div>
              <DialogTitle>Packs &amp; Bulk Templates</DialogTitle>
              <DialogDescription>
                Import or export audio files with Memeboard templates
              </DialogDescription>
            </div>
          </div>
        </DialogHeader>

        <div className="p-4 space-y-3.5 overflow-y-auto text-xs">
          {/* Export Pack */}
          <div className="p-3 rounded-lg bg-zinc-900 border border-zinc-800 space-y-2.5">
            <div className="flex items-center justify-between gap-2">
              <div className="flex items-center gap-1.5">
                <Download className="w-3.5 h-3.5 text-zinc-200" />
                <span className="text-[11px] font-semibold text-zinc-100">
                  Export Pack (.memeboard / .zip)
                </span>
              </div>
              <span className="text-[10px] font-mono text-zinc-400">
                {exportCount} clips
              </span>
            </div>

            <p className="text-[10px] text-zinc-400 leading-relaxed">
              Bundles audio files together with <span className="font-mono text-zinc-300">memeboard.json</span> templates
              (names, emojis, categories, trims, gain, hotkeys &amp; modes).
            </p>

            <div className="flex items-center gap-2">
              <select
                value={exportCategory}
                onChange={(e) => setExportCategory(e.target.value)}
                className="flex-1 h-8 bg-zinc-950 text-xs text-zinc-100 px-2.5 rounded-md border border-zinc-800 focus:border-zinc-500 outline-none"
              >
                <option value="All">All Categories ({sounds.length})</option>
                {categories
                  .filter((c) => c !== 'All')
                  .map((cat) => (
                    <option key={cat} value={cat}>
                      Category: {cat}
                    </option>
                  ))}
              </select>
              <Button
                type="button"
                variant="default"
                size="sm"
                disabled={isBusy || exportCount === 0}
                onClick={() =>
                  onExportPack(exportCategory === 'All' ? null : exportCategory)
                }
              >
                <Archive className="w-3.5 h-3.5" />
                <span>Export</span>
              </Button>
            </div>
          </div>

          {/* Import Pack */}
          <div className="p-3 rounded-lg bg-zinc-900 border border-zinc-800 space-y-2">
            <div className="flex items-center justify-between gap-2">
              <div className="flex items-center gap-1.5">
                <Upload className="w-3.5 h-3.5 text-zinc-200" />
                <span className="text-[11px] font-semibold text-zinc-100">
                  Import Pack (.memeboard / .zip)
                </span>
              </div>
              <Button
                type="button"
                variant="secondary"
                size="xs"
                disabled={isBusy}
                onClick={onImportPack}
              >
                <FolderUp className="w-3 h-3" /> Choose File
              </Button>
            </div>
            <p className="text-[10px] text-zinc-400 leading-relaxed">
              Restores full Memeboard templates from a pack, or imports every audio file inside any standard <span className="font-mono text-zinc-300">.zip</span> archive.
            </p>
          </div>

          {/* Bulk Audio Import with Template */}
          <div className="p-3 rounded-lg bg-zinc-900 border border-zinc-800 space-y-2.5">
            <div className="flex items-center gap-1.5">
              <Layers className="w-3.5 h-3.5 text-zinc-200" />
              <span className="text-[11px] font-semibold text-zinc-100">
                Bulk Import Audio with Template
              </span>
            </div>

            <div className="grid grid-cols-2 gap-2">
              <div className="space-y-1">
                <label className="text-[10px] text-zinc-400 uppercase tracking-wider">
                  Category
                </label>
                <Input
                  dir="auto"
                  list="pack-template-cats"
                  value={tplCategory}
                  onChange={(e) => setTplCategory(e.target.value)}
                  placeholder="General"
                  className="h-7 text-[11px]"
                />
                <datalist id="pack-template-cats">
                  {selectableCategories.map((c) => (
                    <option key={c} value={c} />
                  ))}
                </datalist>
              </div>

              <div className="space-y-1">
                <label className="text-[10px] text-zinc-400 uppercase tracking-wider">
                  Mode
                </label>
                <div className="grid grid-cols-3 gap-0.5 bg-zinc-950 border border-zinc-800 p-0.5 rounded-md h-7">
                  {(['restart', 'stop', 'overlap'] as PlaybackMode[]).map((mode) => (
                    <button
                      key={mode}
                      type="button"
                      onClick={() => setTplMode(mode)}
                      className={cn(
                        'rounded text-[10px] font-medium capitalize transition-colors cursor-pointer',
                        tplMode === mode
                          ? 'bg-zinc-100 text-zinc-950'
                          : 'text-zinc-400 hover:text-zinc-100'
                      )}
                    >
                      {mode}
                    </button>
                  ))}
                </div>
              </div>
            </div>

            <div className="flex items-center justify-between gap-2">
              <div className="flex items-center gap-1 flex-wrap">
                {QUICK_EMOJIS.map((em) => (
                  <button
                    key={em}
                    type="button"
                    onClick={() => setTplEmoji(em)}
                    className={cn(
                      'w-6 h-6 rounded flex items-center justify-center text-xs transition-colors cursor-pointer',
                      tplEmoji === em
                        ? 'bg-zinc-100 text-zinc-950'
                        : 'bg-zinc-950 hover:bg-zinc-800'
                    )}
                  >
                    {em}
                  </button>
                ))}
              </div>
              <Button
                type="button"
                variant="default"
                size="xs"
                disabled={isBusy}
                onClick={() =>
                  onBulkImportWithTemplate({
                    category: tplCategory.trim() || 'General',
                    emoji: tplEmoji.trim() || '🔊',
                    playbackMode: tplMode,
                  })
                }
              >
                Select Audio…
              </Button>
            </div>
          </div>
        </div>

        <div className="flex items-center justify-end px-4 py-3 bg-zinc-900/60 border-t border-zinc-800 shrink-0">
          <Button type="button" variant="ghost" size="sm" onClick={onClose}>
            Done
          </Button>
        </div>
      </DialogContent>
    </Dialog>
  );
};

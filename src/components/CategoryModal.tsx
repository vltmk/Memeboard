import React, { useEffect, useMemo, useState } from 'react';
import { Check, FolderPlus, PackageCheck, Search, Tag, Trash2 } from 'lucide-react';
import { SoundItem } from '../types';
import { Dialog, DialogContent, DialogDescription, DialogHeader, DialogTitle } from './ui/dialog';
import { Button } from './ui/button';
import { Input } from './ui/input';
import { cn } from '../lib/utils';

interface CategoryModalProps {
  isOpen: boolean;
  editingCategory: string | null; // null = create new category
  sounds: SoundItem[];
  existingCategories: string[];
  onClose: () => void;
  onSaveCategory: (
    oldName: string | null,
    newName: string,
    selectedSoundIds: string[]
  ) => void;
  onDeleteCategory?: (categoryName: string) => void;
  onExportCategory?: (categoryName: string) => void;
}

export const CategoryModal: React.FC<CategoryModalProps> = ({
  isOpen,
  editingCategory,
  sounds,
  existingCategories,
  onClose,
  onSaveCategory,
  onDeleteCategory,
  onExportCategory,
}) => {
  const [name, setName] = useState('');
  const [selectedIds, setSelectedIds] = useState<Set<string>>(new Set());
  const [filter, setFilter] = useState('');

  useEffect(() => {
    if (!isOpen) return;
    setFilter('');
    if (editingCategory) {
      setName(editingCategory);
      const initial = new Set(
        sounds
          .filter((s) => (s.category?.trim() || 'General').toLowerCase() === editingCategory.toLowerCase())
          .map((s) => s.id)
      );
      setSelectedIds(initial);
    } else {
      setName('');
      setSelectedIds(new Set());
    }
  }, [isOpen, editingCategory, sounds]);

  const filteredSounds = useMemo(() => {
    const q = filter.trim().toLowerCase();
    if (!q) return sounds;
    return sounds.filter(
      (s) =>
        s.name.toLowerCase().includes(q) ||
        (s.category || 'General').toLowerCase().includes(q)
    );
  }, [sounds, filter]);

  const toggleSound = (id: string) => {
    setSelectedIds((prev) => {
      const next = new Set(prev);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });
  };

  const trimmedName = name.trim();
  const isDuplicateNew =
    !editingCategory &&
    trimmedName.length > 0 &&
    existingCategories.some((c) => c.toLowerCase() === trimmedName.toLowerCase());
  const isAllReserved = trimmedName.toLowerCase() === 'all';
  const canSave = trimmedName.length > 0 && !isAllReserved && !isDuplicateNew;

  const handleSubmit = (e: React.FormEvent) => {
    e.preventDefault();
    if (!canSave) return;
    onSaveCategory(editingCategory, trimmedName, Array.from(selectedIds));
    onClose();
  };

  return (
    <Dialog open={isOpen} onOpenChange={(open) => !open && onClose()}>
      <DialogContent className="max-w-sm">
        <DialogHeader>
          <div className="flex items-center gap-2 pr-6">
            {editingCategory ? (
              <Tag className="w-4 h-4 text-zinc-200" />
            ) : (
              <FolderPlus className="w-4 h-4 text-zinc-200" />
            )}
            <div>
              <DialogTitle>
                {editingCategory ? 'Edit Category' : 'New Category'}
              </DialogTitle>
              <DialogDescription>
                Name the pill and choose which clips belong to it
              </DialogDescription>
            </div>
          </div>
        </DialogHeader>

        <form onSubmit={handleSubmit} className="flex flex-col flex-1 overflow-hidden">
          <div className="p-4 space-y-3 overflow-y-auto text-xs">
            <div className="space-y-1">
              <label className="text-[10px] font-medium text-zinc-400 uppercase tracking-wider">
                Category Name
              </label>
              <Input
                dir="auto"
                value={name}
                onChange={(e) => setName(e.target.value)}
                placeholder="e.g. Voice, Memes, Reactions"
                autoFocus
              />
              {isAllReserved && (
                <p className="text-[10px] text-amber-400">
                  &ldquo;All&rdquo; is reserved for the unfiltered view.
                </p>
              )}
              {isDuplicateNew && (
                <p className="text-[10px] text-amber-400">
                  This category already exists — right-click its pill to edit it.
                </p>
              )}
            </div>

            <div className="space-y-1.5">
              <div className="flex items-center justify-between">
                <label className="text-[10px] font-medium text-zinc-400 uppercase tracking-wider">
                  Clips in Category
                </label>
                <span className="text-[10px] font-mono text-zinc-300">
                  {selectedIds.size}/{sounds.length}
                </span>
              </div>

              {sounds.length > 0 ? (
                <>
                  <div className="relative">
                    <Search className="w-3 h-3 text-zinc-500 absolute left-2.5 top-1/2 -translate-y-1/2 pointer-events-none" />
                    <Input
                      dir="auto"
                      value={filter}
                      onChange={(e) => setFilter(e.target.value)}
                      placeholder="Filter clips..."
                      className="pl-7 h-7 text-[11px] bg-zinc-900"
                    />
                  </div>

                  <div className="max-h-52 overflow-y-auto rounded-lg border border-zinc-800 bg-zinc-900/70 divide-y divide-zinc-800/70">
                    {filteredSounds.map((sound) => {
                      const checked = selectedIds.has(sound.id);
                      const currentCat = sound.category?.trim() || 'General';
                      return (
                        <button
                          key={sound.id}
                          type="button"
                          onClick={() => toggleSound(sound.id)}
                          className={cn(
                            'w-full px-2.5 py-1.5 flex items-center justify-between gap-2 text-left transition-colors cursor-pointer',
                            checked
                              ? 'bg-zinc-800/80 text-zinc-50'
                              : 'hover:bg-zinc-800/40 text-zinc-300'
                          )}
                        >
                          <div className="flex items-center gap-2 min-w-0">
                            <span
                              className={cn(
                                'w-4 h-4 rounded border flex items-center justify-center shrink-0 transition-colors',
                                checked
                                  ? 'bg-zinc-100 border-zinc-100 text-zinc-950'
                                  : 'border-zinc-700 bg-zinc-950'
                              )}
                            >
                              {checked && <Check className="w-2.5 h-2.5 stroke-[3]" />}
                            </span>
                            <span className="text-sm leading-none shrink-0">
                              {sound.emoji || '🔊'}
                            </span>
                            <span dir="auto" className="text-[11px] font-medium truncate">
                              {sound.name}
                            </span>
                          </div>
                          <span
                            dir="auto"
                            className="text-[10px] font-mono text-zinc-500 shrink-0 max-w-[76px] truncate"
                          >
                            {currentCat}
                          </span>
                        </button>
                      );
                    })}
                    {filteredSounds.length === 0 && (
                      <div className="py-5 text-center text-[11px] text-zinc-500">
                        No clips match &ldquo;{filter}&rdquo;
                      </div>
                    )}
                  </div>
                </>
              ) : (
                <div className="p-3 rounded-lg border border-zinc-800 bg-zinc-900/60 text-[11px] text-zinc-400">
                  No clips added yet. Create the category first, then drop or import audio into it.
                </div>
              )}
            </div>
          </div>

          <div className="flex items-center justify-between gap-2 px-4 py-3 bg-zinc-900/60 border-t border-zinc-800 shrink-0">
            <div className="flex items-center gap-1">
              {editingCategory &&
                editingCategory.toLowerCase() !== 'general' &&
                onDeleteCategory && (
                  <Button
                    type="button"
                    variant="destructive"
                    size="sm"
                    onClick={() => {
                      onDeleteCategory(editingCategory);
                      onClose();
                    }}
                    title="Delete category (clips move to General)"
                  >
                    <Trash2 className="w-3.5 h-3.5" />
                  </Button>
                )}
              {editingCategory && selectedIds.size > 0 && onExportCategory && (
                <Button
                  type="button"
                  variant="outline"
                  size="sm"
                  onClick={() => onExportCategory(editingCategory)}
                  title="Export this category as a Memeboard pack"
                >
                  <PackageCheck className="w-3.5 h-3.5" /> Export
                </Button>
              )}
            </div>

            <div className="flex items-center gap-1.5">
              <Button type="button" variant="ghost" size="sm" onClick={onClose}>
                Cancel
              </Button>
              <Button type="submit" variant="default" size="sm" disabled={!canSave}>
                {editingCategory ? 'Save' : 'Create'}
              </Button>
            </div>
          </div>
        </form>
      </DialogContent>
    </Dialog>
  );
};

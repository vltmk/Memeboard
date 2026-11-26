import React, { useState, useEffect, useRef, useCallback } from 'react';
import {
  Play,
  Square,
  Keyboard,
  Volume2,
  Scissors,
  Trash2,
  RotateCcw,
  Tag,
  Headphones,
} from 'lucide-react';
import { SoundItem, PlaybackMode } from '../types';
import { drawWaveform } from '../utils/audio';
import { formatKeyboardCombo } from '../utils/hotkeys';
import { api } from '../services/api';
import { Dialog, DialogContent, DialogHeader, DialogTitle, DialogDescription } from './ui/dialog';
import { Button } from './ui/button';
import { Input } from './ui/input';
import { Slider } from './ui/slider';
import { cn } from '../lib/utils';

interface SoundEditorModalProps {
  sound: SoundItem | null;
  existingCategories: string[];
  onClose: () => void;
  onSave: (updated: SoundItem) => void;
  onDelete: (sound: SoundItem) => void;
}

const EMOJI_PRESETS = [
  '🔊', '💥', '😂', '🦆', '📣',
  '🎵', '🔥', '💀', '🤯', '🎉',
  '👏', '🤡', '🚀', '😱', '⚡',
];

const CATEGORY_PRESETS = ['General', 'Memes', 'Gaming', 'Reactions', 'Music', 'Voice'];
const MIN_TRIM_GAP = 0.05;

export const SoundEditorModal: React.FC<SoundEditorModalProps> = ({
  sound,
  existingCategories,
  onClose,
  onSave,
  onDelete,
}) => {
  const [name, setName] = useState('');
  const [emoji, setEmoji] = useState('🔊');
  const [category, setCategory] = useState('General');
  const [volume, setVolume] = useState(1.0);
  const [hotkey, setHotkey] = useState('');
  const [trimStart, setTrimStart] = useState(0);
  const [trimEnd, setTrimEnd] = useState(1);
  const [playbackMode, setPlaybackMode] = useState<PlaybackMode>('restart');
  const [isRecordingHotkey, setIsRecordingHotkey] = useState(false);

  const [isPlayingPreview, setIsPlayingPreview] = useState(false);
  const [previewTime, setPreviewTime] = useState(0);
  const animFrameRef = useRef<number | null>(null);
  const startTimeRef = useRef<number>(0);
  const playOffsetRef = useRef<number>(0);
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const draggingHandleRef = useRef<'start' | 'end' | null>(null);
  const drawParamsRef = useRef({ trimStart: 0, trimEnd: 1, previewTime: 0 });
  drawParamsRef.current = { trimStart, trimEnd, previewTime };

  const allCategories = Array.from(new Set([...CATEGORY_PRESETS, ...existingCategories])).filter(Boolean);

  useEffect(() => {
    if (sound) {
      setName(sound.name);
      setEmoji(sound.emoji);
      setCategory(sound.category || 'General');
      setVolume(sound.volume);
      setHotkey(sound.hotkey || '');
      setTrimStart(sound.trimStart);
      setTrimEnd(sound.trimEnd > sound.trimStart ? sound.trimEnd : sound.duration);
      setPlaybackMode(sound.playbackMode);
      setPreviewTime(sound.trimStart);
      setIsPlayingPreview(false);
      setIsRecordingHotkey(false);
    }
  }, [sound]);

  useEffect(() => {
    const canvas = canvasRef.current;
    if (!canvas || !sound) return;

    const resize = () => {
      const rect = canvas.getBoundingClientRect();
      if (rect.width > 0 && rect.height > 0) {
        const dpr = window.devicePixelRatio || 1;
        const targetW = Math.round(rect.width * dpr);
        const targetH = Math.round(rect.height * dpr);
        if (canvas.width !== targetW || canvas.height !== targetH) {
          canvas.width = targetW;
          canvas.height = targetH;
        }
        const { trimStart: ts, trimEnd: te, previewTime: pt } = drawParamsRef.current;
        drawWaveform(canvas, sound.peaks, ts, te, sound.duration, pt);
      }
    };

    resize();
    const ro = new ResizeObserver(resize);
    ro.observe(canvas);
    return () => ro.disconnect();
  }, [sound]);

  useEffect(() => {
    if (canvasRef.current && sound) {
      drawWaveform(canvasRef.current, sound.peaks, trimStart, trimEnd, sound.duration, previewTime);
    }
  }, [sound, trimStart, trimEnd, previewTime]);

  const stopPreview = useCallback(() => {
    setIsPlayingPreview(false);
    if (animFrameRef.current) {
      cancelAnimationFrame(animFrameRef.current);
      animFrameRef.current = null;
    }
    if (sound) {
      api.stopPreview(sound.id).catch(() => {});
    }
    setPreviewTime(trimStart);
  }, [sound, trimStart]);

  const startPreviewFrom = useCallback(
    (seekTime: number) => {
      if (!sound) return;
      if (animFrameRef.current) {
        cancelAnimationFrame(animFrameRef.current);
        animFrameRef.current = null;
      }
      const startAt = Math.max(trimStart, Math.min(seekTime, trimEnd - 0.02));
      setIsPlayingPreview(true);
      setPreviewTime(startAt);
      playOffsetRef.current = startAt;
      startTimeRef.current = performance.now();

      api.previewSound(sound.id, trimStart, trimEnd, volume, startAt).catch(() => {});

      const loop = (now: number) => {
        const elapsed = (now - startTimeRef.current) / 1000;
        const current = playOffsetRef.current + elapsed;
        if (current >= trimEnd) {
          stopPreview();
        } else {
          setPreviewTime(current);
          animFrameRef.current = requestAnimationFrame(loop);
        }
      };
      animFrameRef.current = requestAnimationFrame(loop);
    },
    [sound, trimStart, trimEnd, volume, stopPreview]
  );

  const togglePreview = () => {
    if (isPlayingPreview) {
      stopPreview();
    } else {
      startPreviewFrom(previewTime >= trimStart && previewTime < trimEnd - 0.03 ? previewTime : trimStart);
    }
  };

  useEffect(() => {
    return () => {
      if (animFrameRef.current) {
        cancelAnimationFrame(animFrameRef.current);
      }
      if (sound) {
        api.stopPreview(sound.id).catch(() => {});
      }
      api.setHotkeyRecording(false).catch(() => {});
    };
  }, [sound]);

  useEffect(() => {
    api.setHotkeyRecording(isRecordingHotkey).catch(() => {});
    if (!isRecordingHotkey) return;

    const handleKeyDown = (e: KeyboardEvent) => {
      e.preventDefault();
      e.stopPropagation();

      if (e.key === 'Escape') {
        setIsRecordingHotkey(false);
        return;
      }
      if (e.key === 'Backspace' || e.key === 'Delete') {
        setHotkey('');
        setIsRecordingHotkey(false);
        return;
      }

      const combo = formatKeyboardCombo(e);
      if (combo) {
        setHotkey(combo);
        setIsRecordingHotkey(false);
      }
    };

    window.addEventListener('keydown', handleKeyDown, true);
    return () => {
      window.removeEventListener('keydown', handleKeyDown, true);
    };
  }, [isRecordingHotkey]);

  if (!sound) return null;

  const getPointerTime = (clientX: number) => {
    if (!canvasRef.current) return 0;
    const rect = canvasRef.current.getBoundingClientRect();
    if (rect.width <= 0) return 0;
    const x = Math.max(0, Math.min(clientX - rect.left, rect.width));
    return (x / rect.width) * sound.duration;
  };

  const handleCanvasPointerDown = (e: React.PointerEvent<HTMLCanvasElement>) => {
    if (!canvasRef.current || sound.duration <= 0) return;
    const rect = canvasRef.current.getBoundingClientRect();
    const clickX = e.clientX - rect.left;
    const startPx = (trimStart / sound.duration) * rect.width;
    const endPx = (trimEnd / sound.duration) * rect.width;

    if (Math.abs(clickX - startPx) <= 14) {
      draggingHandleRef.current = 'start';
      e.currentTarget.setPointerCapture(e.pointerId);
    } else if (Math.abs(clickX - endPx) <= 14) {
      draggingHandleRef.current = 'end';
      e.currentTarget.setPointerCapture(e.pointerId);
    } else {
      const clickTime = Math.max(trimStart, Math.min(getPointerTime(e.clientX), trimEnd - 0.02));
      setPreviewTime(clickTime);
      if (isPlayingPreview) {
        startPreviewFrom(clickTime);
      }
    }
  };

  const handleCanvasPointerMove = (e: React.PointerEvent<HTMLCanvasElement>) => {
    if (!draggingHandleRef.current || sound.duration <= 0) return;
    const t = getPointerTime(e.clientX);
    if (draggingHandleRef.current === 'start') {
      const next = Math.max(0, Math.min(t, trimEnd - MIN_TRIM_GAP));
      setTrimStart(next);
      if (!isPlayingPreview) setPreviewTime(next);
    } else if (draggingHandleRef.current === 'end') {
      const next = Math.min(sound.duration, Math.max(t, trimStart + MIN_TRIM_GAP));
      setTrimEnd(next);
    }
  };

  const handleCanvasPointerUp = () => {
    draggingHandleRef.current = null;
  };

  const handleSave = () => {
    stopPreview();
    onSave({
      ...sound,
      name: name.trim() || sound.name,
      emoji: emoji.trim() || '🔊',
      category: category.trim() || 'General',
      volume,
      hotkey: hotkey.trim() || null,
      trimStart: Math.max(0, trimStart),
      trimEnd: Math.max(trimStart + MIN_TRIM_GAP, Math.min(trimEnd, sound.duration)),
      playbackMode,
    });
    onClose();
  };

  return (
    <Dialog
      open={Boolean(sound)}
      onOpenChange={(open) => {
        if (!open) {
          stopPreview();
          onClose();
        }
      }}
    >
      <DialogContent className="max-w-md">
        <DialogHeader>
          <div className="flex items-center gap-2.5 pr-6">
            <span className="w-7 h-7 rounded-md bg-zinc-900 border border-zinc-800 flex items-center justify-center text-base">
              {emoji}
            </span>
            <div>
              <DialogTitle>Edit Sound</DialogTitle>
              <DialogDescription>Trim waveform, icon, volume &amp; hotkey</DialogDescription>
            </div>
          </div>
        </DialogHeader>

        <div className="p-4 space-y-4 overflow-y-auto text-xs">
          {/* Name + Category */}
          <div className="grid grid-cols-2 gap-2.5">
            <div className="space-y-1">
              <label className="text-[10px] font-medium text-zinc-400 uppercase tracking-wider">
                Name
              </label>
              <Input
                value={name}
                onChange={(e) => setName(e.target.value)}
                placeholder="Clip name"
              />
            </div>

            <div className="space-y-1">
              <label className="text-[10px] font-medium text-zinc-400 uppercase tracking-wider flex items-center gap-1">
                <Tag className="w-2.5 h-2.5" /> Category
              </label>
              <Input
                list="memeboard-categories"
                value={category}
                onChange={(e) => setCategory(e.target.value)}
                placeholder="General"
              />
              <datalist id="memeboard-categories">
                {allCategories.map((c) => (
                  <option key={c} value={c} />
                ))}
              </datalist>
            </div>
          </div>

          {/* Emoji Picker */}
          <div className="space-y-1.5">
            <div className="flex items-center justify-between">
              <label className="text-[10px] font-medium text-zinc-400 uppercase tracking-wider">
                Emoji Icon
              </label>
              <Input
                value={emoji}
                onChange={(e) => setEmoji(e.target.value.slice(0, 4))}
                className="w-12 h-6 text-center px-1"
                title="Custom emoji"
              />
            </div>
            <div className="flex flex-wrap gap-1 bg-zinc-900 border border-zinc-800 p-1.5 rounded-lg">
              {EMOJI_PRESETS.map((e) => (
                <button
                  key={e}
                  type="button"
                  onClick={() => setEmoji(e)}
                  className={cn(
                    'w-6 h-6 rounded flex items-center justify-center text-sm transition-colors cursor-pointer',
                    emoji === e ? 'bg-zinc-100 text-zinc-950' : 'hover:bg-zinc-800'
                  )}
                >
                  {e}
                </button>
              ))}
            </div>
          </div>

          {/* Waveform Preview & Trim */}
          <div className="space-y-1.5">
            <div className="flex items-center justify-between">
              <label className="text-[10px] font-medium text-zinc-400 uppercase tracking-wider flex items-center gap-1">
                <Scissors className="w-3 h-3" /> Waveform Preview
              </label>
              <div className="flex items-center gap-2">
                <span className="text-[10px] font-mono text-zinc-400">
                  {trimStart.toFixed(2)}s – {trimEnd.toFixed(2)}s
                </span>
                <button
                  type="button"
                  onClick={() => {
                    setTrimStart(0);
                    setTrimEnd(sound.duration);
                    setPreviewTime(0);
                  }}
                  className="text-[10px] text-zinc-400 hover:text-zinc-100 flex items-center gap-0.5 cursor-pointer"
                >
                  <RotateCcw className="w-2.5 h-2.5" /> Reset
                </button>
              </div>
            </div>

            <div className="bg-zinc-900 rounded-lg p-1.5 border border-zinc-800">
              <canvas
                ref={canvasRef}
                onPointerDown={handleCanvasPointerDown}
                onPointerMove={handleCanvasPointerMove}
                onPointerUp={handleCanvasPointerUp}
                onPointerCancel={handleCanvasPointerUp}
                className="w-full h-20 rounded cursor-ew-resize touch-none block"
              />
            </div>

            <div className="flex items-center justify-between pt-0.5">
              <Button
                type="button"
                variant={isPlayingPreview ? 'secondary' : 'default'}
                size="xs"
                onClick={togglePreview}
              >
                {isPlayingPreview ? (
                  <>
                    <Square className="w-2.5 h-2.5 fill-current" /> Stop
                  </>
                ) : (
                  <>
                    <Play className="w-2.5 h-2.5 fill-current" /> Preview
                  </>
                )}
              </Button>
              <span className="text-[10px] text-zinc-400 flex items-center gap-1">
                <Headphones className="w-3 h-3" /> Headphones only
              </span>
            </div>
          </div>

          {/* Volume */}
          <div className="space-y-1.5">
            <div className="flex items-center justify-between">
              <label className="text-[10px] font-medium text-zinc-400 uppercase tracking-wider flex items-center gap-1">
                <Volume2 className="w-3 h-3" /> Clip Gain
              </label>
              <span className="text-[11px] font-mono text-zinc-200">{Math.round(volume * 100)}%</span>
            </div>
            <Slider
              min={0}
              max={2}
              step={0.02}
              value={[volume]}
              onValueChange={([v]) => setVolume(v)}
            />
          </div>

          {/* Hotkey & Playback Mode */}
          <div className="grid grid-cols-2 gap-2.5">
            <div className="space-y-1">
              <label className="text-[10px] font-medium text-zinc-400 uppercase tracking-wider flex items-center gap-1">
                <Keyboard className="w-3 h-3" /> Hotkey
              </label>
              <div className="flex gap-1">
                <button
                  type="button"
                  onClick={() => setIsRecordingHotkey(true)}
                  className={cn(
                    'flex-1 h-8 px-2.5 rounded-md text-[11px] font-mono text-left border transition-colors cursor-pointer truncate',
                    isRecordingHotkey
                      ? 'bg-zinc-100 text-zinc-950 border-white'
                      : 'bg-zinc-900 border-zinc-800 text-zinc-200 hover:border-zinc-700'
                  )}
                >
                  {isRecordingHotkey ? 'Press keys...' : hotkey || 'Bind key'}
                </button>
                {hotkey && (
                  <Button
                    type="button"
                    variant="outline"
                    size="icon"
                    className="h-8 w-8"
                    onClick={() => setHotkey('')}
                    title="Clear Hotkey"
                  >
                    ✕
                  </Button>
                )}
              </div>
            </div>

            <div className="space-y-1">
              <label className="text-[10px] font-medium text-zinc-400 uppercase tracking-wider">
                Mode
              </label>
              <div className="grid grid-cols-3 gap-0.5 bg-zinc-900 border border-zinc-800 p-0.5 rounded-md h-8">
                {(['restart', 'stop', 'overlap'] as PlaybackMode[]).map((mode) => (
                  <button
                    key={mode}
                    type="button"
                    onClick={() => setPlaybackMode(mode)}
                    className={cn(
                      'rounded text-[10px] font-medium capitalize transition-colors cursor-pointer',
                      playbackMode === mode
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
        </div>

        {/* Footer */}
        <div className="flex items-center justify-between px-4 py-3 bg-zinc-900/60 border-t border-zinc-800 shrink-0">
          <Button
            type="button"
            variant="destructive"
            size="sm"
            onClick={() => {
              stopPreview();
              onDelete(sound);
              onClose();
            }}
          >
            <Trash2 className="w-3.5 h-3.5" /> Delete
          </Button>

          <div className="flex items-center gap-1.5">
            <Button
              type="button"
              variant="ghost"
              size="sm"
              onClick={() => {
                stopPreview();
                onClose();
              }}
            >
              Cancel
            </Button>
            <Button type="button" variant="default" size="sm" onClick={handleSave}>
              Save
            </Button>
          </div>
        </div>
      </DialogContent>
    </Dialog>
  );
};

import React, { useState, useEffect, useRef, useCallback } from 'react';
import {
  X,
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
import { api } from '../services/api';

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

function formatKeyboardCombo(e: KeyboardEvent): string | null {
  if (['Control', 'Shift', 'Alt', 'Meta'].includes(e.key)) {
    return null;
  }

  const parts: string[] = [];
  if (e.ctrlKey) parts.push('Control');
  if (e.shiftKey) parts.push('Shift');
  if (e.altKey) parts.push('Alt');
  if (e.metaKey) parts.push('Super');

  let key = e.key;
  if (e.code.startsWith('Numpad')) {
    const sub = e.code.slice(6);
    if (/^[0-9]$/.test(sub) || ['Add', 'Subtract', 'Multiply', 'Divide', 'Decimal', 'Enter'].includes(sub)) {
      key = e.code;
    }
  } else if (e.code.startsWith('Digit')) {
    key = e.code.slice(5);
  } else if (e.code.startsWith('Key')) {
    key = e.code.slice(3).toUpperCase();
  } else if (key.startsWith('Arrow')) {
    key = key.replace('Arrow', '');
  } else if (key === ' ') {
    key = 'Space';
  } else if (key.length === 1) {
    key = key.toUpperCase();
  }

  parts.push(key);
  return parts.join('+');
}

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

  // Size canvas cleanly with ResizeObserver instead of reallocating on every animation frame
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
        drawWaveform(canvas, sound.peaks, trimStart, trimEnd, sound.duration, previewTime);
      }
    };

    resize();
    const ro = new ResizeObserver(resize);
    ro.observe(canvas);
    return () => ro.disconnect();
  }, [sound, trimStart, trimEnd, previewTime]);

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

      api.previewSound(sound.id, startAt, trimEnd, volume).catch(() => {});

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

  // Pause global Raw Input hotkeys while recording keybind
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
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/75 backdrop-blur-sm p-4 select-none animate-in fade-in">
      <div className="bg-[#313338] border border-[#232428] w-full max-w-xl rounded-2xl shadow-2xl overflow-hidden flex flex-col max-h-[92vh]">
        {/* Modal Header */}
        <div className="flex items-center justify-between px-6 py-4 border-b border-[#232428] shrink-0">
          <div className="flex items-center gap-3">
            <div className="w-10 h-10 rounded-xl bg-[#232428] flex items-center justify-center text-2xl shadow-inner">
              {emoji}
            </div>
            <div>
              <h2 className="text-base font-bold text-white">Edit Sound Clip</h2>
              <p className="text-xs text-[#949ba4]">Trim waveform, organize category &amp; bind global hotkey</p>
            </div>
          </div>
          <button
            type="button"
            onClick={() => {
              stopPreview();
              onClose();
            }}
            className="p-1.5 rounded-lg text-[#949ba4] hover:text-white hover:bg-[#35373c] transition-colors"
          >
            <X className="w-5 h-5" />
          </button>
        </div>

        {/* Body */}
        <div className="p-6 space-y-5 overflow-y-auto">
          {/* Name + Category + Emoji */}
          <div className="grid grid-cols-1 sm:grid-cols-12 gap-4">
            <div className="sm:col-span-6 space-y-1.5">
              <label className="text-xs font-semibold text-[#b5bac1] uppercase tracking-wider">
                Sound Name
              </label>
              <input
                type="text"
                value={name}
                onChange={(e) => setName(e.target.value)}
                placeholder="e.g. Vine Boom"
                className="w-full bg-[#1e1f22] text-sm text-white px-3 py-2 rounded-lg border border-transparent focus:border-[#5865f2] outline-none transition-colors"
              />
            </div>

            <div className="sm:col-span-6 space-y-1.5">
              <label className="text-xs font-semibold text-[#b5bac1] uppercase tracking-wider flex items-center gap-1">
                <Tag className="w-3 h-3" /> Category
              </label>
              <input
                type="text"
                list="memeboard-categories"
                value={category}
                onChange={(e) => setCategory(e.target.value)}
                placeholder="General"
                className="w-full bg-[#1e1f22] text-sm text-white px-3 py-2 rounded-lg border border-transparent focus:border-[#5865f2] outline-none transition-colors"
              />
              <datalist id="memeboard-categories">
                {allCategories.map((c) => (
                  <option key={c} value={c} />
                ))}
              </datalist>
            </div>
          </div>

          {/* Emoji Picker (shows all 15 presets + custom input) */}
          <div className="space-y-2">
            <div className="flex items-center justify-between">
              <label className="text-xs font-semibold text-[#b5bac1] uppercase tracking-wider">
                Icon / Emoji
              </label>
              <input
                type="text"
                value={emoji}
                onChange={(e) => setEmoji(e.target.value.slice(0, 4))}
                title="Or paste any custom emoji"
                className="w-16 text-center bg-[#1e1f22] text-sm text-white px-2 py-1 rounded-md border border-[#35373c] focus:border-[#5865f2] outline-none"
              />
            </div>
            <div className="flex flex-wrap items-center gap-1.5 bg-[#1e1f22] p-2 rounded-lg">
              {EMOJI_PRESETS.map((e) => (
                <button
                  key={e}
                  type="button"
                  onClick={() => setEmoji(e)}
                  className={`w-8 h-8 rounded-md flex items-center justify-center text-base transition-all ${
                    emoji === e
                      ? 'bg-[#5865f2] scale-110 shadow'
                      : 'hover:bg-[#35373c]'
                  }`}
                >
                  {e}
                </button>
              ))}
            </div>
          </div>

          {/* Waveform & Trim Section */}
          <div className="space-y-2">
            <div className="flex items-center justify-between">
              <label className="text-xs font-semibold text-[#b5bac1] uppercase tracking-wider flex items-center gap-1.5">
                <Scissors className="w-3.5 h-3.5 text-[#5865f2]" /> Waveform Trim &amp; Seek
              </label>
              <div className="flex items-center gap-3">
                <span className="text-xs font-mono text-[#949ba4]">
                  {trimStart.toFixed(2)}s – {trimEnd.toFixed(2)}s ({(trimEnd - trimStart).toFixed(2)}s)
                </span>
                <button
                  type="button"
                  onClick={() => {
                    setTrimStart(0);
                    setTrimEnd(sound.duration);
                    setPreviewTime(0);
                  }}
                  title="Reset trim to full clip"
                  className="text-[11px] font-medium text-[#b5bac1] hover:text-white flex items-center gap-1"
                >
                  <RotateCcw className="w-3 h-3" /> Reset
                </button>
              </div>
            </div>

            <div className="relative bg-[#1e1f22] rounded-xl p-2 border border-[#232428]">
              <canvas
                ref={canvasRef}
                onPointerDown={handleCanvasPointerDown}
                onPointerMove={handleCanvasPointerMove}
                onPointerUp={handleCanvasPointerUp}
                onPointerCancel={handleCanvasPointerUp}
                className="w-full h-24 rounded-lg cursor-ew-resize touch-none block"
              />
            </div>

            <div className="flex items-center justify-between gap-3 pt-1">
              <button
                type="button"
                onClick={togglePreview}
                className={`flex items-center gap-2 px-4 py-1.5 rounded-lg text-xs font-semibold transition-all ${
                  isPlayingPreview
                    ? 'bg-[#f23f42] hover:bg-[#d83538] text-white'
                    : 'bg-[#5865f2] hover:bg-[#4752c4] text-white'
                }`}
              >
                {isPlayingPreview ? (
                  <>
                    <Square className="w-3.5 h-3.5 fill-current" /> Stop Preview
                  </>
                ) : (
                  <>
                    <Play className="w-3.5 h-3.5 fill-current" /> Preview Headphones
                  </>
                )}
              </button>

              <span className="text-[11px] text-[#23a55a] flex items-center gap-1 font-medium">
                <Headphones className="w-3.5 h-3.5" /> Monitor-only (not sent to mic/Discord)
              </span>
            </div>
          </div>

          {/* Volume Slider */}
          <div className="space-y-1.5">
            <div className="flex items-center justify-between">
              <label className="text-xs font-semibold text-[#b5bac1] uppercase tracking-wider flex items-center gap-1.5">
                <Volume2 className="w-3.5 h-3.5" /> Clip Volume Gain
              </label>
              <span className="text-xs font-mono text-white">{Math.round(volume * 100)}%</span>
            </div>
            <input
              type="range"
              min="0"
              max="2"
              step="0.02"
              value={volume}
              onChange={(e) => setVolume(parseFloat(e.target.value))}
              className="w-full h-2 bg-[#1e1f22] rounded-lg appearance-none cursor-pointer accent-[#5865f2]"
            />
          </div>

          {/* Hotkey & Playback Mode */}
          <div className="grid grid-cols-1 sm:grid-cols-2 gap-4">
            <div className="space-y-1.5">
              <label className="text-xs font-semibold text-[#b5bac1] uppercase tracking-wider flex items-center gap-1.5">
                <Keyboard className="w-3.5 h-3.5" /> Global Hotkey (Pass-Through)
              </label>
              <div className="flex gap-1.5">
                <button
                  type="button"
                  onClick={() => setIsRecordingHotkey(true)}
                  className={`flex-1 py-2 px-3 rounded-lg text-xs font-mono text-left border transition-all ${
                    isRecordingHotkey
                      ? 'bg-[#5865f2]/20 border-[#5865f2] text-white animate-pulse'
                      : 'bg-[#1e1f22] border-transparent text-[#dbdee1] hover:border-[#35373c]'
                  }`}
                >
                  {isRecordingHotkey
                    ? 'Press any key combo...'
                    : hotkey || 'Click to bind hotkey'}
                </button>
                {hotkey && (
                  <button
                    type="button"
                    onClick={() => setHotkey('')}
                    className="px-2.5 rounded-lg bg-[#1e1f22] text-[#949ba4] hover:text-white text-xs"
                    title="Clear Hotkey"
                  >
                    ✕
                  </button>
                )}
              </div>
            </div>

            <div className="space-y-1.5">
              <label className="text-xs font-semibold text-[#b5bac1] uppercase tracking-wider">
                Trigger Behavior
              </label>
              <div className="grid grid-cols-3 gap-1 bg-[#1e1f22] p-1 rounded-lg">
                {(['restart', 'stop', 'overlap'] as PlaybackMode[]).map((mode) => (
                  <button
                    key={mode}
                    type="button"
                    onClick={() => setPlaybackMode(mode)}
                    className={`py-1.5 rounded-md text-[11px] font-semibold capitalize transition-colors ${
                      playbackMode === mode
                        ? 'bg-[#5865f2] text-white'
                        : 'text-[#949ba4] hover:text-white'
                    }`}
                  >
                    {mode}
                  </button>
                ))}
              </div>
            </div>
          </div>
        </div>

        {/* Footer */}
        <div className="flex items-center justify-between px-6 py-4 bg-[#2b2d31] border-t border-[#232428] shrink-0">
          <button
            type="button"
            onClick={() => {
              stopPreview();
              onDelete(sound);
              onClose();
            }}
            className="flex items-center gap-1.5 px-3 py-2 rounded-lg text-xs font-semibold text-[#f23f42] hover:bg-[#f23f42]/10 transition-colors"
          >
            <Trash2 className="w-4 h-4" /> Delete Sound
          </button>

          <div className="flex items-center gap-2">
            <button
              type="button"
              onClick={() => {
                stopPreview();
                onClose();
              }}
              className="px-4 py-2 rounded-lg text-xs font-semibold text-[#dbdee1] hover:bg-[#35373c] transition-colors"
            >
              Cancel
            </button>
            <button
              type="button"
              onClick={handleSave}
              className="px-5 py-2 rounded-lg bg-[#23a55a] hover:bg-[#1e8e4c] text-white text-xs font-semibold shadow-md transition-colors"
            >
              Save Changes
            </button>
          </div>
        </div>
      </div>
    </div>
  );
};

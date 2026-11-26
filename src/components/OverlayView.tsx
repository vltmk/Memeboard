import React, { useState, useEffect, useRef, useCallback } from 'react';
import { emit, listen } from '@tauri-apps/api/event';
import { Square, X, GripHorizontal, Radio } from 'lucide-react';
import { SoundItem } from '../types';
import { api, isTauriEnvironment } from '../services/api';
import { SoundPill } from './SoundPill';
import { Button } from './ui/button';

export const OverlayView: React.FC = () => {
  const [sounds, setSounds] = useState<SoundItem[]>([]);
  const [playingIds, setPlayingIds] = useState<string[]>([]);
  const [localPreviewId, setLocalPreviewId] = useState<string | null>(null);
  const [isVisible, setIsVisible] = useState<boolean>(true);
  const previewTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);

  const loadSounds = useCallback(async () => {
    try {
      const state = await api.loadSoundboardState();
      setSounds(state.sounds);
    } catch (e) {
      console.error('Overlay load error:', e);
    }
  }, []);

  useEffect(() => {
    loadSounds();
    if (!isTauriEnvironment()) return;

    api
      .getOverlayVisibility()
      .then((visible) => setIsVisible(visible))
      .catch(() => {});

    const unlistenPromise = Promise.all([
      listen('soundboard-updated', () => {
        loadSounds();
      }),
      listen<boolean>('overlay-visibility', (event) => {
        setIsVisible(Boolean(event.payload));
        if (event.payload) {
          loadSounds();
        }
      }),
      listen('soundboard-panic', () => {
        setPlayingIds([]);
        setLocalPreviewId(null);
      }),
    ]);

    return () => {
      if (previewTimerRef.current) clearTimeout(previewTimerRef.current);
      unlistenPromise.then((unlisteners) => unlisteners.forEach((fn) => fn())).catch(() => {});
    };
  }, [loadSounds]);

  useEffect(() => {
    if (!isVisible) return;

    let cancelled = false;
    const poll = async () => {
      try {
        const status = await api.getPlaybackStatus();
        if (!cancelled) {
          setPlayingIds(status.playingSoundIds);
        }
      } catch {
        // ignore transient poll errors
      }
    };

    poll();
    const interval = setInterval(poll, 140);
    return () => {
      cancelled = true;
      clearInterval(interval);
    };
  }, [isVisible]);

  const handleLocalPreview = (sound: SoundItem) => {
    if (previewTimerRef.current) {
      clearTimeout(previewTimerRef.current);
      previewTimerRef.current = null;
    }
    if (localPreviewId === sound.id) {
      api.stopPreview(sound.id).catch(() => {});
      setLocalPreviewId(null);
      return;
    }
    if (localPreviewId) {
      api.stopPreview(localPreviewId).catch(() => {});
    }
    const start = Math.max(0, sound.trimStart || 0);
    const end = sound.trimEnd > start ? sound.trimEnd : sound.duration;
    setLocalPreviewId(sound.id);
    api.previewSound(sound.id, start, end, sound.volume, start).catch(() => {});
    const ms = Math.max(120, Math.ceil((end - start) * 1000) + 60);
    previewTimerRef.current = setTimeout(() => {
      setLocalPreviewId((cur) => (cur === sound.id ? null : cur));
    }, ms);
  };

  const handleEditFromOverlay = (sound: SoundItem) => {
    emit('open-sound-editor', sound.id).catch(() => {});
  };

  return (
    <div className="w-screen h-screen bg-zinc-950/95 backdrop-blur-md border border-zinc-800 rounded-xl flex flex-col overflow-hidden shadow-2xl select-none">
      {/* Compact Monochromatic Drag Header */}
      <div
        data-tauri-drag-region
        className="flex items-center justify-between px-3 py-2 bg-zinc-900/90 border-b border-zinc-800 cursor-grab active:cursor-grabbing shrink-0"
      >
        <div className="flex items-center gap-1.5 pointer-events-none">
          <GripHorizontal className="w-3 h-3 text-zinc-500" />
          <Radio className="w-3 h-3 text-zinc-200" />
          <span className="text-[11px] font-semibold text-zinc-100 tracking-tight">
            Memeboard
          </span>
        </div>

        <div className="flex items-center gap-1">
          <Button
            type="button"
            variant="ghost"
            size="xs"
            onClick={() => api.stopAllSounds().catch(() => {})}
            title="Stop All Sounds"
            className="h-6 px-2 text-[10px] text-zinc-300"
          >
            <Square className="w-2.5 h-2.5 fill-current" /> Stop
          </Button>
          <Button
            type="button"
            variant="ghost"
            size="icon"
            onClick={() => api.toggleOverlayWindow().catch(() => {})}
            title="Hide Overlay"
            className="h-6 w-6"
          >
            <X className="w-3 h-3" />
          </Button>
        </div>
      </div>

      {/* Dense Pills */}
      <div className="flex-1 overflow-y-auto p-2.5 flex flex-wrap gap-1.5 content-start">
        {sounds.map((sound) => (
          <SoundPill
            key={sound.id}
            compact
            sound={sound}
            isPlaying={playingIds.includes(sound.id)}
            isLocalPreviewing={localPreviewId === sound.id}
            onBroadcast={(s) => api.playSound(s.id).catch(() => {})}
            onStopBroadcast={(s) => api.stopSound(s.id).catch(() => {})}
            onLocalPreview={handleLocalPreview}
            onEdit={handleEditFromOverlay}
          />
        ))}

        {sounds.length === 0 && (
          <div className="w-full py-10 text-center text-[11px] text-zinc-500">
            No sounds yet. Drop clips into Memeboard.
          </div>
        )}
      </div>

      <div className="px-2.5 py-1 border-t border-zinc-900 text-[9px] text-zinc-500 flex justify-between shrink-0">
        <span>Click: Mic • ▷ Local • Right-click: Edit</span>
      </div>
    </div>
  );
};

import React, { useState, useEffect } from 'react';
import { listen } from '@tauri-apps/api/event';
import { Square, X, Layers, GripHorizontal } from 'lucide-react';
import { SoundItem } from '../types';
import { api, isTauriEnvironment } from '../services/api';

export const OverlayView: React.FC = () => {
  const [sounds, setSounds] = useState<SoundItem[]>([]);
  const [playingIds, setPlayingIds] = useState<string[]>([]);
  const [isVisible, setIsVisible] = useState<boolean>(false);

  const loadSounds = async () => {
    try {
      const state = await api.loadSoundboardState();
      setSounds(state.sounds);
    } catch (e) {
      console.error('Overlay load error:', e);
    }
  };

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
      }),
    ]);

    return () => {
      unlistenPromise.then((unlisteners) => unlisteners.forEach((fn) => fn())).catch(() => {});
    };
  }, []);

  // Poll playback status ONLY while overlay is actually visible
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

  return (
    <div className="w-screen h-screen bg-[#1e1f22]/94 backdrop-blur-md border border-[#35373c] rounded-2xl flex flex-col overflow-hidden shadow-2xl select-none">
      {/* Draggable Titlebar via Tauri drag region */}
      <div
        data-tauri-drag-region
        className="flex items-center justify-between px-3.5 py-2.5 bg-[#2b2d31]/95 border-b border-[#35373c]/60 cursor-grab active:cursor-grabbing shrink-0"
      >
        <div className="flex items-center gap-2 pointer-events-none">
          <GripHorizontal className="w-3.5 h-3.5 text-[#72767d]" />
          <Layers className="w-3.5 h-3.5 text-[#5865f2]" />
          <span className="text-xs font-bold text-white tracking-wide">
            Memeboard Overlay
          </span>
        </div>

        <div className="flex items-center gap-1.5">
          <button
            type="button"
            onClick={() => api.stopAllSounds().catch(() => {})}
            title="Stop All Sounds"
            className="flex items-center gap-1 px-2 py-1 rounded bg-[#f23f42]/20 hover:bg-[#f23f42] text-[#f23f42] hover:text-white text-[10px] font-bold transition-colors"
          >
            <Square className="w-2.5 h-2.5 fill-current" /> Stop
          </button>
          <button
            type="button"
            onClick={() => api.toggleOverlayWindow().catch(() => {})}
            title="Hide Overlay"
            className="p-1 rounded hover:bg-[#35373c] text-[#949ba4] hover:text-white transition-colors"
          >
            <X className="w-3.5 h-3.5" />
          </button>
        </div>
      </div>

      {/* Sound Tiles */}
      <div className="flex-1 overflow-y-auto p-3 grid grid-cols-3 gap-2 content-start">
        {sounds.map((sound) => {
          const isPlaying = playingIds.includes(sound.id);
          return (
            <button
              key={sound.id}
              type="button"
              onClick={() =>
                (isPlaying ? api.stopSound(sound.id) : api.playSound(sound.id)).catch(() => {})
              }
              className={`flex flex-col items-center justify-center p-2.5 rounded-xl border text-center transition-all ${
                isPlaying
                  ? 'bg-[#5865f2]/25 border-[#5865f2] text-white shadow-md shadow-[#5865f2]/20'
                  : 'bg-[#2b2d31]/80 hover:bg-[#35373c] border-[#35373c]/40 text-[#dbdee1]'
              }`}
            >
              <span className="text-xl mb-1">{sound.emoji}</span>
              <span className="text-[11px] font-semibold leading-tight truncate w-full">
                {sound.name}
              </span>
              {sound.hotkey && (
                <span className="mt-1 px-1 py-0.5 rounded bg-[#1e1f22] text-[9px] font-mono text-[#949ba4] truncate max-w-full">
                  {sound.hotkey}
                </span>
              )}
            </button>
          );
        })}

        {sounds.length === 0 && (
          <div className="col-span-3 py-12 text-center text-xs text-[#949ba4]">
            No sounds added yet. Drag audio clips into Memeboard to fill your overlay.
          </div>
        )}
      </div>
    </div>
  );
};

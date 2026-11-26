import React, { useState, useEffect, useCallback, useRef } from 'react';
import { listen } from '@tauri-apps/api/event';
import { getCurrentWebview } from '@tauri-apps/api/webview';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { Header } from './components/Header';
import { SoundGrid } from './components/SoundGrid';
import { SoundEditorModal } from './components/SoundEditorModal';
import { AudioSettingsModal } from './components/AudioSettingsModal';
import { OverlayView } from './components/OverlayView';
import {
  AudioDevicesResult,
  AudioPlaybackStatus,
  AudioSettings,
  SoundItem,
  SoundboardState,
  VirtualMicStatus,
} from './types';
import { api, isTauriEnvironment } from './services/api';

const AUDIO_EXTENSIONS = ['mp3', 'wav', 'ogg', 'flac', 'm4a', 'aac', 'opus', 'webm'];

const DEFAULT_SETTINGS: AudioSettings = {
  schemaVersion: 1,
  micDeviceId: null,
  broadcastDeviceId: null,
  monitorDeviceId: null,
  micVolume: 1.0,
  monitorVolume: 0.8,
  broadcastVolume: 1.0,
  monitorMuted: false,
  broadcastMuted: false,
  voiceDuckingEnabled: true,
  voiceDuckingDb: -10.0,
  loudnessTargetLufs: -14.0,
  panicHotkey: 'Control+Shift+X',
  overlayHotkey: 'Control+Shift+S',
  autoSwitchDefaultMic: true,
  aecSafeMonitor: true,
};

const DEFAULT_PLAYBACK_STATUS: AudioPlaybackStatus = {
  playingSoundIds: [],
  activeClipCount: 0,
  micActive: false,
  micStreamActive: false,
  broadcastStreamActive: false,
  monitorStreamActive: false,
  lastAudioError: null,
};

function hasSupportedAudioExt(filePath: string): boolean {
  const dot = filePath.lastIndexOf('.');
  if (dot < 0) return false;
  return AUDIO_EXTENSIONS.includes(filePath.slice(dot + 1).toLowerCase());
}

export function App() {
  const isOverlayMode =
    typeof window !== 'undefined' &&
    new URLSearchParams(window.location.search).get('overlay') === 'true';

  if (isOverlayMode) {
    return <OverlayView />;
  }

  return <MainApp />;
}

const MainApp: React.FC = () => {
  const [boardState, setBoardState] = useState<SoundboardState>({
    sounds: [],
    settings: DEFAULT_SETTINGS,
  });
  const [audioDevices, setAudioDevices] = useState<AudioDevicesResult>({
    inputDevices: [],
    outputDevices: [],
    currentInput: null,
    currentBroadcast: null,
    currentMonitor: null,
  });
  const [playbackStatus, setPlaybackStatus] = useState<AudioPlaybackStatus>(DEFAULT_PLAYBACK_STATUS);
  const [virtualMicStatus, setVirtualMicStatus] = useState<VirtualMicStatus | null>(null);
  const [isInstallingDriver, setIsInstallingDriver] = useState(false);

  const [editingSound, setEditingSound] = useState<SoundItem | null>(null);
  const [isSettingsOpen, setIsSettingsOpen] = useState(false);
  const [isDraggingFile, setIsDraggingFile] = useState(false);
  const [localPreviewId, setLocalPreviewId] = useState<string | null>(null);
  const [toastMsg, setToastMsg] = useState<string | null>(null);

  const boardStateRef = useRef(boardState);
  boardStateRef.current = boardState;
  const volumeSaveTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const previewTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);

  const showToast = useCallback((msg: string) => {
    setToastMsg(msg);
    setTimeout(() => {
      setToastMsg((prev) => (prev === msg ? null : prev));
    }, 3200);
  }, []);

  const refreshDevicesAndVirtualMic = useCallback(async () => {
    if (!isTauriEnvironment()) return;
    try {
      const [devs, vm] = await Promise.all([
        api.getAudioDevices(),
        api.getVirtualMicStatus(),
      ]);
      setAudioDevices(devs);
      setVirtualMicStatus(vm);
    } catch (e) {
      console.error('Device refresh error:', e);
    }
  }, []);

  useEffect(() => {
    if (!isTauriEnvironment()) return;

    (async () => {
      try {
        const [loaded, devs, vm] = await Promise.all([
          api.loadSoundboardState(),
          api.getAudioDevices(),
          api.getVirtualMicStatus(),
        ]);
        setBoardState(loaded);
        setAudioDevices(devs);
        setVirtualMicStatus(vm);
      } catch (err) {
        console.error('Initialization failed:', err);
      }
    })();

    const unlistenPromise = Promise.all([
      listen<VirtualMicStatus>('virtual-mic-updated', (event) => {
        setVirtualMicStatus(event.payload);
        api.getAudioDevices().then(setAudioDevices).catch(() => {});
      }),
      listen('soundboard-updated', async () => {
        try {
          const fresh = await api.loadSoundboardState();
          setBoardState(fresh);
        } catch {
          // ignore
        }
      }),
      listen('soundboard-panic', () => {
        setPlaybackStatus((prev) => ({ ...prev, playingSoundIds: [], activeClipCount: 0 }));
        setLocalPreviewId(null);
      }),
      listen<string>('open-sound-editor', (event) => {
        const target = boardStateRef.current.sounds.find((s) => s.id === event.payload);
        if (target) {
          setEditingSound(target);
          const win = getCurrentWindow();
          win.unminimize().catch(() => {});
          win.show().catch(() => {});
          win.setFocus().catch(() => {});
        }
      }),
    ]);

    return () => {
      if (previewTimerRef.current) clearTimeout(previewTimerRef.current);
      unlistenPromise.then((fns) => fns.forEach((fn) => fn())).catch(() => {});
    };
  }, []);

  useEffect(() => {
    if (!isTauriEnvironment()) return;

    let interval: ReturnType<typeof setInterval> | null = null;
    const poll = async () => {
      try {
        const status = await api.getPlaybackStatus();
        setPlaybackStatus(status);
      } catch {
        // ignore
      }
    };

    const syncPolling = () => {
      if (document.hidden) {
        if (interval) {
          clearInterval(interval);
          interval = null;
        }
      } else {
        poll();
        if (!interval) {
          interval = setInterval(poll, 140);
        }
      }
    };

    syncPolling();
    document.addEventListener('visibilitychange', syncPolling);
    return () => {
      document.removeEventListener('visibilitychange', syncPolling);
      if (interval) clearInterval(interval);
    };
  }, []);

  const persistState = useCallback(async (nextState: SoundboardState) => {
    if (volumeSaveTimerRef.current) {
      clearTimeout(volumeSaveTimerRef.current);
      volumeSaveTimerRef.current = null;
    }
    boardStateRef.current = nextState;
    setBoardState(nextState);
    if (!isTauriEnvironment()) return;
    try {
      await api.saveSoundboardState(nextState);
      const canonical = await api.loadSoundboardState();
      setBoardState(canonical);
    } catch (e) {
      showToast(`Failed to save state: ${e}`);
    }
  }, [showToast]);

  const handleImportPaths = useCallback(
    async (paths: string[]) => {
      const validPaths = paths.filter(hasSupportedAudioExt);
      if (validPaths.length === 0) {
        if (paths.length > 0) {
          showToast('Unsupported format. Use MP3, WAV, OGG, FLAC, M4A, or Opus.');
        }
        return;
      }

      const imported: SoundItem[] = [];
      for (const filePath of validPaths) {
        try {
          const item = await api.importSoundFile(filePath);
          imported.push(item);
        } catch (err) {
          showToast(`Import failed: ${err}`);
        }
      }

      if (imported.length > 0) {
        const current = boardStateRef.current;
        await persistState({
          ...current,
          sounds: [...imported, ...current.sounds],
        });
        showToast(
          imported.length === 1
            ? `Added "${imported[0].name}"`
            : `Added ${imported.length} clips`
        );
      }
    },
    [persistState, showToast]
  );

  useEffect(() => {
    if (!isTauriEnvironment()) return;

    let cancelled = false;
    let unlisten: (() => void) | null = null;

    getCurrentWebview()
      .onDragDropEvent((event) => {
        if (cancelled) return;
        if (event.payload.type === 'enter' || event.payload.type === 'over') {
          setIsDraggingFile(true);
        } else if (event.payload.type === 'leave') {
          setIsDraggingFile(false);
        } else if (event.payload.type === 'drop') {
          setIsDraggingFile(false);
          handleImportPaths(event.payload.paths);
        }
      })
      .then((fn) => {
        if (cancelled) fn();
        else unlisten = fn;
      })
      .catch(() => {});

    return () => {
      cancelled = true;
      if (unlisten) unlisten();
    };
  }, [handleImportPaths]);

  const handleAddSoundClick = async () => {
    try {
      const picked = await api.pickAudioFile();
      if (picked && picked.length > 0) {
        await handleImportPaths(picked);
      }
    } catch (e) {
      showToast(`Dialog error: ${e}`);
    }
  };

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
    api.previewSound(sound.id, start, end, sound.volume, start).catch((e) => showToast(`${e}`));
    const ms = Math.max(120, Math.ceil((end - start) * 1000) + 60);
    previewTimerRef.current = setTimeout(() => {
      setLocalPreviewId((cur) => (cur === sound.id ? null : cur));
    }, ms);
  };

  const handleSaveSound = (updated: SoundItem) => {
    const current = boardStateRef.current;
    persistState({
      ...current,
      sounds: current.sounds.map((s) => (s.id === updated.id ? updated : s)),
    });
  };

  const handleDeleteSound = async (sound: SoundItem) => {
    try {
      await api.deleteSound(sound.id, sound.filePath);
      setBoardState((prev) => ({
        ...prev,
        sounds: prev.sounds.filter((s) => s.id !== sound.id),
      }));
      showToast(`Deleted "${sound.name}"`);
    } catch (e) {
      showToast(`Failed to delete: ${e}`);
    }
  };

  const handleSaveSettings = async (newSettings: AudioSettings) => {
    const prevSettings = boardStateRef.current.settings;
    const devicesChanged =
      prevSettings.micDeviceId !== newSettings.micDeviceId ||
      prevSettings.broadcastDeviceId !== newSettings.broadcastDeviceId ||
      prevSettings.monitorDeviceId !== newSettings.monitorDeviceId;

    if (devicesChanged) {
      try {
        await api.setAudioDevices(
          newSettings.micDeviceId,
          newSettings.broadcastDeviceId,
          newSettings.monitorDeviceId
        );
      } catch (e) {
        showToast(`Device warning: ${e}`);
      }
    }

    await persistState({
      ...boardStateRef.current,
      settings: newSettings,
    });
    await refreshDevicesAndVirtualMic();
    try {
      const status = await api.getPlaybackStatus();
      setPlaybackStatus(status);
      if (devicesChanged && status.lastAudioError) {
        showToast(`Audio warning: ${status.lastAudioError}`);
        return;
      }
    } catch {
      // ignore
    }
    showToast('Settings applied');
  };

  const scheduleHeaderSettingsSave = (nextSettings: AudioSettings) => {
    const updated = { ...boardStateRef.current, settings: nextSettings };
    boardStateRef.current = updated;
    setBoardState(updated);
    api
      .setVolumes(
        nextSettings.micVolume,
        nextSettings.monitorVolume,
        nextSettings.broadcastVolume,
        nextSettings.monitorMuted,
        nextSettings.broadcastMuted
      )
      .catch(() => {});

    if (volumeSaveTimerRef.current) {
      clearTimeout(volumeSaveTimerRef.current);
    }
    volumeSaveTimerRef.current = setTimeout(() => {
      volumeSaveTimerRef.current = null;
      api.saveSoundboardState(boardStateRef.current).catch(() => {});
    }, 250);
  };

  const handleInstallVirtualDriver = async () => {
    setIsInstallingDriver(true);
    try {
      const status = await api.installVirtualDriver();
      setVirtualMicStatus(status);
      await refreshDevicesAndVirtualMic();
      showToast(status.virtualDeviceAttached ? 'Virtual Mic is live' : status.statusMessage);
    } catch (e) {
      showToast(`Setup failed: ${e}`);
    } finally {
      setIsInstallingDriver(false);
    }
  };

  const handleSetAutoSwitchMic = async (enabled: boolean) => {
    try {
      const status = await api.setAutoSwitchMic(enabled);
      setVirtualMicStatus(status);
      setBoardState((prev) => ({
        ...prev,
        settings: { ...prev.settings, autoSwitchDefaultMic: enabled },
      }));
      showToast(enabled ? 'Default mic: Virtual Mic' : 'Restored physical mic');
    } catch (e) {
      showToast(`Could not switch mic: ${e}`);
    }
  };

  const existingCategories = Array.from(
    new Set(boardState.sounds.map((s) => s.category).filter(Boolean))
  );

  return (
    <div className="h-screen w-screen flex flex-col bg-zinc-950 text-zinc-50 overflow-hidden select-none">
      <Header
        settings={boardState.settings}
        playbackStatus={playbackStatus}
        virtualMicStatus={virtualMicStatus}
        onAddSoundClick={handleAddSoundClick}
        onOpenSettings={() => {
          refreshDevicesAndVirtualMic();
          setIsSettingsOpen(true);
        }}
        onStopAll={() => {
          setLocalPreviewId(null);
          api.stopAllSounds().catch(() => {});
        }}
        onToggleOverlay={() => api.toggleOverlayWindow().catch(() => {})}
        onDockBottomRight={() => api.dockMainWindow().catch(() => {})}
        onToggleMonitorMute={() =>
          scheduleHeaderSettingsSave({
            ...boardState.settings,
            monitorMuted: !boardState.settings.monitorMuted,
          })
        }
        onMonitorVolumeChange={(val) =>
          scheduleHeaderSettingsSave({
            ...boardState.settings,
            monitorVolume: val,
            monitorMuted: false,
          })
        }
        onToggleBroadcastMute={() =>
          scheduleHeaderSettingsSave({
            ...boardState.settings,
            broadcastMuted: !boardState.settings.broadcastMuted,
          })
        }
        onBroadcastVolumeChange={(val) =>
          scheduleHeaderSettingsSave({
            ...boardState.settings,
            broadcastVolume: val,
            broadcastMuted: false,
          })
        }
      />

      <SoundGrid
        sounds={boardState.sounds}
        playingIds={playbackStatus.playingSoundIds}
        localPreviewId={localPreviewId}
        isDraggingFile={isDraggingFile}
        virtualMicStatus={virtualMicStatus}
        broadcastStreamActive={playbackStatus.broadcastStreamActive}
        onBroadcastSound={(s) => api.playSound(s.id).catch((e) => showToast(`${e}`))}
        onStopBroadcast={(s) => api.stopSound(s.id).catch(() => {})}
        onLocalPreview={handleLocalPreview}
        onEditSound={(sound) => setEditingSound(sound)}
        onAddSoundClick={handleAddSoundClick}
        onOpenSettings={() => {
          refreshDevicesAndVirtualMic();
          setIsSettingsOpen(true);
        }}
        onInstallVirtualDriver={handleInstallVirtualDriver}
        isInstallingDriver={isInstallingDriver}
      />

      <SoundEditorModal
        sound={editingSound}
        existingCategories={existingCategories}
        onClose={() => setEditingSound(null)}
        onSave={handleSaveSound}
        onDelete={handleDeleteSound}
      />

      <AudioSettingsModal
        isOpen={isSettingsOpen}
        settings={boardState.settings}
        devices={audioDevices}
        virtualMicStatus={virtualMicStatus}
        onClose={() => setIsSettingsOpen(false)}
        onSaveSettings={handleSaveSettings}
        onRefreshDevices={refreshDevicesAndVirtualMic}
        onInstallVirtualDriver={handleInstallVirtualDriver}
        onSetAutoSwitchMic={handleSetAutoSwitchMic}
        onRestoreOriginalMic={() => handleSetAutoSwitchMic(false)}
        isInstallingDriver={isInstallingDriver}
      />

      {toastMsg && (
        <div className="fixed bottom-9 right-3 z-50 px-3 py-1.5 rounded-md bg-zinc-900 border border-zinc-700 text-[11px] font-medium text-zinc-100 shadow-lg animate-in fade-in max-w-[300px]">
          {toastMsg}
        </div>
      )}
    </div>
  );
};

export default App;

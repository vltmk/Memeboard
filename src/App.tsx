import React, { useState, useEffect, useCallback, useRef } from 'react';
import { listen } from '@tauri-apps/api/event';
import { getCurrentWebview } from '@tauri-apps/api/webview';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { Header } from './components/Header';
import { SoundGrid } from './components/SoundGrid';
import { SoundEditorModal } from './components/SoundEditorModal';
import { AudioSettingsModal } from './components/AudioSettingsModal';
import { CategoryModal } from './components/CategoryModal';
import { PackTransferModal } from './components/PackTransferModal';
import { OverlayView } from './components/OverlayView';
import {
  AudioDevicesResult,
  AudioPlaybackStatus,
  AudioSettings,
  PlaybackMode,
  PlatformInfo,
  SoundItem,
  SoundboardState,
  VirtualMicStatus,
} from './types';
import { api, isTauriEnvironment } from './services/api';
import { cn } from './lib/utils';

const AUDIO_EXTENSIONS = ['mp3', 'wav', 'ogg', 'flac', 'm4a', 'aac', 'opus', 'webm'];
const PACK_EXTENSIONS = ['memeboard', 'zip'];

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
  loudnessTargetLufs: -16.0,
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

function getExt(filePath: string): string {
  const dot = filePath.lastIndexOf('.');
  if (dot < 0) return '';
  return filePath.slice(dot + 1).toLowerCase();
}

function hasSupportedAudioExt(filePath: string): boolean {
  return AUDIO_EXTENSIONS.includes(getExt(filePath));
}

function isPackFile(filePath: string): boolean {
  return PACK_EXTENSIONS.includes(getExt(filePath));
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
    categories: [],
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
  const [platformInfo, setPlatformInfo] = useState<PlatformInfo>({
    os: 'windows',
    sessionType: 'win32',
  });
  const [isInstallingDriver, setIsInstallingDriver] = useState(false);

  const [editingSound, setEditingSound] = useState<SoundItem | null>(null);
  const [isSettingsOpen, setIsSettingsOpen] = useState(false);
  const [isPacksOpen, setIsPacksOpen] = useState(false);
  const [isPackBusy, setIsPackBusy] = useState(false);
  const [categoryModalState, setCategoryModalState] = useState<{
    open: boolean;
    editing: string | null;
  }>({ open: false, editing: null });

  const [isDraggingFile, setIsDraggingFile] = useState(false);
  const [localPreviewId, setLocalPreviewId] = useState<string | null>(null);
  const [toastMsg, setToastMsg] = useState<string | null>(null);
  const [isDocked, setIsDocked] = useState(true);
  const [dockAnimKey, setDockAnimKey] = useState(0);

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
        const [loaded, devs, vm, docked, plat] = await Promise.all([
          api.loadSoundboardState(),
          api.getAudioDevices(),
          api.getVirtualMicStatus(),
          api.isMainDocked().catch(() => true),
          api.getPlatformInfo().catch(() => ({ os: 'windows', sessionType: 'win32' })),
        ]);
        setBoardState({ ...loaded, categories: loaded.categories ?? [] });
        setAudioDevices(devs);
        setVirtualMicStatus(vm);
        setIsDocked(docked);
        setPlatformInfo(plat);
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
          setBoardState({ ...fresh, categories: fresh.categories ?? [] });
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
      listen<boolean>('main-dock-changed', (event) => {
        setIsDocked(Boolean(event.payload));
      }),
      listen('main-docked', () => {
        setIsDocked(true);
        setDockAnimKey((k) => k + 1);
      }),
      listen('main-shown', () => {
        setDockAnimKey((k) => k + 1);
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
      setBoardState({ ...canonical, categories: canonical.categories ?? [] });
    } catch (e) {
      showToast(`Failed to save state: ${e}`);
    }
  }, [showToast]);

  const handleImportPackPath = useCallback(
    async (packPath: string) => {
      setIsPackBusy(true);
      try {
        const res = await api.importPack(packPath);
        const fresh = await api.loadSoundboardState();
        setBoardState({ ...fresh, categories: fresh.categories ?? [] });
        const count = res.importedSounds.length;
        if (count === 0 && res.warnings.length > 0) {
          showToast(res.warnings[0]);
        } else {
          showToast(
            res.skippedCount > 0
              ? `Imported ${count} clips (${res.skippedCount} skipped)`
              : `Imported ${count} clip${count === 1 ? '' : 's'} from pack`
          );
        }
      } catch (e) {
        showToast(`Pack import failed: ${e}`);
      } finally {
        setIsPackBusy(false);
      }
    },
    [showToast]
  );

  const handleImportPaths = useCallback(
    async (
      paths: string[],
      template?: { category?: string; emoji?: string; playbackMode?: PlaybackMode }
    ) => {
      const packPaths = paths.filter(isPackFile);
      const validPaths = paths.filter(hasSupportedAudioExt);

      for (const p of packPaths) {
        await handleImportPackPath(p);
      }

      if (validPaths.length === 0) {
        if (paths.length > 0 && packPaths.length === 0) {
          showToast('Unsupported format. Use MP3, WAV, OGG, FLAC, M4A, Opus, or .memeboard.');
        }
        return;
      }

      const imported: SoundItem[] = [];
      for (const filePath of validPaths) {
        try {
          const item = await api.importSoundFile(
            filePath,
            undefined,
            template?.emoji,
            template?.category
          );
          if (template?.playbackMode) {
            item.playbackMode = template.playbackMode;
          }
          imported.push(item);
        } catch (err) {
          showToast(`Import failed: ${err}`);
        }
      }

      if (imported.length > 0) {
        const current = boardStateRef.current;
        const nextCategories = Array.from(
          new Set([
            ...(current.categories ?? []),
            ...imported.map((i) => i.category).filter(Boolean),
          ])
        );
        await persistState({
          ...current,
          sounds: [...imported, ...current.sounds],
          categories: nextCategories,
        });
        showToast(
          imported.length === 1
            ? `Added "${imported[0].name}"`
            : `Added ${imported.length} clips`
        );
      }
    },
    [persistState, showToast, handleImportPackPath]
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

  const handleAddSoundClick = async (targetCategory?: string) => {
    try {
      const picked = await api.pickAudioFile();
      if (picked && picked.length > 0) {
        await handleImportPaths(
          picked,
          targetCategory ? { category: targetCategory } : undefined
        );
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
    const nextCats = Array.from(
      new Set([...(current.categories ?? []), updated.category].filter(Boolean))
    );
    persistState({
      ...current,
      sounds: current.sounds.map((s) => (s.id === updated.id ? updated : s)),
      categories: nextCats,
    });
  };

  const handleMoveSoundCategory = (sound: SoundItem, category: string) => {
    const clean = category.trim() || 'General';
    if ((sound.category || 'General').toLowerCase() === clean.toLowerCase()) return;
    handleSaveSound({ ...sound, category: clean });
    showToast(`Moved "${sound.name}" to ${clean}`);
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

  const handleDownloadSound = async (sound: SoundItem) => {
    try {
      const ext = getExt(sound.filePath) || 'wav';
      const destPath = await api.pickExportAudioPath(sound.name, ext);
      if (!destPath) return;
      await api.exportSoundFile(sound.id, destPath);
      showToast(`Saved "${sound.name}.${ext}"`);
    } catch (e) {
      showToast(`Download failed: ${e}`);
    }
  };

  const handleExportPack = async (categoryFilter: string | null) => {
    setIsPackBusy(true);
    try {
      const defaultName = categoryFilter
        ? `memeboard-${categoryFilter.toLowerCase()}`
        : 'memeboard-pack';
      const destPath = await api.pickExportPackPath(defaultName);
      if (!destPath) return;
      const count = await api.exportPack(destPath, categoryFilter);
      showToast(`Exported ${count} clip${count === 1 ? '' : 's'} with templates`);
    } catch (e) {
      showToast(`Export failed: ${e}`);
    } finally {
      setIsPackBusy(false);
    }
  };

  const handleImportPackDialog = async () => {
    try {
      const packPath = await api.pickImportPackPath();
      if (!packPath) return;
      await handleImportPackPath(packPath);
      setIsPacksOpen(false);
    } catch (e) {
      showToast(`Pack picker error: ${e}`);
    }
  };

  const handleBulkImportWithTemplate = async (template: {
    category: string;
    emoji: string;
    playbackMode: PlaybackMode;
  }) => {
    try {
      const picked = await api.pickAudioFile();
      if (picked && picked.length > 0) {
        await handleImportPaths(picked, template);
        setIsPacksOpen(false);
      }
    } catch (e) {
      showToast(`Bulk import error: ${e}`);
    }
  };

  const handleSaveCategory = async (
    oldName: string | null,
    newName: string,
    selectedSoundIds: string[]
  ) => {
    const current = boardStateRef.current;
    const selectedSet = new Set(selectedSoundIds);

    const updatedSounds = current.sounds.map((s) => {
      const curCat = s.category?.trim() || 'General';
      if (selectedSet.has(s.id)) {
        return { ...s, category: newName };
      }
      if (oldName && curCat.toLowerCase() === oldName.toLowerCase()) {
        return { ...s, category: 'General' };
      }
      return s;
    });

    const catList = (current.categories ?? []).filter(
      (c) =>
        (!oldName || c.toLowerCase() !== oldName.toLowerCase()) &&
        c.toLowerCase() !== newName.toLowerCase()
    );
    catList.push(newName);

    await persistState({
      ...current,
      sounds: updatedSounds,
      categories: catList,
    });
    showToast(oldName ? `Updated "${newName}"` : `Created "${newName}"`);
  };

  const handleDeleteCategory = async (categoryName: string) => {
    if (categoryName.toLowerCase() === 'general') return;
    const current = boardStateRef.current;
    const updatedSounds = current.sounds.map((s) =>
      (s.category?.trim() || 'General').toLowerCase() === categoryName.toLowerCase()
        ? { ...s, category: 'General' }
        : s
    );
    const nextCategories = (current.categories ?? []).filter(
      (c) => c.toLowerCase() !== categoryName.toLowerCase()
    );
    await persistState({
      ...current,
      sounds: updatedSounds,
      categories: nextCategories,
    });
    showToast(`Deleted "${categoryName}"`);
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
    new Set([
      ...(boardState.categories ?? []),
      ...boardState.sounds.map((s) => s.category).filter(Boolean),
    ])
  );

  return (
    <div
      key={dockAnimKey}
      className={cn(
        'h-screen w-screen flex flex-col bg-zinc-950 text-zinc-50 overflow-hidden select-none animate-dock-in',
        isDocked
          ? 'border-t border-l border-zinc-800/95'
          : 'border border-zinc-800'
      )}
    >
      <Header
        settings={boardState.settings}
        playbackStatus={playbackStatus}
        virtualMicStatus={virtualMicStatus}
        isDocked={isDocked}
        onOpenPacks={() => setIsPacksOpen(true)}
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
        onMinimizeWindow={() => {
          if (isTauriEnvironment()) getCurrentWindow().minimize().catch(() => {});
        }}
        onHideToTray={() => {
          if (isTauriEnvironment()) getCurrentWindow().hide().catch(() => {});
        }}
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
        customCategories={boardState.categories ?? []}
        playingIds={playbackStatus.playingSoundIds}
        localPreviewId={localPreviewId}
        isDraggingFile={isDraggingFile}
        isDocked={isDocked}
        virtualMicStatus={virtualMicStatus}
        broadcastStreamActive={playbackStatus.broadcastStreamActive}
        onBroadcastSound={(s) => api.playSound(s.id).catch((e) => showToast(`${e}`))}
        onStopBroadcast={(s) => api.stopSound(s.id).catch(() => {})}
        onLocalPreview={handleLocalPreview}
        onEditSound={(sound) => setEditingSound(sound)}
        onDownloadSound={handleDownloadSound}
        onDeleteSound={handleDeleteSound}
        onMoveSoundCategory={handleMoveSoundCategory}
        onAddSoundClick={handleAddSoundClick}
        onCreateCategory={() => setCategoryModalState({ open: true, editing: null })}
        onEditCategory={(cat) => setCategoryModalState({ open: true, editing: cat })}
        onDeleteCategory={handleDeleteCategory}
        onExportCategoryPack={(cat) => handleExportPack(cat)}
        onOpenPacks={() => setIsPacksOpen(true)}
        onOpenSettings={() => {
          refreshDevicesAndVirtualMic();
          setIsSettingsOpen(true);
        }}
        onInstallVirtualDriver={handleInstallVirtualDriver}
        isInstallingDriver={isInstallingDriver}
        platformInfo={platformInfo}
      />

      <SoundEditorModal
        sound={editingSound}
        existingCategories={existingCategories}
        onClose={() => setEditingSound(null)}
        onSave={handleSaveSound}
        onDelete={handleDeleteSound}
        onDownload={handleDownloadSound}
      />

      <CategoryModal
        isOpen={categoryModalState.open}
        editingCategory={categoryModalState.editing}
        sounds={boardState.sounds}
        existingCategories={existingCategories}
        onClose={() => setCategoryModalState({ open: false, editing: null })}
        onSaveCategory={handleSaveCategory}
        onDeleteCategory={handleDeleteCategory}
        onExportCategory={(cat) => handleExportPack(cat)}
      />

      <PackTransferModal
        isOpen={isPacksOpen}
        sounds={boardState.sounds}
        categories={existingCategories}
        isBusy={isPackBusy}
        onClose={() => setIsPacksOpen(false)}
        onExportPack={handleExportPack}
        onImportPack={handleImportPackDialog}
        onBulkImportWithTemplate={handleBulkImportWithTemplate}
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
        platformInfo={platformInfo}
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

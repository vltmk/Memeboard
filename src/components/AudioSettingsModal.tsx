import React, { useEffect, useRef, useState } from 'react';
import {
  Mic,
  Headphones,
  Radio,
  ShieldAlert,
  Sliders,
  Volume2,
  Keyboard,
  RefreshCw,
  CheckCircle2,
  AlertTriangle,
  Wrench,
  RotateCcw,
} from 'lucide-react';
import { AudioDevicesResult, AudioSettings, VirtualMicStatus } from '../types';
import { api } from '../services/api';
import { formatKeyboardCombo } from '../utils/hotkeys';
import { Dialog, DialogContent, DialogHeader, DialogTitle, DialogDescription } from './ui/dialog';
import { Tabs, TabsList, TabsTrigger, TabsContent } from './ui/tabs';
import { Button } from './ui/button';
import { Slider } from './ui/slider';
import { Switch } from './ui/switch';
import { cn } from '../lib/utils';

interface AudioSettingsModalProps {
  isOpen: boolean;
  settings: AudioSettings;
  devices: AudioDevicesResult;
  virtualMicStatus: VirtualMicStatus | null;
  onClose: () => void;
  onSaveSettings: (updated: AudioSettings) => void;
  onRefreshDevices: () => void;
  onInstallVirtualDriver: () => void;
  onSetAutoSwitchMic: (enabled: boolean) => void;
  onRestoreOriginalMic: () => void;
  isInstallingDriver: boolean;
}

export const AudioSettingsModal: React.FC<AudioSettingsModalProps> = ({
  isOpen,
  settings,
  devices,
  virtualMicStatus,
  onClose,
  onSaveSettings,
  onRefreshDevices,
  onInstallVirtualDriver,
  onSetAutoSwitchMic,
  onRestoreOriginalMic,
  isInstallingDriver,
}) => {
  const [localSettings, setLocalSettings] = useState<AudioSettings>(settings);
  const [activeTab, setActiveTab] = useState('routing');
  const [recordingField, setRecordingField] = useState<'panic' | 'overlay' | null>(null);
  const wasOpenRef = useRef(false);

  useEffect(() => {
    if (isOpen && !wasOpenRef.current) {
      setLocalSettings(settings);
      setRecordingField(null);
    }
    if (!isOpen && wasOpenRef.current) {
      api.setHotkeyRecording(false).catch(() => {});
    }
    wasOpenRef.current = isOpen;
  }, [isOpen, settings]);

  useEffect(() => {
    api.setHotkeyRecording(recordingField !== null).catch(() => {});
    if (!recordingField) return;

    const handleKeyDown = (e: KeyboardEvent) => {
      e.preventDefault();
      e.stopPropagation();

      if (e.key === 'Escape' && recordingField !== 'panic') {
        setRecordingField(null);
        return;
      }
      if ((e.key === 'Backspace' || e.key === 'Delete') && recordingField === 'overlay') {
        setLocalSettings((prev) => ({ ...prev, overlayHotkey: '' }));
        setRecordingField(null);
        return;
      }

      const combo = formatKeyboardCombo(e);
      if (!combo) return;

      if (recordingField === 'panic') {
        setLocalSettings((prev) => ({ ...prev, panicHotkey: combo }));
      } else if (recordingField === 'overlay') {
        setLocalSettings((prev) => ({ ...prev, overlayHotkey: combo }));
      }
      setRecordingField(null);
    };

    window.addEventListener('keydown', handleKeyDown, true);
    return () => window.removeEventListener('keydown', handleKeyDown, true);
  }, [recordingField]);

  const handleCancel = () => {
    api
      .setVolumes(
        settings.micVolume,
        settings.monitorVolume,
        settings.broadcastVolume,
        settings.monitorMuted,
        settings.broadcastMuted
      )
      .catch(() => {});
    api
      .setDuckingSettings(settings.voiceDuckingEnabled, settings.voiceDuckingDb)
      .catch(() => {});
    api.setAecSafeMonitor(settings.aecSafeMonitor).catch(() => {});
    onClose();
  };

  const physicalInputs = devices.inputDevices.filter((d) => !d.isVirtual);
  const virtualOutputs = devices.outputDevices.filter((d) => d.isVirtual);
  const physicalOutputs = devices.outputDevices.filter((d) => !d.isVirtual);

  return (
    <Dialog
      open={isOpen}
      onOpenChange={(open) => {
        if (!open) handleCancel();
      }}
    >
      <DialogContent className="max-w-md">
        <DialogHeader>
          <div className="flex items-center gap-2 pr-6">
            <Sliders className="w-4 h-4 text-zinc-200" />
            <div>
              <DialogTitle>Audio &amp; Hotkey Settings</DialogTitle>
              <DialogDescription>Virtual mic routing, DSP &amp; global keybinds</DialogDescription>
            </div>
          </div>
        </DialogHeader>

        <div className="px-4 pt-3 overflow-y-auto flex-1">
          <Tabs value={activeTab} onValueChange={setActiveTab} className="w-full">
            <TabsList className="w-full grid grid-cols-3">
              <TabsTrigger value="routing">Routing</TabsTrigger>
              <TabsTrigger value="ducking">DSP &amp; AEC</TabsTrigger>
              <TabsTrigger value="hotkeys">Hotkeys</TabsTrigger>
            </TabsList>

            <TabsContent value="routing" className="space-y-3.5 pb-4">
              {/* Built-in Virtual Mic Status Card */}
              <div className="p-3 rounded-lg bg-zinc-900 border border-zinc-800 space-y-2.5">
                <div className="flex items-start justify-between gap-2">
                  <div className="flex items-start gap-2 min-w-0">
                    {virtualMicStatus?.virtualDeviceAttached ? (
                      <CheckCircle2 className="w-4 h-4 text-emerald-400 shrink-0 mt-0.5" />
                    ) : (
                      <AlertTriangle className="w-4 h-4 text-amber-400 shrink-0 mt-0.5" />
                    )}
                    <div className="min-w-0 space-y-0.5">
                      <p className="text-[11px] font-semibold text-zinc-100">
                        Memeboard Virtual Mic
                      </p>
                      <p className="text-[10px] text-zinc-400">
                        {virtualMicStatus?.statusMessage || 'Checking virtual driver...'}
                      </p>
                      {virtualMicStatus?.savedPhysicalMic && (
                        <p className="text-[10px] text-zinc-300 truncate">
                          Restores: {virtualMicStatus.savedPhysicalMic}
                        </p>
                      )}
                    </div>
                  </div>

                  <div className="flex items-center gap-1 shrink-0">
                    {virtualMicStatus?.virtualMicIsDefault && (
                      <Button type="button" variant="secondary" size="xs" onClick={onRestoreOriginalMic}>
                        <RotateCcw className="w-3 h-3" /> Restore
                      </Button>
                    )}
                    {(!virtualMicStatus?.driverInstalled || !virtualMicStatus?.virtualDeviceAttached) && (
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
                    )}
                  </div>
                </div>

                <div className="pt-2 border-t border-zinc-800 flex items-center justify-between gap-3">
                  <div>
                    <p className="text-[11px] font-medium text-zinc-100">
                      Auto-Switch Default Mic
                    </p>
                    <p className="text-[10px] text-zinc-400">
                      Reconnect TeamSpeak/games once if opened before Memeboard.
                    </p>
                  </div>
                  <Switch
                    checked={localSettings.autoSwitchDefaultMic}
                    onCheckedChange={(next) => {
                      setLocalSettings((p) => ({ ...p, autoSwitchDefaultMic: next }));
                      onSetAutoSwitchMic(next);
                    }}
                  />
                </div>
              </div>

              <div className="flex items-center justify-between">
                <span className="text-[10px] font-semibold text-zinc-400 uppercase tracking-wider">
                  Endpoints
                </span>
                <button
                  type="button"
                  onClick={onRefreshDevices}
                  className="flex items-center gap-1 text-[10px] text-zinc-300 hover:text-white cursor-pointer"
                >
                  <RefreshCw className="w-3 h-3" /> Refresh
                </button>
              </div>

              <div className="space-y-2.5">
                <div className="space-y-1">
                  <label className="text-[11px] font-medium text-zinc-300 flex items-center gap-1.5">
                    <Mic className="w-3 h-3" /> Physical Mic
                  </label>
                  <select
                    value={localSettings.micDeviceId || ''}
                    onChange={(e) =>
                      setLocalSettings({ ...localSettings, micDeviceId: e.target.value || null })
                    }
                    className="w-full h-8 bg-zinc-900 text-xs text-zinc-100 px-2.5 rounded-md border border-zinc-800 focus:border-zinc-500 outline-none"
                  >
                    <option value="">Auto (Default Physical Mic)</option>
                    {physicalInputs.map((d) => (
                      <option key={d.id} value={d.id}>
                        {d.name}
                      </option>
                    ))}
                  </select>
                </div>

                <div className="space-y-1">
                  <label className="text-[11px] font-medium text-zinc-300 flex items-center gap-1.5">
                    <Radio className="w-3 h-3" /> Virtual Mic Bridge
                  </label>
                  <select
                    value={localSettings.broadcastDeviceId || ''}
                    onChange={(e) =>
                      setLocalSettings({
                        ...localSettings,
                        broadcastDeviceId: e.target.value || null,
                      })
                    }
                    className="w-full h-8 bg-zinc-900 text-xs text-zinc-100 px-2.5 rounded-md border border-zinc-800 focus:border-zinc-500 outline-none"
                  >
                    <option value="">Auto (Memeboard Virtual Mic)</option>
                    {virtualOutputs.map((d) => (
                      <option key={d.id} value={d.id}>
                        [Virtual] {d.name}
                      </option>
                    ))}
                    {physicalOutputs.map((d) => (
                      <option key={d.id} value={d.id}>
                        {d.name}
                      </option>
                    ))}
                  </select>
                </div>

                <div className="space-y-1">
                  <label className="text-[11px] font-medium text-zinc-300 flex items-center gap-1.5">
                    <Headphones className="w-3 h-3" /> Headphones Monitor
                  </label>
                  <select
                    value={localSettings.monitorDeviceId || ''}
                    onChange={(e) =>
                      setLocalSettings({
                        ...localSettings,
                        monitorDeviceId: e.target.value || null,
                      })
                    }
                    className="w-full h-8 bg-zinc-900 text-xs text-zinc-100 px-2.5 rounded-md border border-zinc-800 focus:border-zinc-500 outline-none"
                  >
                    <option value="">System Default Playback</option>
                    {physicalOutputs.map((d) => (
                      <option key={d.id} value={d.id}>
                        {d.name}
                      </option>
                    ))}
                  </select>
                </div>

                <div className="pt-2 border-t border-zinc-800 space-y-1.5">
                  <div className="flex justify-between text-[11px]">
                    <span className="text-zinc-300">Mic Pass-Through Gain</span>
                    <span className="font-mono text-zinc-100">
                      {Math.round(localSettings.micVolume * 100)}%
                    </span>
                  </div>
                  <Slider
                    min={0}
                    max={2}
                    step={0.02}
                    value={[localSettings.micVolume]}
                    onValueChange={([nextMic]) => {
                      setLocalSettings({ ...localSettings, micVolume: nextMic });
                      api
                        .setVolumes(
                          nextMic,
                          localSettings.monitorVolume,
                          localSettings.broadcastVolume,
                          localSettings.monitorMuted,
                          localSettings.broadcastMuted
                        )
                        .catch(() => {});
                    }}
                  />
                </div>
              </div>
            </TabsContent>

            <TabsContent value="ducking" className="space-y-3.5 pb-4">
              <div className="flex items-center justify-between p-3 rounded-lg bg-zinc-900 border border-zinc-800 gap-3">
                <div>
                  <p className="text-[11px] font-semibold text-zinc-100">Auto Voice Ducking</p>
                  <p className="text-[10px] text-zinc-400">
                    Dips SFX volume whenever you speak
                  </p>
                </div>
                <Switch
                  checked={localSettings.voiceDuckingEnabled}
                  onCheckedChange={(next) => {
                    setLocalSettings({ ...localSettings, voiceDuckingEnabled: next });
                    api.setDuckingSettings(next, localSettings.voiceDuckingDb).catch(() => {});
                  }}
                />
              </div>

              <div className="space-y-1.5">
                <div className="flex justify-between text-[11px]">
                  <span className="text-zinc-300">Ducking Depth</span>
                  <span className="font-mono text-zinc-100">{localSettings.voiceDuckingDb} dB</span>
                </div>
                <Slider
                  min={-24}
                  max={0}
                  step={1}
                  disabled={!localSettings.voiceDuckingEnabled}
                  value={[localSettings.voiceDuckingDb]}
                  onValueChange={([nextDb]) => {
                    setLocalSettings({ ...localSettings, voiceDuckingDb: nextDb });
                    api
                      .setDuckingSettings(localSettings.voiceDuckingEnabled, nextDb)
                      .catch(() => {});
                  }}
                />
              </div>

              <div className="space-y-1.5 pt-2 border-t border-zinc-800">
                <div className="flex justify-between text-[11px]">
                  <span className="text-zinc-300 flex items-center gap-1">
                    <Volume2 className="w-3 h-3" /> Loudness Target (BS.1770)
                  </span>
                  <span className="font-mono text-zinc-100">
                    {localSettings.loudnessTargetLufs} LUFS
                  </span>
                </div>
                <Slider
                  min={-24}
                  max={-8}
                  step={1}
                  value={[localSettings.loudnessTargetLufs]}
                  onValueChange={([lufs]) =>
                    setLocalSettings({ ...localSettings, loudnessTargetLufs: lufs })
                  }
                />
              </div>

              <div className="flex items-center justify-between p-3 rounded-lg bg-zinc-900 border border-zinc-800 gap-3">
                <div>
                  <p className="text-[11px] font-semibold text-zinc-100">
                    AEC-Safe Monitor (+7 Hz)
                  </p>
                  <p className="text-[10px] text-zinc-400">
                    Phase-shifts headphones only so Echo Cancellation can&apos;t cancel clips.
                  </p>
                </div>
                <Switch
                  checked={localSettings.aecSafeMonitor}
                  onCheckedChange={(next) => {
                    setLocalSettings({ ...localSettings, aecSafeMonitor: next });
                    api.setAecSafeMonitor(next).catch(() => {});
                  }}
                />
              </div>

              <div className="p-3 rounded-lg bg-zinc-900/70 border border-zinc-800 space-y-1.5 text-[10px] text-zinc-400">
                <p className="font-semibold text-zinc-200 flex items-center gap-1">
                  <ShieldAlert className="w-3 h-3 text-amber-400" /> Voice App Checklist
                </p>
                <p>
                  <strong className="text-zinc-200">TeamSpeak:</strong> Uncheck Echo cancellation,
                  Echo reduction, and set background noise Off/Low.
                </p>
                <p>
                  <strong className="text-zinc-200">Discord:</strong> Disable Echo Cancellation &amp;
                  Krisp Noise Suppression.
                </p>
                <p>
                  <strong className="text-zinc-200">Google Meet:</strong> Turn Noise cancellation
                  Off.
                </p>
              </div>
            </TabsContent>

            <TabsContent value="hotkeys" className="space-y-3 pb-4">
              <p className="text-[10px] text-zinc-400 bg-zinc-900 border border-zinc-800 rounded-md p-2.5">
                Non-consuming Raw Input pass-through hooks work globally inside games without
                blocking Esc or F-keys.
              </p>

              <div className="p-3 rounded-lg bg-zinc-900 border border-zinc-800 flex items-center justify-between gap-3">
                <div>
                  <p className="text-[11px] font-semibold text-zinc-100">Panic Stop All</p>
                  <p className="text-[10px] text-zinc-400">Halts every playing clip</p>
                </div>
                <button
                  type="button"
                  onClick={() => setRecordingField('panic')}
                  className={cn(
                    'min-w-36 h-8 px-2.5 rounded-md text-[11px] font-mono border cursor-pointer',
                    recordingField === 'panic'
                      ? 'bg-zinc-100 text-zinc-950 border-white'
                      : 'bg-zinc-950 border-zinc-800 text-zinc-200'
                  )}
                >
                  {recordingField === 'panic'
                    ? 'Press keys...'
                    : localSettings.panicHotkey || 'Unbound'}
                </button>
              </div>

              <div className="p-3 rounded-lg bg-zinc-900 border border-zinc-800 flex items-center justify-between gap-3">
                <div>
                  <p className="text-[11px] font-semibold text-zinc-100 flex items-center gap-1">
                    <Keyboard className="w-3 h-3" /> Toggle Overlay
                  </p>
                  <p className="text-[10px] text-zinc-400">Bottom-right floating overlay</p>
                </div>
                <button
                  type="button"
                  onClick={() => setRecordingField('overlay')}
                  className={cn(
                    'min-w-36 h-8 px-2.5 rounded-md text-[11px] font-mono border cursor-pointer',
                    recordingField === 'overlay'
                      ? 'bg-zinc-100 text-zinc-950 border-white'
                      : 'bg-zinc-950 border-zinc-800 text-zinc-200'
                  )}
                >
                  {recordingField === 'overlay'
                    ? 'Press keys...'
                    : localSettings.overlayHotkey || 'Bind key'}
                </button>
              </div>
            </TabsContent>
          </Tabs>
        </div>

        <div className="flex items-center justify-end gap-1.5 px-4 py-3 bg-zinc-900/60 border-t border-zinc-800 shrink-0">
          <Button type="button" variant="ghost" size="sm" onClick={handleCancel}>
            Cancel
          </Button>
          <Button
            type="button"
            variant="default"
            size="sm"
            onClick={() => {
              onSaveSettings(localSettings);
              onClose();
            }}
          >
            Save &amp; Apply
          </Button>
        </div>
      </DialogContent>
    </Dialog>
  );
};

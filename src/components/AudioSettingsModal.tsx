import React, { useEffect, useRef, useState } from 'react';
import {
  X,
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

function formatHotkeyCombo(e: KeyboardEvent): string | null {
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
  const [activeTab, setActiveTab] = useState<'routing' | 'ducking' | 'hotkeys'>('routing');
  const [recordingField, setRecordingField] = useState<'panic' | 'overlay' | null>(null);
  const wasOpenRef = useRef(false);

  // Sync local draft ONLY on modal open transition so background updates don't wipe edits
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

  // Global keybind recorder (pauses Raw Input triggers while active)
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

      const combo = formatHotkeyCombo(e);
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

  if (!isOpen) return null;

  const handleSave = () => {
    onSaveSettings(localSettings);
    onClose();
  };

  const handleCancel = () => {
    // Revert any live-previewed volume/ducking adjustments back to persisted settings
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
    onClose();
  };

  const physicalInputs = devices.inputDevices.filter((d) => !d.isVirtual);
  const virtualOutputs = devices.outputDevices.filter((d) => d.isVirtual);
  const physicalOutputs = devices.outputDevices.filter((d) => !d.isVirtual);

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/75 backdrop-blur-sm p-4 select-none animate-in fade-in">
      <div className="bg-[#313338] border border-[#232428] w-full max-w-2xl rounded-2xl shadow-2xl overflow-hidden flex flex-col max-h-[92vh]">
        {/* Modal Header */}
        <div className="flex items-center justify-between px-6 py-4 border-b border-[#232428] shrink-0">
          <div className="flex items-center gap-3">
            <div className="w-9 h-9 rounded-xl bg-[#5865f2] flex items-center justify-center shadow-md">
              <Sliders className="w-5 h-5 text-white" />
            </div>
            <div>
              <h2 className="text-base font-bold text-white">Audio Routing, Virtual Mic &amp; DSP</h2>
              <p className="text-xs text-[#949ba4]">
                Keep your physical microphone pure while broadcasting sound effects seamlessly
              </p>
            </div>
          </div>
          <button
            type="button"
            onClick={handleCancel}
            className="p-1.5 rounded-lg text-[#949ba4] hover:text-white hover:bg-[#35373c] transition-colors"
          >
            <X className="w-5 h-5" />
          </button>
        </div>

        {/* Tab Navigation */}
        <div className="flex items-center gap-1 px-6 pt-3 border-b border-[#232428] bg-[#2b2d31]/50 text-xs font-semibold shrink-0">
          <button
            type="button"
            onClick={() => setActiveTab('routing')}
            className={`pb-2.5 px-3 border-b-2 transition-colors ${
              activeTab === 'routing'
                ? 'border-[#5865f2] text-white'
                : 'border-transparent text-[#949ba4] hover:text-[#dbdee1]'
            }`}
          >
            Virtual Mic &amp; Devices
          </button>
          <button
            type="button"
            onClick={() => setActiveTab('ducking')}
            className={`pb-2.5 px-3 border-b-2 transition-colors ${
              activeTab === 'ducking'
                ? 'border-[#5865f2] text-white'
                : 'border-transparent text-[#949ba4] hover:text-[#dbdee1]'
            }`}
          >
            Voice Ducking &amp; LUFS
          </button>
          <button
            type="button"
            onClick={() => setActiveTab('hotkeys')}
            className={`pb-2.5 px-3 border-b-2 transition-colors ${
              activeTab === 'hotkeys'
                ? 'border-[#5865f2] text-white'
                : 'border-transparent text-[#949ba4] hover:text-[#dbdee1]'
            }`}
          >
            Global Hotkeys
          </button>
        </div>

        {/* Tab Body */}
        <div className="p-6 overflow-y-auto space-y-5">
          {activeTab === 'routing' && (
            <>
              {/* Built-in Virtual Mic Status Card */}
              <div
                className={`p-4 rounded-xl border ${
                  virtualMicStatus?.virtualDeviceAttached
                    ? 'bg-[#23a55a]/10 border-[#23a55a]/35'
                    : 'bg-[#2b2d31] border-[#f0b232]/40'
                }`}
              >
                <div className="flex items-start justify-between gap-3">
                  <div className="flex items-start gap-3">
                    {virtualMicStatus?.virtualDeviceAttached ? (
                      <CheckCircle2 className="w-5 h-5 text-[#23a55a] shrink-0 mt-0.5" />
                    ) : (
                      <AlertTriangle className="w-5 h-5 text-[#f0b232] shrink-0 mt-0.5" />
                    )}
                    <div className="space-y-1">
                      <p className="text-xs font-bold text-white">
                        Built-in Virtual Microphone (Memeboard Virtual Mic)
                      </p>
                      <p className="text-[11px] text-[#dbdee1]">
                        {virtualMicStatus?.statusMessage ||
                          'Checking built-in USB Audio virtual driver status...'}
                      </p>
                      {virtualMicStatus?.savedPhysicalMic && (
                        <p className="text-[11px] text-[#23a55a]">
                          Saved original mic: <strong>{virtualMicStatus.savedPhysicalMic}</strong>{' '}
                          (restored automatically when Memeboard quits)
                        </p>
                      )}
                    </div>
                  </div>

                  <div className="flex items-center gap-2 shrink-0">
                    {virtualMicStatus?.virtualMicIsDefault && (
                      <button
                        type="button"
                        onClick={onRestoreOriginalMic}
                        className="flex items-center gap-1 px-2.5 py-1.5 rounded-lg bg-[#35373c] hover:bg-[#404249] text-[11px] font-semibold text-white transition-colors"
                      >
                        <RotateCcw className="w-3 h-3" /> Restore Mic
                      </button>
                    )}
                    {(!virtualMicStatus?.driverInstalled || !virtualMicStatus?.virtualDeviceAttached) && (
                      <button
                        type="button"
                        onClick={onInstallVirtualDriver}
                        disabled={isInstallingDriver}
                        className="flex items-center gap-1.5 px-3 py-1.5 rounded-lg bg-[#5865f2] hover:bg-[#4752c4] disabled:opacity-60 text-[11px] font-semibold text-white transition-colors"
                      >
                        <Wrench className="w-3.5 h-3.5" />
                        <span>
                          {isInstallingDriver
                            ? 'Working...'
                            : virtualMicStatus?.driverInstalled
                            ? 'Attach Device'
                            : 'Install Driver'}
                        </span>
                      </button>
                    )}
                  </div>
                </div>

                {/* Auto-switch Windows Default Mic toggle */}
                <div className="mt-3 pt-3 border-t border-white/10 flex items-center justify-between gap-4">
                  <div>
                    <p className="text-xs font-semibold text-white">
                      Auto-Switch Windows Default Mic While Open
                    </p>
                    <p className="text-[11px] text-[#949ba4]">
                      Apps &amp; games using Default Mic get Memeboard automatically without touching your real mic settings
                    </p>
                  </div>
                  <button
                    type="button"
                    onClick={() => {
                      const next = !localSettings.autoSwitchDefaultMic;
                      setLocalSettings((p) => ({ ...p, autoSwitchDefaultMic: next }));
                      onSetAutoSwitchMic(next);
                    }}
                    className={`w-11 h-6 rounded-full transition-colors p-0.5 shrink-0 ${
                      localSettings.autoSwitchDefaultMic ? 'bg-[#23a55a]' : 'bg-[#35373c]'
                    }`}
                  >
                    <div
                      className={`w-5 h-5 rounded-full bg-white shadow transform transition-transform ${
                        localSettings.autoSwitchDefaultMic ? 'translate-x-5' : 'translate-x-0'
                      }`}
                    />
                  </button>
                </div>
              </div>

              <div className="flex items-center justify-between">
                <span className="text-xs font-semibold text-[#949ba4] uppercase tracking-wider">
                  Audio Endpoints
                </span>
                <button
                  type="button"
                  onClick={onRefreshDevices}
                  className="flex items-center gap-1.5 text-xs text-[#5865f2] hover:underline font-medium"
                >
                  <RefreshCw className="w-3.5 h-3.5" /> Refresh Devices
                </button>
              </div>

              {/* Physical Microphone Input */}
              <div className="space-y-1.5">
                <label className="text-xs font-semibold text-[#dbdee1] flex items-center gap-2">
                  <Mic className="w-4 h-4 text-[#5865f2]" /> Physical Microphone Input (Untouched Source)
                </label>
                <select
                  value={localSettings.micDeviceId || ''}
                  onChange={(e) =>
                    setLocalSettings({
                      ...localSettings,
                      micDeviceId: e.target.value || null,
                    })
                  }
                  className="w-full bg-[#1e1f22] text-sm text-white px-3 py-2.5 rounded-xl border border-[#232428] focus:border-[#5865f2] outline-none"
                >
                  <option value="">Auto (Default Physical Microphone)</option>
                  {physicalInputs.map((d) => (
                    <option key={d.id} value={d.id}>
                      {d.name} {d.isDefault ? '(OS Default)' : ''}
                    </option>
                  ))}
                </select>
                {devices.currentInput && (
                  <p className="text-[11px] text-[#949ba4]">
                    Active stream: <span className="text-[#dbdee1]">{devices.currentInput}</span>
                  </p>
                )}
              </div>

              {/* Broadcast Virtual Mic Bridge */}
              <div className="space-y-1.5">
                <label className="text-xs font-semibold text-[#dbdee1] flex items-center gap-2">
                  <Radio className="w-4 h-4 text-[#23a55a]" /> Virtual Mic Bridge Output (Sent to Discord/Apps)
                </label>
                <select
                  value={localSettings.broadcastDeviceId || ''}
                  onChange={(e) =>
                    setLocalSettings({
                      ...localSettings,
                      broadcastDeviceId: e.target.value || null,
                    })
                  }
                  className="w-full bg-[#1e1f22] text-sm text-white px-3 py-2.5 rounded-xl border border-[#232428] focus:border-[#5865f2] outline-none"
                >
                  <option value="">Auto (Built-in Memeboard Virtual Mic / VB-Cable)</option>
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
                {devices.currentBroadcast && (
                  <p className="text-[11px] text-[#23a55a]">
                    Active broadcast target: {devices.currentBroadcast}
                  </p>
                )}
              </div>

              {/* Monitor Headphones Output */}
              <div className="space-y-1.5">
                <label className="text-xs font-semibold text-[#dbdee1] flex items-center gap-2">
                  <Headphones className="w-4 h-4 text-[#b5bac1]" /> Local Monitor Headphones (You Hear Clips &amp; Previews)
                </label>
                <select
                  value={localSettings.monitorDeviceId || ''}
                  onChange={(e) =>
                    setLocalSettings({
                      ...localSettings,
                      monitorDeviceId: e.target.value || null,
                    })
                  }
                  className="w-full bg-[#1e1f22] text-sm text-white px-3 py-2.5 rounded-xl border border-[#232428] focus:border-[#5865f2] outline-none"
                >
                  <option value="">System Default Playback Device</option>
                  {physicalOutputs.map((d) => (
                    <option key={d.id} value={d.id}>
                      {d.name} {d.isDefault ? '(Default)' : ''}
                    </option>
                  ))}
                </select>
              </div>

              {/* Mic Pass-Through Gain */}
              <div className="pt-2 border-t border-[#232428] space-y-2">
                <div className="flex justify-between text-xs font-semibold">
                  <span className="text-[#dbdee1]">Voice Pass-Through Level (100% = Bit-Transparent)</span>
                  <span className="font-mono text-[#5865f2]">
                    {Math.round(localSettings.micVolume * 100)}%
                  </span>
                </div>
                <input
                  type="range"
                  min="0"
                  max="2"
                  step="0.02"
                  value={localSettings.micVolume}
                  onChange={(e) => {
                    const nextMic = parseFloat(e.target.value);
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
                  className="w-full h-2 bg-[#1e1f22] rounded-lg appearance-none cursor-pointer accent-[#5865f2]"
                />
              </div>
            </>
          )}

          {activeTab === 'ducking' && (
            <div className="space-y-6">
              <div className="flex items-center justify-between p-4 rounded-xl bg-[#2b2d31] border border-[#232428]">
                <div>
                  <h3 className="text-sm font-bold text-white">Auto Voice Ducking</h3>
                  <p className="text-xs text-[#949ba4] mt-0.5">
                    Smoothly lowers playing sound clips whenever your microphone detects speech
                  </p>
                </div>
                <button
                  type="button"
                  onClick={() => {
                    const next = !localSettings.voiceDuckingEnabled;
                    setLocalSettings({ ...localSettings, voiceDuckingEnabled: next });
                    api.setDuckingSettings(next, localSettings.voiceDuckingDb).catch(() => {});
                  }}
                  className={`w-12 h-6 rounded-full transition-colors p-0.5 ${
                    localSettings.voiceDuckingEnabled ? 'bg-[#23a55a]' : 'bg-[#35373c]'
                  }`}
                >
                  <div
                    className={`w-5 h-5 rounded-full bg-white shadow transform transition-transform ${
                      localSettings.voiceDuckingEnabled ? 'translate-x-6' : 'translate-x-0'
                    }`}
                  />
                </button>
              </div>

              <div className="space-y-2">
                <div className="flex justify-between text-xs font-semibold">
                  <span className="text-[#dbdee1]">Ducking Attenuation Depth</span>
                  <span className="font-mono text-[#5865f2]">
                    {localSettings.voiceDuckingDb} dB
                  </span>
                </div>
                <input
                  type="range"
                  min="-24"
                  max="0"
                  step="1"
                  disabled={!localSettings.voiceDuckingEnabled}
                  value={localSettings.voiceDuckingDb}
                  onChange={(e) => {
                    const nextDb = parseFloat(e.target.value);
                    setLocalSettings({ ...localSettings, voiceDuckingDb: nextDb });
                    api.setDuckingSettings(localSettings.voiceDuckingEnabled, nextDb).catch(() => {});
                  }}
                  className="w-full h-2 bg-[#1e1f22] rounded-lg appearance-none cursor-pointer accent-[#5865f2] disabled:opacity-50"
                />
                <div className="flex justify-between text-[10px] text-[#949ba4]">
                  <span>-24 dB (Whisper quiet SFX)</span>
                  <span>-10 dB (Balanced)</span>
                  <span>0 dB (No ducking)</span>
                </div>
              </div>

              <div className="pt-4 border-t border-[#232428] space-y-2">
                <div className="flex justify-between text-xs font-semibold">
                  <span className="text-[#dbdee1] flex items-center gap-1.5">
                    <Volume2 className="w-4 h-4 text-[#23a55a]" /> Auto Loudness Target (ITU-R BS.1770)
                  </span>
                  <span className="font-mono text-[#23a55a]">
                    {localSettings.loudnessTargetLufs} LUFS
                  </span>
                </div>
                <input
                  type="range"
                  min="-24"
                  max="-8"
                  step="1"
                  value={localSettings.loudnessTargetLufs}
                  onChange={(e) =>
                    setLocalSettings({
                      ...localSettings,
                      loudnessTargetLufs: parseFloat(e.target.value),
                    })
                  }
                  className="w-full h-2 bg-[#1e1f22] rounded-lg appearance-none cursor-pointer accent-[#23a55a]"
                />
                <p className="text-[11px] text-[#949ba4]">
                  Recomputes gated loudness across every trimmed clip automatically when saved.
                </p>
              </div>
            </div>
          )}

          {activeTab === 'hotkeys' && (
            <div className="space-y-4">
              <div className="p-3.5 rounded-xl bg-[#5865f2]/10 border border-[#5865f2]/30 text-xs text-[#dbdee1]">
                Memeboard uses non-consuming Windows Raw Input pass-through hooks — hotkeys trigger
                cleanly in fullscreen games without ever blocking keys like <kbd className="px-1.5 py-0.5 rounded bg-[#1e1f22] font-mono">Esc</kbd> or{' '}
                <kbd className="px-1.5 py-0.5 rounded bg-[#1e1f22] font-mono">F1–F5</kbd>.
              </div>

              {/* Panic Key */}
              <div className="p-4 rounded-xl bg-[#2b2d31] border border-[#232428] flex items-center justify-between gap-4">
                <div>
                  <h4 className="text-xs font-bold text-white flex items-center gap-1.5">
                    <ShieldAlert className="w-4 h-4 text-[#f23f42]" /> Panic Key (Stop All Sounds)
                  </h4>
                  <p className="text-[11px] text-[#949ba4] mt-0.5">
                    Instantly stops all playing audio clips anywhere
                  </p>
                </div>
                <button
                  type="button"
                  onClick={() => setRecordingField('panic')}
                  className={`min-w-44 py-2 px-3 rounded-lg text-xs font-mono border text-center transition-all ${
                    recordingField === 'panic'
                      ? 'bg-[#5865f2]/20 border-[#5865f2] text-white animate-pulse'
                      : 'bg-[#1e1f22] border-transparent text-[#dbdee1] hover:border-[#35373c]'
                  }`}
                >
                  {recordingField === 'panic'
                    ? 'Press key combo...'
                    : localSettings.panicHotkey || 'Unbound'}
                </button>
              </div>

              {/* Overlay Toggle */}
              <div className="p-4 rounded-xl bg-[#2b2d31] border border-[#232428] flex items-center justify-between gap-4">
                <div>
                  <h4 className="text-xs font-bold text-white flex items-center gap-1.5">
                    <Keyboard className="w-4 h-4 text-[#5865f2]" /> Toggle Floating Mini Overlay
                  </h4>
                  <p className="text-[11px] text-[#949ba4] mt-0.5">
                    Shows or hides the non-focus-stealing soundboard overlay
                  </p>
                </div>
                <button
                  type="button"
                  onClick={() => setRecordingField('overlay')}
                  className={`min-w-44 py-2 px-3 rounded-lg text-xs font-mono border text-center transition-all ${
                    recordingField === 'overlay'
                      ? 'bg-[#5865f2]/20 border-[#5865f2] text-white animate-pulse'
                      : 'bg-[#1e1f22] border-transparent text-[#dbdee1] hover:border-[#35373c]'
                  }`}
                >
                  {recordingField === 'overlay'
                    ? 'Press key combo...'
                    : localSettings.overlayHotkey || 'Click to bind'}
                </button>
              </div>
            </div>
          )}
        </div>

        {/* Footer */}
        <div className="flex items-center justify-end gap-2 px-6 py-4 bg-[#2b2d31] border-t border-[#232428] shrink-0">
          <button
            type="button"
            onClick={handleCancel}
            className="px-4 py-2 rounded-lg text-xs font-semibold text-[#dbdee1] hover:bg-[#35373c] transition-colors"
          >
            Cancel
          </button>
          <button
            type="button"
            onClick={handleSave}
            className="px-5 py-2 rounded-lg bg-[#5865f2] hover:bg-[#4752c4] text-white text-xs font-semibold shadow-md transition-colors"
          >
            Save &amp; Apply
          </button>
        </div>
      </div>
    </div>
  );
};

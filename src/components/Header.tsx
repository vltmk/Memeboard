import React from 'react';
import {
  Plus,
  Settings,
  Volume2,
  VolumeX,
  Radio,
  Square,
  Layers,
  Mic,
  AlertTriangle,
} from 'lucide-react';
import { AudioPlaybackStatus, AudioSettings, VirtualMicStatus } from '../types';

interface HeaderProps {
  settings: AudioSettings;
  playbackStatus: AudioPlaybackStatus;
  virtualMicStatus: VirtualMicStatus | null;
  onAddSoundClick: () => void;
  onOpenSettings: () => void;
  onStopAll: () => void;
  onToggleOverlay: () => void;
  onToggleMonitorMute: () => void;
  onMonitorVolumeChange: (val: number) => void;
  onToggleBroadcastMute: () => void;
  onBroadcastVolumeChange: (val: number) => void;
}

export const Header: React.FC<HeaderProps> = ({
  settings,
  playbackStatus,
  virtualMicStatus,
  onAddSoundClick,
  onOpenSettings,
  onStopAll,
  onToggleOverlay,
  onToggleMonitorMute,
  onMonitorVolumeChange,
  onToggleBroadcastMute,
  onBroadcastVolumeChange,
}) => {
  const isPlaying = playbackStatus.activeClipCount > 0;
  const driverReady = virtualMicStatus?.driverInstalled ?? false;
  const vmicAttached = virtualMicStatus?.virtualDeviceAttached ?? false;
  const routingHealthy =
    playbackStatus.broadcastStreamActive && playbackStatus.micStreamActive;

  let statusLabel = 'Audio Engine Ready';
  let statusColor = 'text-[#949ba4] bg-[#232428] border-[#35373c]';
  let dotColor = 'bg-[#949ba4]';

  if (routingHealthy) {
    statusLabel = playbackStatus.micActive ? 'Voice Active • Live' : 'Virtual Mic Live';
    statusColor = 'text-[#23a55a] bg-[#23a55a]/10 border-[#23a55a]/30';
    dotColor = playbackStatus.micActive ? 'bg-[#23a55a] animate-ping' : 'bg-[#23a55a]';
  } else if (!driverReady || !vmicAttached) {
    statusLabel = driverReady ? 'Virtual Mic Standby' : 'Driver Setup Needed';
    statusColor = 'text-[#f0b232] bg-[#f0b232]/10 border-[#f0b232]/30';
    dotColor = 'bg-[#f0b232]';
  } else if (!playbackStatus.micStreamActive || playbackStatus.lastAudioError) {
    statusLabel = 'Check Audio Input';
    statusColor = 'text-[#f23f42] bg-[#f23f42]/10 border-[#f23f42]/30';
    dotColor = 'bg-[#f23f42]';
  }

  const tooltip =
    playbackStatus.lastAudioError ||
    virtualMicStatus?.statusMessage ||
    'Real-time pass-through & soundboard status';

  return (
    <header className="flex items-center justify-between px-5 py-3 bg-[#2b2d31] border-b border-[#1e1f22] select-none shrink-0 gap-4">
      {/* Left: Brand + Health Badge */}
      <div className="flex items-center gap-3 min-w-0">
        <div className="w-9 h-9 rounded-xl bg-gradient-to-br from-[#5865f2] to-[#4752c4] flex items-center justify-center shadow-lg shadow-[#5865f2]/20 shrink-0">
          <Radio className="w-5 h-5 text-white" />
        </div>
        <div className="flex flex-col min-w-0">
          <div className="flex items-center gap-2.5">
            <h1 className="text-base font-bold tracking-tight text-white leading-none">
              Memeboard
            </h1>
            <button
              type="button"
              onClick={onOpenSettings}
              title={tooltip}
              className={`inline-flex items-center gap-1.5 px-2 py-0.5 rounded-full text-[11px] font-semibold border transition-colors ${statusColor}`}
            >
              <span className="relative flex h-2 w-2">
                <span className={`inline-flex h-2 w-2 rounded-full ${dotColor}`} />
              </span>
              {statusLabel}
              {!routingHealthy && (!driverReady || !vmicAttached || playbackStatus.lastAudioError) && (
                <AlertTriangle className="w-3 h-3 ml-0.5" />
              )}
            </button>
          </div>
          <span className="text-[11px] text-[#949ba4] truncate mt-0.5">
            {virtualMicStatus?.virtualMicIsDefault
              ? 'Default Mic: Memeboard Virtual Mic (Physical mic preserved)'
              : 'Clean Mic Pass-Through & Virtual Soundboard'}
          </span>
        </div>
      </div>

      {/* Center: Live Dual Mix Controls (Monitor Headphones + Virtual Mic Output) */}
      <div className="hidden md:flex items-center gap-3 bg-[#232428] px-3 py-1.5 rounded-xl border border-[#35373c]/60">
        {/* Local Headphones Monitor */}
        <div className="flex items-center gap-2 pr-3 border-r border-[#35373c]">
          <button
            type="button"
            onClick={onToggleMonitorMute}
            title={settings.monitorMuted ? 'Unmute Local Headphones' : 'Mute Local Headphones'}
            className={`p-1.5 rounded-lg transition-colors ${
              settings.monitorMuted
                ? 'bg-[#f23f42]/15 text-[#f23f42] hover:bg-[#f23f42]/25'
                : 'text-[#b5bac1] hover:text-white hover:bg-[#35373c]'
            }`}
          >
            {settings.monitorMuted ? (
              <VolumeX className="w-4 h-4" />
            ) : (
              <Volume2 className="w-4 h-4" />
            )}
          </button>
          <div className="flex flex-col w-20 xl:w-24">
            <div className="flex justify-between text-[10px] font-medium text-[#949ba4]">
              <span>Headphones</span>
              <span>{settings.monitorMuted ? 'MUTED' : `${Math.round(settings.monitorVolume * 100)}%`}</span>
            </div>
            <input
              type="range"
              min="0"
              max="1"
              step="0.01"
              value={settings.monitorVolume}
              onChange={(e) => onMonitorVolumeChange(parseFloat(e.target.value))}
              className="w-full h-1.5 bg-[#35373c] rounded-lg appearance-none cursor-pointer accent-[#5865f2] mt-1"
            />
          </div>
        </div>

        {/* Virtual Mic Broadcast Volume */}
        <div className="flex items-center gap-2">
          <button
            type="button"
            onClick={onToggleBroadcastMute}
            title={
              settings.broadcastMuted
                ? 'Unmute Soundboard into Virtual Mic'
                : 'Mute Soundboard into Virtual Mic (Voice still passes through)'
            }
            className={`p-1.5 rounded-lg transition-colors ${
              settings.broadcastMuted
                ? 'bg-[#f23f42]/15 text-[#f23f42] hover:bg-[#f23f42]/25'
                : 'text-[#23a55a] hover:bg-[#35373c]'
            }`}
          >
            <Mic className="w-4 h-4" />
          </button>
          <div className="flex flex-col w-20 xl:w-24">
            <div className="flex justify-between text-[10px] font-medium text-[#949ba4]">
              <span>Mic SFX Mix</span>
              <span>
                {settings.broadcastMuted ? 'MUTED' : `${Math.round(settings.broadcastVolume * 100)}%`}
              </span>
            </div>
            <input
              type="range"
              min="0"
              max="1"
              step="0.01"
              value={settings.broadcastVolume}
              onChange={(e) => onBroadcastVolumeChange(parseFloat(e.target.value))}
              className="w-full h-1.5 bg-[#35373c] rounded-lg appearance-none cursor-pointer accent-[#23a55a] mt-1"
            />
          </div>
        </div>
      </div>

      {/* Right: Actions */}
      <div className="flex items-center gap-2 shrink-0">
        <button
          type="button"
          onClick={onStopAll}
          title={`Stop All Sounds (Panic Hotkey: ${settings.panicHotkey || 'None'})`}
          className={`flex items-center gap-1.5 px-3 py-2 rounded-lg text-xs font-semibold transition-all ${
            isPlaying
              ? 'bg-[#f23f42] hover:bg-[#d83538] text-white shadow-md shadow-[#f23f42]/25'
              : 'bg-[#35373c] hover:bg-[#404249] text-[#dbdee1]'
          }`}
        >
          <Square className="w-3.5 h-3.5 fill-current" />
          <span>Stop All</span>
        </button>

        <button
          type="button"
          onClick={onToggleOverlay}
          title={`Toggle Mini Floating Overlay (${settings.overlayHotkey || 'Not Bound'})`}
          className="p-2 rounded-lg bg-[#35373c] hover:bg-[#404249] text-[#dbdee1] hover:text-white transition-colors"
        >
          <Layers className="w-4 h-4" />
        </button>

        <button
          type="button"
          onClick={onOpenSettings}
          title="Audio Routing, Virtual Mic & Hotkey Settings"
          className="p-2 rounded-lg bg-[#35373c] hover:bg-[#404249] text-[#dbdee1] hover:text-white transition-colors"
        >
          <Settings className="w-4 h-4" />
        </button>

        <button
          type="button"
          onClick={onAddSoundClick}
          className="flex items-center gap-1.5 px-3.5 py-2 rounded-lg bg-[#5865f2] hover:bg-[#4752c4] text-white text-xs font-semibold shadow-md shadow-[#5865f2]/20 transition-all cursor-pointer"
        >
          <Plus className="w-4 h-4" />
          <span>Add Sound</span>
        </button>
      </div>
    </header>
  );
};

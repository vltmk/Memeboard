import React from 'react';
import {
  Settings,
  Volume2,
  VolumeX,
  Square,
  Layers,
  Mic,
  ArrowDownRight,
  Package,
  Minus,
  X,
} from 'lucide-react';
import { AudioPlaybackStatus, AudioSettings, VirtualMicStatus } from '../types';
import { BrandIcon } from './BrandIcon';
import { Button } from './ui/button';
import { Slider } from './ui/slider';
import { cn } from '../lib/utils';

interface HeaderProps {
  settings: AudioSettings;
  playbackStatus: AudioPlaybackStatus;
  virtualMicStatus: VirtualMicStatus | null;
  isDocked: boolean;
  onOpenPacks: () => void;
  onOpenSettings: () => void;
  onStopAll: () => void;
  onToggleOverlay: () => void;
  onDockBottomRight: () => void;
  onMinimizeWindow: () => void;
  onHideToTray: () => void;
  onToggleMonitorMute: () => void;
  onMonitorVolumeChange: (val: number) => void;
  onToggleBroadcastMute: () => void;
  onBroadcastVolumeChange: (val: number) => void;
}

export const Header: React.FC<HeaderProps> = ({
  settings,
  playbackStatus,
  virtualMicStatus,
  isDocked,
  onOpenPacks,
  onOpenSettings,
  onStopAll,
  onToggleOverlay,
  onDockBottomRight,
  onMinimizeWindow,
  onHideToTray,
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
  const isVoiceLive = routingHealthy && playbackStatus.micActive;
  const isLive = routingHealthy && !playbackStatus.micActive;

  let statusLabel = 'Ready';
  let dotColor = 'bg-zinc-500';
  if (routingHealthy) {
    statusLabel = isVoiceLive ? 'Voice Live' : 'Live';
    dotColor = 'bg-emerald-400';
  } else if (!driverReady || !vmicAttached) {
    statusLabel = driverReady ? 'Standby' : 'Setup';
    dotColor = 'bg-amber-400';
  } else if (!playbackStatus.micStreamActive || playbackStatus.lastAudioError) {
    statusLabel = 'Check Mic';
    dotColor = 'bg-red-400';
  }

  const tooltip =
    playbackStatus.lastAudioError ||
    virtualMicStatus?.statusMessage ||
    'Audio routing status';

  return (
    <header className="bg-zinc-950/95 border-b border-zinc-800/90 select-none shrink-0">
      {/* Top Drag Drawer Handle + Actions */}
      <div
        data-tauri-drag-region
        className="flex items-center justify-between px-3 py-2 gap-1.5"
      >
        <div className="flex items-center gap-2 min-w-0" data-tauri-drag-region>
          <div className="w-6 h-6 rounded-md bg-zinc-100 text-zinc-950 flex items-center justify-center shrink-0 pointer-events-none shadow-xs">
            <BrandIcon className="w-4 h-4" />
          </div>
          <span className="text-xs font-semibold tracking-tight text-zinc-50 truncate pointer-events-none">
            Memeboard
          </span>

          {/* Smooth Animated Live / Voice Live Indicator */}
          <button
            type="button"
            onClick={onOpenSettings}
            title={tooltip}
            className={cn(
              'inline-flex items-center gap-1.5 rounded-full border px-2 py-0.5 text-[10px] font-mono font-medium cursor-pointer',
              'transition-all duration-300 ease-out shrink-0',
              isVoiceLive
                ? 'border-emerald-500/45 bg-emerald-500/12 text-emerald-300 shadow-[0_0_12px_-3px_rgba(16,185,129,0.45)]'
                : isLive
                  ? 'border-zinc-700/90 bg-zinc-900/90 text-zinc-200 hover:border-zinc-600'
                  : 'border-zinc-800 bg-zinc-950 text-zinc-400 hover:border-zinc-700'
            )}
          >
            <span className="relative flex h-2 w-2 items-center justify-center shrink-0">
              {isVoiceLive && (
                <span className="absolute inset-0 rounded-full bg-emerald-400/70 voice-ripple" />
              )}
              {isLive && (
                <span className="absolute inset-0 rounded-full bg-emerald-400/50 live-halo" />
              )}
              <span
                className={cn(
                  'relative h-1.5 w-1.5 rounded-full transition-colors duration-300',
                  dotColor
                )}
              />
            </span>

            <span className="transition-all duration-300">{statusLabel}</span>

            {isVoiceLive && (
              <span className="inline-flex items-end gap-[1.5px] h-2.5 ml-0.5">
                <span className="w-[1.5px] h-full rounded-full bg-emerald-400 origin-bottom voice-eq-1" />
                <span className="w-[1.5px] h-full rounded-full bg-emerald-300 origin-bottom voice-eq-2" />
                <span className="w-[1.5px] h-full rounded-full bg-emerald-400 origin-bottom voice-eq-3" />
              </span>
            )}
          </button>
        </div>

        <div className="flex items-center gap-0.5 shrink-0">
          <Button
            type="button"
            variant={isPlaying ? 'default' : 'ghost'}
            size="icon"
            onClick={onStopAll}
            title={`Panic Stop All (${settings.panicHotkey || 'Unbound'})`}
            className="h-6 w-6"
          >
            <Square className="w-2.5 h-2.5 fill-current" />
          </Button>

          <Button
            type="button"
            variant="ghost"
            size="icon"
            onClick={onToggleOverlay}
            title={`Toggle Overlay (${settings.overlayHotkey || 'Unbound'})`}
            className="h-6 w-6"
          >
            <Layers className="w-3.5 h-3.5" />
          </Button>

          <Button
            type="button"
            variant={isDocked ? 'ghost' : 'secondary'}
            size="icon"
            onClick={onDockBottomRight}
            title={isDocked ? 'Docked to Bottom-Right' : 'Snap Back to Bottom-Right'}
            className={cn(
              'h-6 w-6',
              isDocked ? 'text-zinc-300' : 'text-zinc-50 ring-1 ring-zinc-600'
            )}
          >
            <ArrowDownRight className="w-3.5 h-3.5" />
          </Button>

          <Button
            type="button"
            variant="ghost"
            size="icon"
            onClick={onOpenPacks}
            title="Import / Export Packs & Templates"
            className="h-6 w-6"
          >
            <Package className="w-3.5 h-3.5" />
          </Button>

          <Button
            type="button"
            variant="ghost"
            size="icon"
            onClick={onOpenSettings}
            title="Audio, Virtual Mic & Hotkey Settings"
            className="h-6 w-6"
          >
            <Settings className="w-3.5 h-3.5" />
          </Button>

          <div className="w-px h-3.5 bg-zinc-800 mx-0.5" />

          <Button
            type="button"
            variant="ghost"
            size="icon"
            onClick={onMinimizeWindow}
            title="Minimize"
            className="h-6 w-6 text-zinc-500 hover:text-zinc-100"
          >
            <Minus className="w-3.5 h-3.5" />
          </Button>

          <Button
            type="button"
            variant="ghost"
            size="icon"
            onClick={onHideToTray}
            title="Hide to System Tray"
            className="h-6 w-6 text-zinc-500 hover:text-zinc-100"
          >
            <X className="w-3.5 h-3.5" />
          </Button>
        </div>
      </div>

      {/* Dense Dual-Mix Strip */}
      <div className="grid grid-cols-2 gap-3 px-3 py-2 bg-zinc-900/50 border-t border-zinc-900">
        {/* Local Headphones Monitor */}
        <div className="flex items-center gap-2 min-w-0">
          <button
            type="button"
            onClick={onToggleMonitorMute}
            title={settings.monitorMuted ? 'Unmute Local Headphones' : 'Mute Local Headphones'}
            className={cn(
              'p-1 rounded-md transition-colors shrink-0 cursor-pointer',
              settings.monitorMuted
                ? 'bg-zinc-800 text-red-400'
                : 'text-zinc-300 hover:bg-zinc-800 hover:text-white'
            )}
          >
            {settings.monitorMuted ? (
              <VolumeX className="w-3.5 h-3.5" />
            ) : (
              <Volume2 className="w-3.5 h-3.5" />
            )}
          </button>
          <div className="flex-1 min-w-0 space-y-1">
            <div className="flex items-center justify-between text-[10px] text-zinc-400 leading-none">
              <span className="truncate">Local</span>
              <span className="font-mono text-zinc-200">
                {settings.monitorMuted ? 'MUTE' : `${Math.round(settings.monitorVolume * 100)}%`}
              </span>
            </div>
            <Slider
              min={0}
              max={1}
              step={0.01}
              value={[settings.monitorVolume]}
              onValueChange={([val]) => onMonitorVolumeChange(val)}
            />
          </div>
        </div>

        {/* Mic Broadcast SFX Mix */}
        <div className="flex items-center gap-2 min-w-0">
          <button
            type="button"
            onClick={onToggleBroadcastMute}
            title={
              settings.broadcastMuted
                ? 'Unmute Soundboard in Mic'
                : 'Mute Soundboard in Mic (Voice keeps passing through)'
            }
            className={cn(
              'p-1 rounded-md transition-colors shrink-0 cursor-pointer',
              settings.broadcastMuted
                ? 'bg-zinc-800 text-red-400'
                : 'text-zinc-300 hover:bg-zinc-800 hover:text-white'
            )}
          >
            <Mic className="w-3.5 h-3.5" />
          </button>
          <div className="flex-1 min-w-0 space-y-1">
            <div className="flex items-center justify-between text-[10px] text-zinc-400 leading-none">
              <span className="truncate">Mic SFX</span>
              <span className="font-mono text-zinc-200">
                {settings.broadcastMuted ? 'MUTE' : `${Math.round(settings.broadcastVolume * 100)}%`}
              </span>
            </div>
            <Slider
              min={0}
              max={1}
              step={0.01}
              value={[settings.broadcastVolume]}
              onValueChange={([val]) => onBroadcastVolumeChange(val)}
            />
          </div>
        </div>
      </div>
    </header>
  );
};

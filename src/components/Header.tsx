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
  ArrowDownRight,
} from 'lucide-react';
import { AudioPlaybackStatus, AudioSettings, VirtualMicStatus } from '../types';
import { Button } from './ui/button';
import { Slider } from './ui/slider';
import { Badge } from './ui/badge';
import { cn } from '../lib/utils';

interface HeaderProps {
  settings: AudioSettings;
  playbackStatus: AudioPlaybackStatus;
  virtualMicStatus: VirtualMicStatus | null;
  onAddSoundClick: () => void;
  onOpenSettings: () => void;
  onStopAll: () => void;
  onToggleOverlay: () => void;
  onDockBottomRight: () => void;
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
  onDockBottomRight,
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

  let statusLabel = 'Ready';
  let dotClass = 'bg-zinc-500';
  if (routingHealthy) {
    statusLabel = playbackStatus.micActive ? 'Voice Live' : 'Live';
    dotClass = playbackStatus.micActive ? 'bg-emerald-400 animate-pulse' : 'bg-emerald-400';
  } else if (!driverReady || !vmicAttached) {
    statusLabel = driverReady ? 'Standby' : 'Setup';
    dotClass = 'bg-amber-400';
  } else if (!playbackStatus.micStreamActive || playbackStatus.lastAudioError) {
    statusLabel = 'Check Mic';
    dotClass = 'bg-red-400';
  }

  const tooltip =
    playbackStatus.lastAudioError ||
    virtualMicStatus?.statusMessage ||
    'Audio routing status';

  return (
    <header className="bg-zinc-950 border-b border-zinc-800/90 select-none shrink-0">
      {/* Top Minimal Brand + Actions */}
      <div className="flex items-center justify-between px-3 py-2 gap-2">
        <div className="flex items-center gap-2 min-w-0">
          <div className="w-6 h-6 rounded-md bg-zinc-100 text-zinc-950 flex items-center justify-center shrink-0">
            <Radio className="w-3.5 h-3.5" />
          </div>
          <span className="text-xs font-semibold tracking-tight text-zinc-50 truncate">
            Memeboard
          </span>
          <button
            type="button"
            onClick={onOpenSettings}
            title={tooltip}
            className="cursor-pointer"
          >
            <Badge variant="muted" className="hover:border-zinc-600">
              <span className={cn('h-1.5 w-1.5 rounded-full', dotClass)} />
              <span>{statusLabel}</span>
            </Badge>
          </button>
        </div>

        <div className="flex items-center gap-1 shrink-0">
          <Button
            type="button"
            variant={isPlaying ? 'default' : 'ghost'}
            size="icon"
            onClick={onStopAll}
            title={`Panic Stop All (${settings.panicHotkey || 'Unbound'})`}
          >
            <Square className="w-3 h-3 fill-current" />
          </Button>

          <Button
            type="button"
            variant="ghost"
            size="icon"
            onClick={onToggleOverlay}
            title={`Toggle Overlay (${settings.overlayHotkey || 'Unbound'})`}
          >
            <Layers className="w-3.5 h-3.5" />
          </Button>

          <Button
            type="button"
            variant="ghost"
            size="icon"
            onClick={onDockBottomRight}
            title="Dock to Bottom-Right"
          >
            <ArrowDownRight className="w-3.5 h-3.5" />
          </Button>

          <Button
            type="button"
            variant="ghost"
            size="icon"
            onClick={onOpenSettings}
            title="Audio, Virtual Mic & Hotkey Settings"
          >
            <Settings className="w-3.5 h-3.5" />
          </Button>

          <Button
            type="button"
            variant="default"
            size="xs"
            onClick={onAddSoundClick}
            className="ml-0.5"
          >
            <Plus className="w-3 h-3" />
            <span>Add</span>
          </Button>
        </div>
      </div>

      {/* Dense Dual-Mix Strip (Always visible in compact vertical layout) */}
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

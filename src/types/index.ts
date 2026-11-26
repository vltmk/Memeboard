export type PlaybackMode = 'restart' | 'stop' | 'overlap';

export interface SoundItem {
  id: string;
  name: string;
  emoji: string;
  filePath: string;
  category: string;
  volume: number;
  hotkey: string | null;
  trimStart: number;
  trimEnd: number;
  duration: number;
  peaks: number[];
  playbackMode: PlaybackMode;
  normalizedGain: number;
  createdAt: number;
}

export interface AudioDevice {
  id: string;
  name: string;
  isDefault: boolean;
  isInput: boolean;
  isVirtual: boolean;
}

export interface AudioDevicesResult {
  inputDevices: AudioDevice[];
  outputDevices: AudioDevice[];
  currentInput: string | null;
  currentBroadcast: string | null;
  currentMonitor: string | null;
}

export interface AudioSettings {
  schemaVersion: number;
  micDeviceId: string | null;
  broadcastDeviceId: string | null;
  monitorDeviceId: string | null;
  micVolume: number;
  monitorVolume: number;
  broadcastVolume: number;
  monitorMuted: boolean;
  broadcastMuted: boolean;
  voiceDuckingEnabled: boolean;
  voiceDuckingDb: number;
  loudnessTargetLufs: number;
  panicHotkey: string;
  overlayHotkey: string;
  autoSwitchDefaultMic: boolean;
  aecSafeMonitor: boolean;
}

export interface SoundboardState {
  sounds: SoundItem[];
  settings: AudioSettings;
}

export interface AudioPlaybackStatus {
  playingSoundIds: string[];
  activeClipCount: number;
  micActive: boolean;
  micStreamActive: boolean;
  broadcastStreamActive: boolean;
  monitorStreamActive: boolean;
  lastAudioError: string | null;
}

export interface VirtualMicStatus {
  driverInstalled: boolean;
  serviceInstalled: boolean;
  virtualDeviceAttached: boolean;
  virtualMicIsDefault: boolean;
  savedPhysicalMic: string | null;
  statusMessage: string;
}

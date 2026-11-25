import { invoke } from '@tauri-apps/api/core';
import { open } from '@tauri-apps/plugin-dialog';
import {
  AudioDevicesResult,
  AudioPlaybackStatus,
  SoundItem,
  SoundboardState,
  VirtualMicStatus,
} from '../types';

export const isTauriEnvironment = (): boolean => {
  return (
    typeof window !== 'undefined' &&
    Boolean((window as unknown as { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__)
  );
};

export const api = {
  async getAudioDevices(): Promise<AudioDevicesResult> {
    return await invoke<AudioDevicesResult>('get_audio_devices');
  },

  async setAudioDevices(
    micDeviceId: string | null,
    broadcastDeviceId: string | null,
    monitorDeviceId: string | null
  ): Promise<void> {
    return await invoke('set_audio_devices', {
      micDeviceId,
      broadcastDeviceId,
      monitorDeviceId,
    });
  },

  async playSound(soundId: string): Promise<void> {
    return await invoke('play_sound', { soundId });
  },

  async previewSound(
    soundId: string,
    trimStart: number,
    trimEnd: number,
    volume: number,
    seekStart?: number
  ): Promise<void> {
    return await invoke('preview_sound', {
      soundId,
      trimStart,
      trimEnd,
      volume,
      seekStart: seekStart ?? null,
    });
  },

  async stopSound(soundId: string): Promise<void> {
    return await invoke('stop_sound', { soundId });
  },

  async stopPreview(soundId: string): Promise<void> {
    return await invoke('stop_preview', { soundId });
  },

  async stopAllSounds(): Promise<void> {
    return await invoke('stop_all_sounds');
  },

  async setVolumes(
    micVolume: number,
    monitorVolume: number,
    broadcastVolume: number,
    monitorMuted: boolean,
    broadcastMuted: boolean
  ): Promise<void> {
    return await invoke('set_volumes', {
      micVolume,
      monitorVolume,
      broadcastVolume,
      monitorMuted,
      broadcastMuted,
    });
  },

  async setDuckingSettings(enabled: boolean, duckingDb: number): Promise<void> {
    return await invoke('set_ducking_settings', { enabled, duckingDb });
  },

  async getPlaybackStatus(): Promise<AudioPlaybackStatus> {
    return await invoke<AudioPlaybackStatus>('get_playback_status');
  },

  async importSoundFile(
    sourcePath: string,
    name?: string,
    emoji?: string,
    category?: string
  ): Promise<SoundItem> {
    return await invoke<SoundItem>('import_sound_file', {
      sourcePath,
      name: name ?? null,
      emoji: emoji ?? null,
      category: category ?? null,
    });
  },

  async pickAudioFile(): Promise<string[] | null> {
    const selected = await open({
      multiple: true,
      title: 'Add Sound Effects',
      filters: [
        {
          name: 'Audio Files',
          extensions: ['mp3', 'wav', 'ogg', 'flac', 'm4a', 'aac', 'opus', 'webm'],
        },
      ],
    });

    if (!selected) return null;
    if (Array.isArray(selected)) return selected;
    return [selected];
  },

  async deleteSound(soundId: string, filePath: string): Promise<void> {
    return await invoke('delete_sound', { soundId, filePath });
  },

  async loadSoundboardState(): Promise<SoundboardState> {
    return await invoke<SoundboardState>('load_soundboard_state');
  },

  async saveSoundboardState(newState: SoundboardState): Promise<void> {
    return await invoke('save_soundboard_state', { newState });
  },

  async toggleOverlayWindow(): Promise<boolean> {
    return await invoke<boolean>('toggle_overlay_window');
  },

  async getOverlayVisibility(): Promise<boolean> {
    return await invoke<boolean>('get_overlay_visibility');
  },

  async setHotkeyRecording(paused: boolean): Promise<void> {
    return await invoke('set_hotkey_recording', { paused });
  },

  async getVirtualMicStatus(): Promise<VirtualMicStatus> {
    return await invoke<VirtualMicStatus>('get_virtual_mic_status');
  },

  async installVirtualDriver(): Promise<VirtualMicStatus> {
    return await invoke<VirtualMicStatus>('install_virtual_driver');
  },

  async setAutoSwitchMic(enabled: boolean): Promise<VirtualMicStatus> {
    return await invoke<VirtualMicStatus>('set_auto_switch_mic', { enabled });
  },

  async restoreOriginalMic(): Promise<VirtualMicStatus> {
    return await invoke<VirtualMicStatus>('restore_original_mic');
  },
};

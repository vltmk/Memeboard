import { invoke } from '@tauri-apps/api/core';
import { open, save } from '@tauri-apps/plugin-dialog';
import {
  AudioDevicesResult,
  AudioPlaybackStatus,
  PackImportResult,
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

  async setAecSafeMonitor(enabled: boolean): Promise<void> {
    return await invoke('set_aec_safe_monitor', { enabled });
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

  async exportSoundFile(soundId: string, destPath: string): Promise<void> {
    return await invoke('export_sound_file', { soundId, destPath });
  },

  async pickExportAudioPath(defaultName: string, extension: string): Promise<string | null> {
    const ext = extension.replace(/^\./, '').toLowerCase() || 'wav';
    const cleanName = defaultName.replace(/[<>:"/\\|?*]/g, '_').trim() || 'sound';
    const selected = await save({
      title: 'Download Audio File',
      defaultPath: `${cleanName}.${ext}`,
      filters: [{ name: `${ext.toUpperCase()} Audio`, extensions: [ext] }],
    });
    return selected ?? null;
  },

  async exportPack(destPath: string, categoryFilter?: string | null): Promise<number> {
    return await invoke<number>('export_pack', {
      destPath,
      categoryFilter: categoryFilter ?? null,
    });
  },

  async pickExportPackPath(defaultName: string): Promise<string | null> {
    const cleanName = defaultName.replace(/[<>:"/\\|?*]/g, '_').trim() || 'memeboard-pack';
    const selected = await save({
      title: 'Export Memeboard Pack',
      defaultPath: `${cleanName}.memeboard`,
      filters: [
        { name: 'Memeboard Pack (*.memeboard, *.zip)', extensions: ['memeboard', 'zip'] },
      ],
    });
    return selected ?? null;
  },

  async importPack(packPath: string): Promise<PackImportResult> {
    return await invoke<PackImportResult>('import_pack', { packPath });
  },

  async pickImportPackPath(): Promise<string | null> {
    const selected = await open({
      multiple: false,
      title: 'Import Memeboard Pack or ZIP',
      filters: [
        { name: 'Memeboard Pack & ZIP', extensions: ['memeboard', 'zip'] },
      ],
    });
    if (!selected) return null;
    return Array.isArray(selected) ? selected[0] ?? null : selected;
  },

  async isMainDocked(): Promise<boolean> {
    return await invoke<boolean>('is_main_docked');
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

  async dockMainWindow(): Promise<void> {
    return await invoke('dock_main_window');
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

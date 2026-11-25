/**
 * Format seconds to MM:SS.ms format
 */
export function formatTime(seconds: number): string {
  if (isNaN(seconds) || seconds < 0) return '0:00.0';
  const mins = Math.floor(seconds / 60);
  const secs = Math.floor(seconds % 60);
  const ms = Math.floor((seconds % 1) * 10);
  return `${mins}:${secs < 10 ? '0' : ''}${secs}.${ms}`;
}

/**
 * Format seconds to M:SS
 */
export function formatDuration(seconds: number): string {
  if (isNaN(seconds) || seconds < 0) return '0:00';
  const mins = Math.floor(seconds / 60);
  const secs = Math.floor(seconds % 60);
  return `${mins}:${secs < 10 ? '0' : ''}${secs}`;
}

/**
 * Draw interactive waveform on canvas
 */
export function drawWaveform(
  canvas: HTMLCanvasElement,
  peaks: number[],
  trimStart: number,
  trimEnd: number,
  duration: number,
  currentTime: number,
  options?: {
    playedColor?: string;
    unplayedColor?: string;
    dimmedColor?: string;
    playheadColor?: string;
    trimHandleColor?: string;
  }
) {
  const ctx = canvas.getContext('2d');
  if (!ctx) return;

  const dpr = window.devicePixelRatio || 1;
  const width = canvas.width / dpr;
  const height = canvas.height / dpr;

  ctx.clearRect(0, 0, canvas.width, canvas.height);
  ctx.save();
  ctx.scale(dpr, dpr);

  const playedColor = options?.playedColor || '#5865f2';
  const unplayedColor = options?.unplayedColor || '#4e5058';
  const dimmedColor = options?.dimmedColor || '#232428';
  const playheadColor = options?.playheadColor || '#ffffff';

  if (!peaks || peaks.length === 0) {
    // Draw placeholder bars
    peaks = Array.from({ length: 60 }, (_, i) => Math.sin(i * 0.2) * 0.4 + 0.5);
  }

  const barCount = peaks.length;
  const barGap = 2;
  const barWidth = Math.max(2, (width - barCount * barGap) / barCount);
  const effectiveDuration = duration > 0 ? duration : 1;

  for (let i = 0; i < barCount; i++) {
    const x = i * (barWidth + barGap);
    const timeAtBar = (i / barCount) * effectiveDuration;
    const isTrimmedOut = timeAtBar < trimStart || timeAtBar > trimEnd;
    const isPlayed = !isTrimmedOut && timeAtBar <= currentTime;

    const rawHeight = peaks[i] * (height - 8);
    const barHeight = Math.max(3, rawHeight);
    const y = (height - barHeight) / 2;

    if (isTrimmedOut) {
      ctx.fillStyle = dimmedColor;
    } else if (isPlayed) {
      ctx.fillStyle = playedColor;
    } else {
      ctx.fillStyle = unplayedColor;
    }

    // Rounded rectangle bar
    const radius = Math.min(barWidth / 2, 2);
    ctx.beginPath();
    ctx.roundRect(x, y, barWidth, barHeight, radius);
    ctx.fill();
  }

  // Draw playhead if within bounds
  if (currentTime >= 0 && currentTime <= effectiveDuration) {
    const playheadX = (currentTime / effectiveDuration) * width;
    ctx.strokeStyle = playheadColor;
    ctx.lineWidth = 2;
    ctx.beginPath();
    ctx.moveTo(playheadX, 0);
    ctx.lineTo(playheadX, height);
    ctx.stroke();
  }

  ctx.restore();
}

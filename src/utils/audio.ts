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

  const playedColor = options?.playedColor || '#fafafa';
  const unplayedColor = options?.unplayedColor || '#3f3f46';
  const dimmedColor = options?.dimmedColor || '#18181b';
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

  // Draw visible trim boundary handles (start & end) so users can always locate and drag them
  const clampedStart = Math.max(0, Math.min(trimStart, effectiveDuration));
  const clampedEnd = Math.max(clampedStart, Math.min(trimEnd, effectiveDuration));
  const startX = Math.max(2, Math.min((clampedStart / effectiveDuration) * width, width - 2));
  const endX = Math.max(2, Math.min((clampedEnd / effectiveDuration) * width, width - 2));

  // Subtle highlight tint inside active trimmed window
  if (endX > startX) {
    ctx.fillStyle = 'rgba(250, 250, 250, 0.06)';
    ctx.fillRect(startX, 0, endX - startX, height);
  }

  const startHandleColor = options?.trimHandleColor || '#fafafa';
  const endHandleColor = options?.trimHandleColor || '#a1a1aa';

  const drawHandle = (x: number, color: string, alignLeft: boolean) => {
    ctx.strokeStyle = color;
    ctx.lineWidth = 2;
    ctx.beginPath();
    ctx.moveTo(x, 0);
    ctx.lineTo(x, height);
    ctx.stroke();

    ctx.fillStyle = color;
    const pillW = 8;
    const pillH = 14;
    const pillX = alignLeft ? x - 1 : x - pillW + 1;
    ctx.beginPath();
    ctx.roundRect(pillX, 2, pillW, pillH, 3);
    ctx.fill();
  };

  drawHandle(startX, startHandleColor, true);
  drawHandle(endX, endHandleColor, false);

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

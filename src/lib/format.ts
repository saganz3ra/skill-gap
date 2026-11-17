const UNITS = ["B", "KB", "MB", "GB", "TB"];

/** 1536 → "1,5 KB". Base 1024, que é o que o Explorer do Windows mostra. */
export function formatBytes(bytes: number): string {
  let value = bytes;
  let unit = 0;
  while (value >= 1024 && unit < UNITS.length - 1) {
    value /= 1024;
    unit++;
  }
  const digits = unit === 0 || value >= 100 ? 0 : 1;
  return `${value.toLocaleString("pt-BR", { maximumFractionDigits: digits })} ${UNITS[unit]}`;
}

/** Taxa em bytes/s → "3,2 MB/s". */
export function formatRate(bytesPerSec: number): string {
  return `${formatBytes(bytesPerSec)}/s`;
}

/** 17100 ms → "17,1 s" */
export function formatMs(ms: number): string {
  if (ms < 1000) return `${ms} ms`;
  return `${(ms / 1000).toLocaleString("pt-BR", { maximumFractionDigits: 1 })} s`;
}

/** Nome do .exe dentro de um comando de inicialização (mesma regra do backend). */
export function exeOf(command: string): string | null {
  const c = command.trim();
  let path: string;
  if (c.startsWith('"')) {
    path = c.slice(1).split('"')[0] ?? "";
  } else {
    const i = c.toLowerCase().indexOf(".exe");
    path = i >= 0 ? c.slice(0, i + 4) : c;
  }
  const file = path.split(/[\\/]/).pop()?.trim();
  return file || null;
}
